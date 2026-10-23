use std::sync::Arc;
use std::time::Duration;

use reqwest::Response;
use reqwest::StatusCode;
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use url::Url;

use lukosbot_integrations::{BoxError, IntegrationError, MusicLink, TrackInfo};

use crate::http::{HttpClient, HttpError};

const API_BASE: &str = "https://api-v2.soundcloud.com/";
const OPERATION_BUDGET: Duration = Duration::from_secs(8);

pub struct SoundCloudClient {
    http: Arc<HttpClient>,
    client_id: SecretString,
    api_base: Url,
}

impl SoundCloudClient {
    pub fn new(http: Arc<HttpClient>, client_id: SecretString) -> Self {
        Self {
            http,
            client_id,
            api_base: Url::parse(API_BASE).expect("the built-in SoundCloud API URL is valid"),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_api_base(
        http: Arc<HttpClient>,
        client_id: SecretString,
        api_base: Url,
    ) -> Self {
        Self {
            http,
            client_id,
            api_base,
        }
    }

    pub async fn search(&self, query: &str) -> Result<TrackInfo, IntegrationError> {
        tokio::time::timeout(OPERATION_BUDGET, self.search_inner(query))
            .await
            .map_err(|_| IntegrationError::Timeout)?
    }

    async fn search_inner(&self, query: &str) -> Result<TrackInfo, IntegrationError> {
        let query = query.trim();
        if query.is_empty() {
            return Err(IntegrationError::InvalidInput {
                message: "SoundCloud search query must not be empty".to_owned(),
            });
        }

        let mut url = self.endpoint(&["search", "tracks"])?;
        {
            let mut pairs = url.query_pairs_mut();
            pairs.append_pair("q", query);
            pairs.append_pair("client_id", self.client_id.expose_secret());
            pairs.append_pair("limit", "1");
        }

        let response: SearchDto = self.get_json(url).await?;
        let track = response
            .collection
            .into_iter()
            .next()
            .ok_or(IntegrationError::NotFound)?;
        track.into_track()
    }

    pub async fn resolve_link(&self, link: &MusicLink) -> Result<TrackInfo, IntegrationError> {
        tokio::time::timeout(OPERATION_BUDGET, self.resolve_link_inner(link))
            .await
            .map_err(|_| IntegrationError::Timeout)?
    }

    async fn resolve_link_inner(&self, link: &MusicLink) -> Result<TrackInfo, IntegrationError> {
        let mut url = self.endpoint(&["resolve"])?;
        {
            let mut pairs = url.query_pairs_mut();
            pairs.append_pair("url", link.url.as_str());
            pairs.append_pair("client_id", self.client_id.expose_secret());
        }

        let track: TrackDto = self.get_json(url).await?;
        track.into_track()
    }

    fn endpoint(&self, segments: &[&str]) -> Result<Url, IntegrationError> {
        let mut url = self.api_base.clone();
        url.path_segments_mut()
            .map_err(|_| IntegrationError::InvalidInput {
                message: "SoundCloud API base is not hierarchical".to_owned(),
            })?
            .extend(segments);
        Ok(url)
    }

    async fn get_json<T: for<'de> Deserialize<'de>>(
        &self,
        url: Url,
    ) -> Result<T, IntegrationError> {
        let request = self.http.raw().get(url);
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
}

fn check_status(response: &Response) -> Result<(), IntegrationError> {
    let status = response.status();
    if status.is_success() {
        return Ok(());
    }
    Err(map_status(status))
}

fn map_status(status: StatusCode) -> IntegrationError {
    match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => IntegrationError::Unauthorized,
        StatusCode::NOT_FOUND => IntegrationError::NotFound,
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
struct SearchDto {
    #[serde(default)]
    collection: Vec<TrackDto>,
}

#[derive(Debug, Deserialize)]
struct TrackDto {
    #[serde(default)]
    id: Option<u64>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    publisher_metadata: Option<PublisherMetadataDto>,
    #[serde(default)]
    user: Option<UserDto>,
    #[serde(default)]
    sets: Option<Vec<String>>,
    #[serde(default)]
    playlist: Option<String>,
    #[serde(default)]
    artwork_url: Option<String>,
    #[serde(default)]
    permalink_url: Option<String>,
    #[serde(default)]
    full_duration: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct PublisherMetadataDto {
    #[serde(default)]
    artist: Option<String>,
    #[serde(default)]
    album_title: Option<String>,
    #[serde(default)]
    release_title: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UserDto {
    #[serde(default)]
    username: Option<String>,
}

impl TrackDto {
    fn into_track(self) -> Result<TrackInfo, IntegrationError> {
        let id = self.id.ok_or(IntegrationError::NotFound)?.to_string();
        let publisher = self.publisher_metadata;
        let artist = publisher
            .as_ref()
            .and_then(|value| value.artist.clone())
            .or_else(|| self.user.and_then(|user| user.username))
            .unwrap_or_default();
        let album = publisher
            .as_ref()
            .and_then(|value| first_non_blank([&value.album_title, &value.release_title]))
            .or_else(|| {
                self.playlist
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned)
            })
            .or_else(|| self.sets.as_ref().and_then(|sets| sets.first().cloned()));
        let duration = self
            .full_duration
            .filter(|millis| *millis > 0)
            .map(Duration::from_millis);

        Ok(TrackInfo {
            platform: lukosbot_integrations::MusicPlatform::SoundCloud,
            id,
            title: self.title.unwrap_or_default(),
            artist,
            album,
            cover_url: self.artwork_url.and_then(|value| Url::parse(&value).ok()),
            track_url: self.permalink_url.and_then(|value| Url::parse(&value).ok()),
            duration,
        })
    }
}

fn first_non_blank(values: [&Option<String>; 2]) -> Option<String> {
    values
        .into_iter()
        .filter_map(|value| value.as_deref())
        .map(str::trim)
        .find(|value| !value.is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests;
