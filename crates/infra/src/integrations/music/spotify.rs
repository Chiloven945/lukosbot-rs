use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use reqwest::header::{ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use reqwest::{Response, StatusCode};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use tokio::sync::Mutex;
use tokio::time::Instant;
use url::Url;

use lukosbot_integrations::{BoxError, IntegrationError, MusicLink, TrackInfo};

use crate::http::{HttpClient, HttpError};

const TOKEN_MARGIN: Duration = Duration::from_secs(60);
const OPERATION_BUDGET: Duration = Duration::from_secs(12);
const TOKEN_URL: &str = "https://accounts.spotify.com/api/token";
const API_BASE: &str = "https://api.spotify.com/v1";

#[derive(Debug, Clone)]
struct SpotifyEndpoints {
    token: Url,
    api: Url,
}

impl Default for SpotifyEndpoints {
    fn default() -> Self {
        Self {
            token: Url::parse(TOKEN_URL).expect("the built-in Spotify token URL is valid"),
            api: Url::parse(API_BASE).expect("the built-in Spotify API URL is valid"),
        }
    }
}

struct TokenCache {
    access_token: Option<SecretString>,
    expires_at: Option<Instant>,
}

impl TokenCache {
    fn valid(&self) -> Option<SecretString> {
        let expires_at = self.expires_at?;
        if Instant::now() + TOKEN_MARGIN < expires_at {
            self.access_token.clone()
        } else {
            None
        }
    }
}

pub struct SpotifyClient {
    http: Arc<HttpClient>,
    client_id: SecretString,
    client_secret: SecretString,
    endpoints: SpotifyEndpoints,
    token: Arc<Mutex<TokenCache>>,
}

impl SpotifyClient {
    pub fn new(
        http: Arc<HttpClient>,
        client_id: SecretString,
        client_secret: SecretString,
    ) -> Self {
        Self {
            http,
            client_id,
            client_secret,
            endpoints: SpotifyEndpoints::default(),
            token: Arc::new(Mutex::new(TokenCache {
                access_token: None,
                expires_at: None,
            })),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_endpoints(
        http: Arc<HttpClient>,
        client_id: SecretString,
        client_secret: SecretString,
        token: Url,
        api: Url,
    ) -> Self {
        Self {
            http,
            client_id,
            client_secret,
            endpoints: SpotifyEndpoints { token, api },
            token: Arc::new(Mutex::new(TokenCache {
                access_token: None,
                expires_at: None,
            })),
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
                message: "Spotify search query must not be empty".to_owned(),
            });
        }

        let mut url = self.endpoint(&["search"])?;
        {
            let mut pairs = url.query_pairs_mut();
            pairs.append_pair("q", query);
            pairs.append_pair("type", "track");
            pairs.append_pair("limit", "1");
        }

        let response: SearchDto = self.authorized_get(url).await?;
        let track = response
            .tracks
            .and_then(|tracks| tracks.items.into_iter().next())
            .ok_or(IntegrationError::NotFound)?;
        track.into_track()
    }

    pub async fn resolve_link(&self, link: &MusicLink) -> Result<TrackInfo, IntegrationError> {
        tokio::time::timeout(OPERATION_BUDGET, self.resolve_link_inner(link))
            .await
            .map_err(|_| IntegrationError::Timeout)?
    }

    async fn resolve_link_inner(&self, link: &MusicLink) -> Result<TrackInfo, IntegrationError> {
        let id = link
            .url
            .path_segments()
            .into_iter()
            .flatten()
            .nth(1)
            .filter(|id| !id.is_empty())
            .ok_or(IntegrationError::NotFound)?;
        let url = self.endpoint(&["tracks", id])?;
        let track: TrackDto = self.authorized_get(url).await?;
        track.into_track()
    }

    fn endpoint(&self, segments: &[&str]) -> Result<Url, IntegrationError> {
        let mut url = self.endpoints.api.clone();
        url.path_segments_mut()
            .map_err(|_| IntegrationError::InvalidInput {
                message: "Spotify API base is not hierarchical".to_owned(),
            })?
            .extend(segments);
        Ok(url)
    }

    async fn authorized_get<T: for<'de> Deserialize<'de>>(
        &self,
        url: Url,
    ) -> Result<T, IntegrationError> {
        let token = self.access_token().await?;
        match self.get_json(url.clone(), &token).await {
            Err(IntegrationError::Unauthorized) => {
                self.invalidate().await;
                let token = self.access_token().await?;
                self.get_json(url, &token).await
            }
            result => result,
        }
    }

    async fn get_json<T: for<'de> Deserialize<'de>>(
        &self,
        url: Url,
        token: &SecretString,
    ) -> Result<T, IntegrationError> {
        let request = self
            .http
            .raw()
            .get(url)
            .header(ACCEPT, "application/json")
            .header(AUTHORIZATION, format!("Bearer {}", token.expose_secret()));
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

    async fn access_token(&self) -> Result<SecretString, IntegrationError> {
        {
            let cache = self.token.lock().await;
            if let Some(token) = cache.valid() {
                return Ok(token);
            }
        }

        let mut cache = self.token.lock().await;
        if let Some(token) = cache.valid() {
            return Ok(token);
        }

        let request = self
            .http
            .raw()
            .post(self.endpoints.token.clone())
            .header(ACCEPT, "application/json")
            .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
            .basic_auth(
                self.client_id.expose_secret(),
                Some(self.client_secret.expose_secret()),
            )
            .body("grant_type=client_credentials");
        let response = self.http.execute(request).await.map_err(map_http_error)?;
        check_status(&response)?;
        let bytes = self
            .http
            .read_limited(response)
            .await
            .map_err(map_http_error)?;
        let dto: TokenDto =
            serde_json::from_slice(&bytes).map_err(|error| IntegrationError::InvalidResponse {
                source: Box::new(error),
            })?;

        let access_token = dto
            .access_token
            .filter(|token| !token.trim().is_empty())
            .ok_or_else(|| IntegrationError::InvalidResponse {
                source: "Spotify returned an empty access token".into(),
            })?;
        let expires_in = dto
            .expires_in
            .filter(|seconds| *seconds > 0)
            .ok_or_else(|| IntegrationError::InvalidResponse {
                source: "Spotify returned an invalid token lifetime".into(),
            })?;
        let lifetime =
            Duration::from_secs(expires_in.saturating_sub(TOKEN_MARGIN.as_secs()).max(1));
        let expires_at = Instant::now().checked_add(lifetime).ok_or_else(|| {
            IntegrationError::InvalidResponse {
                source: "Spotify token lifetime is too large".into(),
            }
        })?;

        let token = SecretString::from(access_token);
        cache.access_token = Some(token.clone());
        cache.expires_at = Some(expires_at);
        Ok(token)
    }

    async fn invalidate(&self) {
        let mut cache = self.token.lock().await;
        cache.access_token = None;
        cache.expires_at = None;
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
struct TokenDto {
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct SearchDto {
    #[serde(default)]
    tracks: Option<TracksDto>,
}

#[derive(Debug, Deserialize)]
struct TracksDto {
    #[serde(default)]
    items: Vec<TrackDto>,
}

#[derive(Debug, Deserialize)]
struct TrackDto {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    artists: Vec<ArtistDto>,
    #[serde(default)]
    album: Option<AlbumDto>,
    #[serde(default)]
    external_urls: Option<HashMap<String, String>>,
    #[serde(default)]
    duration_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct ArtistDto {
    #[serde(default)]
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AlbumDto {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    images: Vec<ImageDto>,
}

#[derive(Debug, Deserialize)]
struct ImageDto {
    #[serde(default)]
    url: Option<String>,
}

impl TrackDto {
    fn into_track(self) -> Result<TrackInfo, IntegrationError> {
        let id = self
            .id
            .filter(|id| !id.trim().is_empty())
            .ok_or(IntegrationError::NotFound)?;
        let title = self.name.unwrap_or_default();
        let artist = self
            .artists
            .into_iter()
            .find_map(|artist| artist.name)
            .unwrap_or_default();
        let album = self.album.as_ref().and_then(|album| album.name.clone());
        let cover_url = self
            .album
            .and_then(|album| album.images.into_iter().find_map(|image| image.url))
            .and_then(|url| Url::parse(&url).ok());
        let track_url = self
            .external_urls
            .and_then(|urls| urls.get("spotify").cloned())
            .and_then(|url| Url::parse(&url).ok())
            .or_else(|| Url::parse(&format!("https://open.spotify.com/track/{id}")).ok());
        let duration = self
            .duration_ms
            .filter(|millis| *millis > 0)
            .map(Duration::from_millis);

        Ok(TrackInfo {
            platform: lukosbot_integrations::MusicPlatform::Spotify,
            id,
            title,
            artist,
            album,
            cover_url,
            track_url,
            duration,
        })
    }
}

#[cfg(test)]
mod tests;
