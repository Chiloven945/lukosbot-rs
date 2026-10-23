use async_trait::async_trait;
use url::Url;

use crate::error::IntegrationError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHubUser {
    pub login: String,
    pub name: Option<String>,
    pub html_url: Option<Url>,
    pub public_repos: u64,
    pub followers: u64,
    pub following: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHubRepository {
    pub full_name: String,
    pub html_url: Option<Url>,
    pub language: Option<String>,
    pub stargazers_count: u64,
    pub forks_count: u64,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHubRepositoryBrief {
    pub full_name: String,
    pub html_url: Option<Url>,
    pub stargazers_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHubSearchResult {
    pub total_count: u64,
    pub items: Vec<GitHubRepositoryBrief>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitHubSort {
    Stars,
    Updated,
}

impl GitHubSort {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Stars => "stars",
            Self::Updated => "updated",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitHubOrder {
    Asc,
    Desc,
}

impl GitHubOrder {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Asc => "asc",
            Self::Desc => "desc",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHubSearchParams {
    pub query: String,
    pub top: u8,
    pub language: Option<String>,
    pub sort: Option<GitHubSort>,
    pub order: Option<GitHubOrder>,
}

#[async_trait]
pub trait GitHubService: Send + Sync {
    async fn user(&self, username: &str) -> Result<GitHubUser, IntegrationError>;

    async fn repository(
        &self,
        owner: &str,
        repository: &str,
    ) -> Result<GitHubRepository, IntegrationError>;

    async fn search(
        &self,
        params: &GitHubSearchParams,
    ) -> Result<GitHubSearchResult, IntegrationError>;
}
