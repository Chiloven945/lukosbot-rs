use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use reqwest::header::{ACCEPT, AUTHORIZATION, RETRY_AFTER};
use reqwest::{Response, StatusCode};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use url::Url;

use lukosbot_integrations::{
    BoxError, GitHubRepository, GitHubRepositoryBrief, GitHubSearchParams, GitHubSearchResult,
    GitHubService, GitHubUser, IntegrationError,
};

use crate::http::{HttpClient, HttpError};

const ACCEPT_GITHUB: &str = "application/vnd.github.v3+json";
const DEFAULT_API_BASE: &str = "https://api.github.com/";
const OPERATION_BUDGET: Duration = Duration::from_secs(8);

/// A GitHub REST client with an optional token.
///
/// The token is kept in a `SecretString` and only exposed while constructing an authorization
/// header. No request URL, header, or token is logged by this module.
pub struct GitHubClient {
    http: Arc<HttpClient>,
    token: Option<SecretString>,
    api_base: Url,
}

impl GitHubClient {
    pub fn new(http: Arc<HttpClient>, token: Option<SecretString>) -> Self {
        Self {
            http,
            token,
            api_base: Url::parse(DEFAULT_API_BASE).expect("the built-in GitHub API URL is valid"),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_api_base(
        http: Arc<HttpClient>,
        token: Option<SecretString>,
        api_base: Url,
    ) -> Self {
        Self {
            http,
            token,
            api_base,
        }
    }

    fn endpoint(&self, segments: &[&str]) -> Result<Url, IntegrationError> {
        let mut url = self.api_base.clone();
        url.path_segments_mut()
            .map_err(|_| IntegrationError::InvalidInput {
                message: "the GitHub API base cannot be used as a base URL".to_owned(),
            })?
            .extend(segments);
        Ok(url)
    }

    async fn get_json<T: for<'de> Deserialize<'de>>(
        &self,
        url: Url,
    ) -> Result<T, IntegrationError> {
        let mut request = self.http.raw().get(url).header(ACCEPT, ACCEPT_GITHUB);
        if let Some(token) = &self.token {
            request = request.header(AUTHORIZATION, format!("Bearer {}", token.expose_secret()));
        }

        let response = self.http.execute(request).await.map_err(map_http_error)?;
        check_status(&response)?;
        let bytes = self
            .http
            .read_limited(response)
            .await
            .map_err(map_http_error)?;
        serde_json::from_slice(&bytes).map_err(|error| IntegrationError::InvalidResponse {
            source: Box::new(error),
        })
    }

    async fn get_json_budgeted<T: for<'de> Deserialize<'de>>(
        &self,
        url: Url,
    ) -> Result<T, IntegrationError> {
        tokio::time::timeout(OPERATION_BUDGET, self.get_json(url))
            .await
            .map_err(|_| IntegrationError::Timeout)?
    }
}

#[async_trait]
impl GitHubService for GitHubClient {
    async fn user(&self, username: &str) -> Result<GitHubUser, IntegrationError> {
        let username = username.trim();
        if username.is_empty() {
            return Err(IntegrationError::InvalidInput {
                message: "GitHub username must not be empty".to_owned(),
            });
        }

        let url = self.endpoint(&["users", username])?;
        let dto: UserDto = self.get_json_budgeted(url).await?;
        Ok(dto.into_user())
    }

    async fn repository(
        &self,
        owner: &str,
        repository: &str,
    ) -> Result<GitHubRepository, IntegrationError> {
        let owner = owner.trim();
        let repository = repository.trim();
        if owner.is_empty() || repository.is_empty() {
            return Err(IntegrationError::InvalidInput {
                message: "GitHub owner and repository must not be empty".to_owned(),
            });
        }

        let url = self.endpoint(&["repos", owner, repository])?;
        let dto: RepositoryDto = self.get_json_budgeted(url).await?;
        Ok(dto.into_repository())
    }

    async fn search(
        &self,
        params: &GitHubSearchParams,
    ) -> Result<GitHubSearchResult, IntegrationError> {
        let query = params.query.trim();
        if query.is_empty() {
            return Err(IntegrationError::InvalidInput {
                message: "GitHub search query must not be empty".to_owned(),
            });
        }
        if !(1..=10).contains(&params.top) {
            return Err(IntegrationError::InvalidInput {
                message: "GitHub search top must be between 1 and 10".to_owned(),
            });
        }

        let mut url = self.endpoint(&["search", "repositories"])?;
        let full_query = match params
            .language
            .as_deref()
            .map(str::trim)
            .filter(|language| !language.is_empty())
        {
            Some(language) => format!("{query} language:{language}"),
            None => query.to_owned(),
        };
        {
            let mut pairs = url.query_pairs_mut();
            pairs.append_pair("q", &full_query);
            pairs.append_pair("per_page", &params.top.to_string());
            if let Some(sort) = params.sort {
                pairs.append_pair("sort", sort.as_str());
            }
            if let Some(order) = params.order {
                pairs.append_pair("order", order.as_str());
            }
        }

        let dto: SearchDto = self.get_json_budgeted(url).await?;
        let mut items = dto
            .items
            .into_iter()
            .map(RepositoryBriefDto::into_brief)
            .collect::<Vec<_>>();
        items.truncate(usize::from(params.top));

        Ok(GitHubSearchResult {
            total_count: dto.total_count,
            items,
        })
    }
}

fn check_status(response: &Response) -> Result<(), IntegrationError> {
    let status = response.status();
    if status.is_success() {
        return Ok(());
    }
    Err(map_status(response, status))
}

fn map_status(response: &Response, status: StatusCode) -> IntegrationError {
    match status {
        StatusCode::NOT_FOUND => IntegrationError::NotFound,
        StatusCode::UNAUTHORIZED => IntegrationError::Unauthorized,
        StatusCode::FORBIDDEN
            if (response
                .headers()
                .get("x-ratelimit-remaining")
                .and_then(|value| value.to_str().ok())
                == Some("0"))
                || response.headers().contains_key(RETRY_AFTER) =>
        {
            IntegrationError::RateLimited {
                retry_after: retry_after(response),
            }
        }
        StatusCode::FORBIDDEN => IntegrationError::Unauthorized,
        StatusCode::TOO_MANY_REQUESTS => IntegrationError::RateLimited {
            retry_after: retry_after(response),
        },
        _ => IntegrationError::Upstream {
            status: status.as_u16(),
        },
    }
}

fn retry_after(response: &Response) -> Option<Duration> {
    response
        .headers()
        .get(RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(Duration::from_secs)
}

fn map_http_error(error: HttpError) -> IntegrationError {
    match error {
        HttpError::Timeout => IntegrationError::Timeout,
        HttpError::BodyTooLarge { limit } => IntegrationError::ResponseTooLarge { limit },
        HttpError::Json(error) => IntegrationError::InvalidResponse {
            source: Box::new(error),
        },
        HttpError::Status { status, .. } => IntegrationError::Upstream {
            status: status.as_u16(),
        },
        other => IntegrationError::Transport {
            source: Box::new(other) as BoxError,
        },
    }
}

fn parse_optional_url(value: Option<String>) -> Option<Url> {
    value.and_then(|value| Url::parse(value.trim()).ok())
}

#[derive(Debug, Deserialize)]
struct UserDto {
    #[serde(default)]
    login: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    html_url: Option<String>,
    #[serde(default)]
    public_repos: u64,
    #[serde(default)]
    followers: u64,
    #[serde(default)]
    following: u64,
}

impl UserDto {
    fn into_user(self) -> GitHubUser {
        GitHubUser {
            login: self.login,
            name: self.name,
            html_url: parse_optional_url(self.html_url),
            public_repos: self.public_repos,
            followers: self.followers,
            following: self.following,
        }
    }
}

#[derive(Debug, Deserialize)]
struct RepositoryDto {
    #[serde(default)]
    full_name: Option<String>,
    #[serde(default)]
    html_url: Option<String>,
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    stargazers_count: u64,
    #[serde(default)]
    forks_count: u64,
    #[serde(default)]
    description: Option<String>,
}

impl RepositoryDto {
    fn into_repository(self) -> GitHubRepository {
        GitHubRepository {
            full_name: self
                .full_name
                .filter(|name| !name.trim().is_empty())
                .unwrap_or_else(|| "未知仓库".to_owned()),
            html_url: parse_optional_url(self.html_url),
            language: self.language,
            stargazers_count: self.stargazers_count,
            forks_count: self.forks_count,
            description: self.description,
        }
    }
}

#[derive(Debug, Deserialize)]
struct SearchDto {
    #[serde(default)]
    total_count: u64,
    #[serde(default)]
    items: Vec<RepositoryBriefDto>,
}

#[derive(Debug, Deserialize)]
struct RepositoryBriefDto {
    #[serde(default)]
    full_name: Option<String>,
    #[serde(default)]
    html_url: Option<String>,
    #[serde(default)]
    stargazers_count: u64,
}

impl RepositoryBriefDto {
    fn into_brief(self) -> GitHubRepositoryBrief {
        GitHubRepositoryBrief {
            full_name: self
                .full_name
                .filter(|name| !name.trim().is_empty())
                .unwrap_or_else(|| "未知仓库".to_owned()),
            html_url: parse_optional_url(self.html_url),
            stargazers_count: self.stargazers_count,
        }
    }
}

// The DTOs above intentionally mirror only the fields used by command output.

#[cfg(test)]
mod tests;
