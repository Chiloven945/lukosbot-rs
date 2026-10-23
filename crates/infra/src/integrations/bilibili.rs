use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::header::{ACCEPT, LOCATION, USER_AGENT};
use reqwest::{Response, StatusCode};
use serde::Deserialize;
use url::Url;

use lukosbot_integrations::{
    BILIBILI_HOSTS, BilibiliOwner, BilibiliService, BilibiliStats, BilibiliTarget, BilibiliVideo,
    BoxError, IntegrationError, VideoId, extract_video_id,
};

use crate::http::{HttpClient, HttpError};

const TOTAL_BUDGET: Duration = Duration::from_secs(12);
const VIEW_BUDGET: Duration = Duration::from_secs(8);
const SHORT_LINK_BUDGET: Duration = Duration::from_secs(6);
const RELATION_BUDGET: Duration = Duration::from_secs(4);
const MAX_REDIRECTS: usize = 5;
const HTML_ACCEPT: &str = "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8";
const JSON_ACCEPT: &str = "application/json";
const BILIBILI_USER_AGENT: &str =
    "Mozilla/5.0 (compatible; LukosBot/0.1; +https://github.com/Chiloven945/lukosbot-rs)";

const VIEW_URL: &str = "https://api.bilibili.com/x/web-interface/view";
const RELATION_URL: &str = "https://api.bilibili.com/x/relation/stat";

#[derive(Debug, Clone)]
struct BilibiliEndpoints {
    view: Url,
    relation: Url,
}

impl Default for BilibiliEndpoints {
    fn default() -> Self {
        Self {
            view: Url::parse(VIEW_URL).expect("the built-in Bilibili view URL is valid"),
            relation: Url::parse(RELATION_URL)
                .expect("the built-in Bilibili relation URL is valid"),
        }
    }
}

pub struct BilibiliClient {
    http: Arc<HttpClient>,
    no_redirect_http: Arc<HttpClient>,
    endpoints: BilibiliEndpoints,
    test_allowed_hosts: Vec<String>,
}

