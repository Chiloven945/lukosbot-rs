use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use reqwest::StatusCode;
use reqwest::header::RETRY_AFTER;
use tokio::sync::Mutex;
use tokio::time::Instant;
use url::Url;

use lukosbot_integrations::{E621Artist, E621Md5, E621Post, E621Service, IntegrationError};

use crate::http::HttpClient;
use crate::http::HttpError;

const DEFAULT_BASE: &str = "https://e621.net/";
const DEFAULT_UA: &str = "LukosBot/0.1 (+https://github.com/Chiloven945/lukosbot-rs)";
const MAX_PAGE: u32 = 750;
const MAX_LIMIT: u32 = 320;

mod dto;

use self::dto::{ArtistDto, PostDto, PostsResponse, SinglePostResponse};

pub struct E621Client {
    http: Arc<HttpClient>,
    base: Url,
    user_agent: String,
    request_interval: Duration,
    last_request: Mutex<Option<Instant>>,
}

impl E621Client {
    pub fn new(http: Arc<HttpClient>) -> Self {
        Self::with_user_agent(http, DEFAULT_UA)
    }

    pub fn with_user_agent(http: Arc<HttpClient>, user_agent: impl Into<String>) -> Self {
        let user_agent = user_agent.into();
        Self {
            http,
            base: Url::parse(DEFAULT_BASE).expect("constant E621 base URL is valid"),
            user_agent: if user_agent.trim().is_empty() {
                DEFAULT_UA.to_owned()
            } else {
                user_agent
            },
            request_interval: Duration::from_secs(1),
            last_request: Mutex::new(None),
        }
    }

    pub fn with_interval(mut self, interval: Duration) -> Self {
        self.request_interval = interval;
        self
    }

    #[cfg(test)]
    fn with_base_url(http: Arc<HttpClient>, base: Url) -> Self {
        Self {
            http,
            base,
            user_agent: DEFAULT_UA.to_owned(),
            request_interval: Duration::ZERO,
            last_request: Mutex::new(None),
        }
    }

    fn endpoint(&self, segments: &[&str]) -> Result<Url, IntegrationError> {
        let mut url = self.base.clone();
        let mut path = url
            .path_segments_mut()
            .map_err(|_| IntegrationError::InvalidInput {
                message: "invalid E621 base URL".to_owned(),
            })?;
        for segment in segments {
            path.push(segment);
        }
        drop(path);
        Ok(url)
    }

    async fn throttle(&self) {
        let mut last_request = self.last_request.lock().await;
        if let Some(previous) = *last_request {
            let next = previous + self.request_interval;
            let now = Instant::now();
            if next > now {
                tokio::time::sleep(next - now).await;
            }
        }
        *last_request = Some(Instant::now());
    }

    async fn get_json(&self, url: Url) -> Result<String, IntegrationError> {
        let mut attempts = 0_u8;
        loop {
            self.throttle().await;
            let request = self
                .http
                .raw()
                .get(url.clone())
                .header(reqwest::header::USER_AGENT, &self.user_agent)
                .header(reqwest::header::ACCEPT, "application/json");
            let response = self.http.execute(request).await.map_err(map_http_error)?;
            let status = response.status();
            if !status.is_success() {
                let retry_after = retry_after(&response);
                if matches!(
                    status,
                    StatusCode::TOO_MANY_REQUESTS | StatusCode::SERVICE_UNAVAILABLE
                ) && attempts == 0
                {
                    attempts += 1;
                    let wait = retry_after.unwrap_or(Duration::from_secs(1));
                    tokio::time::sleep(wait.min(Duration::from_secs(2))).await;
                    continue;
                }
                return Err(map_status(status, retry_after));
            }

            let bytes = self
                .http
                .read_limited(response)
                .await
                .map_err(map_http_error)?;
            return String::from_utf8(bytes).map_err(|error| IntegrationError::InvalidResponse {
                source: Box::new(error),
            });
        }
    }
}

#[async_trait]
impl E621Service for E621Client {
    async fn get_artist(&self, id_or_name: &str) -> Result<Option<E621Artist>, IntegrationError> {
        let url = self.endpoint(&["artists", &format!("{id_or_name}.json")])?;
        match self.get_json(url).await {
            Ok(text) => {
                let dto: ArtistDto = serde_json::from_str(&text).map_err(json_error)?;
                Ok(Some(dto.into_model()))
            }
            Err(IntegrationError::NotFound) => Ok(None),
            Err(error) => Err(error),
        }
    }

    async fn search_artists(
        &self,
        query: &str,
        page: NonZeroU32,
        limit: NonZeroU32,
    ) -> Result<Vec<E621Artist>, IntegrationError> {
        validate_paging(page, limit)?;
        let mut url = self.endpoint(&["artists.json"])?;
        url.query_pairs_mut()
            .append_pair("search[any_name_matches]", query)
            .append_pair("page", &page.to_string())
            .append_pair("limit", &limit.to_string());
        let text = self.get_json(url).await?;
        let dtos: Vec<ArtistDto> = serde_json::from_str(&text).map_err(json_error)?;
        Ok(dtos.into_iter().map(ArtistDto::into_model).collect())
    }

    async fn get_post(&self, id: NonZeroU32) -> Result<Option<E621Post>, IntegrationError> {
        let mut url = self.endpoint(&["posts", &format!("{id}.json")])?;
        add_v2(&mut url);
        match self.get_json(url).await {
            Ok(text) => {
                let response: SinglePostResponse =
                    serde_json::from_str(&text).map_err(json_error)?;
                Ok(Some(response.into_post().into_model()))
            }
            Err(IntegrationError::NotFound) => Ok(None),
            Err(error) => Err(error),
        }
    }

    async fn search_posts(
        &self,
        tags: &str,
        page: NonZeroU32,
        limit: NonZeroU32,
    ) -> Result<Vec<E621Post>, IntegrationError> {
        validate_paging(page, limit)?;
        let mut url = self.endpoint(&["posts.json"])?;
        add_v2(&mut url);
        url.query_pairs_mut()
            .append_pair("tags", tags)
            .append_pair("page", &page.to_string())
            .append_pair("limit", &limit.to_string());
        let text = self.get_json(url).await?;
        let response: PostsResponse = serde_json::from_str(&text).map_err(json_error)?;
        Ok(response
            .into_posts()
            .into_iter()
            .map(PostDto::into_model)
            .collect())
    }

    async fn find_post_by_md5(&self, md5: &E621Md5) -> Result<Option<E621Post>, IntegrationError> {
        let mut url = self.endpoint(&["posts.json"])?;
        add_v2(&mut url);
        url.query_pairs_mut()
            .append_pair("md5", md5.as_str())
            .append_pair("limit", "1")
            .append_pair("page", "1");
        let text = self.get_json(url).await?;
        let response: SinglePostResponse = serde_json::from_str(&text).map_err(json_error)?;
        Ok(Some(response.into_post().into_model()))
    }
}

fn add_v2(url: &mut Url) {
    url.query_pairs_mut()
        .append_pair("v2", "true")
        .append_pair("mode", "extended");
}

fn validate_paging(page: NonZeroU32, limit: NonZeroU32) -> Result<(), IntegrationError> {
    if page.get() > MAX_PAGE {
        return Err(IntegrationError::InvalidInput {
            message: format!("page must be between 1 and {MAX_PAGE}"),
        });
    }
    if limit.get() > MAX_LIMIT {
        return Err(IntegrationError::InvalidInput {
            message: format!("limit must be between 1 and {MAX_LIMIT}"),
        });
    }
    Ok(())
}

fn map_http_error(error: HttpError) -> IntegrationError {
    match error {
        HttpError::Timeout => IntegrationError::Timeout,
        HttpError::BodyTooLarge { limit } => IntegrationError::ResponseTooLarge { limit },
        HttpError::Json(source) => IntegrationError::InvalidResponse {
            source: Box::new(source),
        },
        HttpError::Status { status, .. } => map_status(status, None),
        other => IntegrationError::Transport {
            source: Box::new(other),
        },
    }
}

fn map_status(status: StatusCode, retry_after: Option<Duration>) -> IntegrationError {
    match status {
        StatusCode::NOT_FOUND => IntegrationError::NotFound,
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => IntegrationError::Unauthorized,
        StatusCode::TOO_MANY_REQUESTS | StatusCode::SERVICE_UNAVAILABLE => {
            IntegrationError::RateLimited { retry_after }
        }
        status if status.is_server_error() => IntegrationError::Upstream {
            status: status.as_u16(),
        },
        status => IntegrationError::Upstream {
            status: status.as_u16(),
        },
    }
}

fn retry_after(response: &reqwest::Response) -> Option<Duration> {
    response
        .headers()
        .get(RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(Duration::from_secs)
}

fn json_error(error: serde_json::Error) -> IntegrationError {
    IntegrationError::InvalidResponse {
        source: Box::new(error),
    }
}

#[cfg(test)]
mod tests;