impl BilibiliClient {
    /// `no_redirect_http` must be built from the same settings with redirects disabled.
    pub fn new(http: Arc<HttpClient>, no_redirect_http: Arc<HttpClient>) -> Self {
        Self {
            http,
            no_redirect_http,
            endpoints: BilibiliEndpoints::default(),
            test_allowed_hosts: Vec::new(),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_endpoints(
        http: Arc<HttpClient>,
        no_redirect_http: Arc<HttpClient>,
        view: Url,
        relation: Url,
        test_allowed_hosts: Vec<String>,
    ) -> Self {
        Self {
            http,
            no_redirect_http,
            endpoints: BilibiliEndpoints { view, relation },
            test_allowed_hosts,
        }
    }

    async fn view(&self, id: &VideoId) -> Result<ViewDataDto, IntegrationError> {
        let mut url = self.endpoints.view.clone();
        match id {
            VideoId::Bv(bvid) => {
                url.query_pairs_mut().append_pair("bvid", bvid);
            }
            VideoId::Av(aid) => {
                url.query_pairs_mut().append_pair("aid", &aid.to_string());
            }
        }

        let response: ViewResponse = self.request_json(url).await?;
        if response.code != 0 {
            return Err(IntegrationError::NotFound);
        }
        response.data.ok_or(IntegrationError::NotFound)
    }

    async fn follower_count(&self, mid: u64) -> Option<u64> {
        let mut url = self.endpoints.relation.clone();
        url.query_pairs_mut().append_pair("vmid", &mid.to_string());
        match self.request_json::<RelationResponse>(url).await {
            Ok(response) if response.code == 0 => response.data.and_then(|data| data.follower),
            Ok(_) => None,
            Err(error) => {
                tracing::debug!(
                    category = error.category(),
                    "Bilibili relation query failed; follower count is unknown"
                );
                None
            }
        }
    }

    async fn request_json<T: for<'de> Deserialize<'de>>(
        &self,
        url: Url,
    ) -> Result<T, IntegrationError> {
        let request = self
            .http
            .raw()
            .get(url)
            .header(ACCEPT, JSON_ACCEPT)
            .header(USER_AGENT, BILIBILI_USER_AGENT);
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

    async fn resolve_short_link(
        &self,
        input: &Url,
        deadline: tokio::time::Instant,
    ) -> Result<VideoId, IntegrationError> {
        let mut current = input.clone();
        for _ in 0..MAX_REDIRECTS {
            if !self.is_allowed_host(&current) {
                return Err(IntegrationError::InvalidInput {
                    message: "the short link redirects to a disallowed host".to_owned(),
                });
            }
            if let Some(video) = extract_video_id(current.as_str()) {
                return Ok(video);
            }

            let request = self
                .no_redirect_http
                .raw()
                .get(current.clone())
                .header(ACCEPT, HTML_ACCEPT)
                .header(USER_AGENT, BILIBILI_USER_AGENT);
            let response =
                tokio::time::timeout_at(deadline, self.no_redirect_http.execute(request))
                    .await
                    .map_err(|_| IntegrationError::Timeout)?
                    .map_err(map_http_error)?;

            let status = response.status();
            if status.is_redirection() {
                let location = response
                    .headers()
                    .get(LOCATION)
                    .and_then(|value| value.to_str().ok())
                    .ok_or_else(|| IntegrationError::InvalidResponse {
                        source: "redirect without Location".into(),
                    })?;
                current =
                    current
                        .join(location)
                        .map_err(|error| IntegrationError::InvalidResponse {
                            source: Box::new(error),
                        })?;
                continue;
            }

            if status.is_success() {
                let body = self
                    .no_redirect_http
                    .read_limited(response)
                    .await
                    .map_err(map_http_error)?;
                if let Some(video) = extract_video_id(&String::from_utf8_lossy(&body)) {
                    return Ok(video);
                }
            }

            return Err(IntegrationError::NotFound);
        }

        Err(IntegrationError::InvalidInput {
            message: "too many short-link redirects".to_owned(),
        })
    }

    fn is_allowed_host(&self, url: &Url) -> bool {
        if !matches!(url.scheme(), "http" | "https") {
            return false;
        }
        if !self.test_allowed_hosts.is_empty() {
            return url.host_str().is_some_and(|host| {
                self.test_allowed_hosts
                    .iter()
                    .any(|allowed| host.eq_ignore_ascii_case(allowed))
            }) && url.port().is_some();
        }
        url.port().is_none()
            && url.host_str().is_some_and(|host| {
                BILIBILI_HOSTS
                    .iter()
                    .any(|allowed| host.eq_ignore_ascii_case(allowed))
            })
    }
}

#[async_trait]
impl BilibiliService for BilibiliClient {
    async fn query(&self, target: &BilibiliTarget) -> Result<BilibiliVideo, IntegrationError> {
        let deadline = tokio::time::Instant::now() + TOTAL_BUDGET;
        let target_id = match target {
            BilibiliTarget::Video(id) => id.clone(),
            BilibiliTarget::ShortLink(url) => {
                let short_deadline = deadline.min(tokio::time::Instant::now() + SHORT_LINK_BUDGET);
                self.resolve_short_link(url, short_deadline).await?
            }
        };

        let view_deadline = deadline.min(tokio::time::Instant::now() + VIEW_BUDGET);
        let view = tokio::time::timeout_at(view_deadline, self.view(&target_id))
            .await
            .map_err(|_| IntegrationError::Timeout)??;

        let id = view
            .bvid
            .as_deref()
            .and_then(extract_video_id)
            .or_else(|| extract_video_id(&target_id.normalized()))
            .unwrap_or(target_id.clone());
        let url = id.canonical_url().map_err(IntegrationError::from)?;

        let owner_mid = view.owner.as_ref().and_then(|owner| owner.mid);
        let followers = match owner_mid.filter(|mid| *mid > 0) {
            Some(mid) => {
                let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
                if remaining.is_zero() {
                    None
                } else {
                    tokio::time::timeout(remaining.min(RELATION_BUDGET), self.follower_count(mid))
                        .await
                        .ok()
                        .flatten()
                }
            }
            None => None,
        };

        let stats = view.stat.unwrap_or_default();
        let pages = view.pages.as_ref().map_or(1, |pages| pages.len().max(1));
        let pages = pages.max(view.videos.unwrap_or(0)).max(1) as u32;
        let published_at = view
            .pubdate
            .filter(|seconds| *seconds > 0)
            .and_then(|seconds| DateTime::<Utc>::from_timestamp(seconds, 0));

        Ok(BilibiliVideo {
            id,
            url,
            title: view.title.unwrap_or_default(),
            category: view.tname,
            description: view.desc,
            cover_url: view.pic.and_then(|value| Url::parse(&value).ok()),
            published_at,
            owner: BilibiliOwner {
                mid: owner_mid,
                name: view.owner.and_then(|owner| owner.name),
                followers,
            },
            stats: BilibiliStats {
                views: stats.view,
                danmaku: stats.danmaku,
                replies: stats.reply,
                likes: stats.like,
                coins: stats.coin,
                favorites: stats.favorite,
                shares: stats.share,
            },
            pages,
        })
    }
}

fn check_status(response: &Response) -> Result<(), IntegrationError> {
    let status = response.status();
    if status.is_success() {
        Ok(())
    } else {
        Err(map_status(status))
    }
}

fn map_status(status: StatusCode) -> IntegrationError {
    match status {
        StatusCode::NOT_FOUND => IntegrationError::NotFound,
        StatusCode::UNAUTHORIZED => IntegrationError::Unauthorized,
        StatusCode::TOO_MANY_REQUESTS => IntegrationError::RateLimited { retry_after: None },
        _ => IntegrationError::Upstream {
            status: status.as_u16(),
        },
    }
}

fn map_http_error(error: HttpError) -> IntegrationError {
    match error {
        HttpError::Timeout => IntegrationError::Timeout,
        HttpError::BodyTooLarge { limit } => IntegrationError::ResponseTooLarge { limit },
        HttpError::Json(error) => IntegrationError::InvalidResponse {
            source: Box::new(error),
        },
        HttpError::Status { status, .. } => map_status(status),
        other => IntegrationError::Transport {
            source: Box::new(other) as BoxError,
        },
    }
}

#[derive(Debug, Deserialize)]
struct ViewResponse {
    #[serde(default)]
    code: i64,
    #[serde(default)]
    data: Option<ViewDataDto>,
}

#[derive(Debug, Default, Deserialize)]
struct ViewDataDto {
    #[serde(default)]
    bvid: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    tname: Option<String>,
    #[serde(default)]
    desc: Option<String>,
    #[serde(default)]
    pic: Option<String>,
    #[serde(default)]
    pubdate: Option<i64>,
    #[serde(default)]
    owner: Option<OwnerDto>,
    #[serde(default)]
    stat: Option<StatDto>,
    #[serde(default)]
    pages: Option<Vec<serde_json::Value>>,
    #[serde(default)]
    videos: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct OwnerDto {
    #[serde(default)]
    mid: Option<u64>,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct StatDto {
    #[serde(default)]
    view: u64,
    #[serde(default)]
    danmaku: u64,
    #[serde(default)]
    reply: u64,
    #[serde(default)]
    favorite: u64,
    #[serde(default)]
    coin: u64,
    #[serde(default)]
    share: u64,
    #[serde(default)]
    like: u64,
}

#[derive(Debug, Deserialize)]
struct RelationResponse {
    #[serde(default)]
    code: i64,
    #[serde(default)]
    data: Option<RelationDataDto>,
}

#[derive(Debug, Deserialize)]
struct RelationDataDto {
    #[serde(default)]
    follower: Option<u64>,
}

#[cfg(test)]
mod tests;
