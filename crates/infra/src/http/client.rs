use std::future::Future;
use std::time::Duration;

use reqwest::{RequestBuilder, Response, StatusCode};
use serde::de::DeserializeOwned;
use thiserror::Error;
use url::Url;

/// HTTP client construction settings.
#[derive(Debug, Clone)]
pub struct HttpSettings {
    pub connect_timeout: Duration,
    pub request_timeout: Duration,
    pub max_response_bytes: u64,
    pub user_agent: String,
    pub proxy: Option<String>,
}

impl Default for HttpSettings {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(5),
            request_timeout: Duration::from_secs(20),
            max_response_bytes: 10 * 1024 * 1024,
            user_agent: "LukosBot/0.1 (+https://github.com/Chiloven945/lukosbot-rs)".to_owned(),
            proxy: None,
        }
    }
}

/// The application-wide HTTP client.
///
/// The client never adds credentials automatically and never retries on its own. Callers that
/// need retries must opt in through [`RetryPolicy`] and must guarantee idempotency themselves.
#[derive(Debug, Clone)]
pub struct HttpClient {
    inner: reqwest::Client,
    max_response_bytes: u64,
}

impl HttpClient {
    pub fn new(settings: HttpSettings) -> Result<Self, HttpError> {
        // System proxy auto-detection is deliberately disabled: proxy behaviour is owned by
        // application configuration so deployments are reproducible.
        let mut builder = reqwest::Client::builder()
            .connect_timeout(settings.connect_timeout)
            .timeout(settings.request_timeout)
            .user_agent(settings.user_agent)
            .no_proxy();

        if let Some(proxy) = settings.proxy.as_deref() {
            let proxy = reqwest::Proxy::all(proxy).map_err(HttpError::Proxy)?;
            builder = builder.proxy(proxy);
        }

        let inner = builder.build().map_err(HttpError::Build)?;

        Ok(Self {
            inner,
            max_response_bytes: settings.max_response_bytes,
        })
    }

    /// Access to the underlying client for callers that need a custom request.
    pub fn raw(&self) -> &reqwest::Client {
        &self.inner
    }

    /// Sends a request and rejects non-success status codes with a bounded body snippet.
    pub async fn send(&self, request: RequestBuilder) -> Result<Response, HttpError> {
        let response = request.send().await.map_err(map_request_error)?;
        let status = response.status();

        if !status.is_success() {
            let snippet = read_snippet(response).await?;
            return Err(HttpError::Status { status, snippet });
        }

        Ok(response)
    }

    pub async fn get(&self, url: Url) -> Result<Response, HttpError> {
        self.send(self.inner.get(url)).await
    }

    /// Fetches a response body while enforcing the configured maximum on the accumulated
    /// bytes, not just on the advertised `Content-Length`.
    pub async fn get_bytes(&self, url: Url) -> Result<Vec<u8>, HttpError> {
        let response = self.get(url).await?;
        self.read_limited(response).await
    }

    pub async fn get_json<T: DeserializeOwned>(&self, url: Url) -> Result<T, HttpError> {
        let bytes = self.get_bytes(url).await?;
        serde_json::from_slice(&bytes).map_err(HttpError::Json)
    }

    /// Streams the response body into memory, failing as soon as the limit is exceeded.
    pub async fn read_limited(&self, mut response: Response) -> Result<Vec<u8>, HttpError> {
        if let Some(length) = response.content_length()
            && length > self.max_response_bytes
        {
            return Err(HttpError::BodyTooLarge {
                limit: self.max_response_bytes,
            });
        }

        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(HttpError::Body)? {
            let total = body.len() as u64 + chunk.len() as u64;
            if total > self.max_response_bytes {
                return Err(HttpError::BodyTooLarge {
                    limit: self.max_response_bytes,
                });
            }
            body.extend_from_slice(&chunk);
        }

        Ok(body)
    }
}

fn map_request_error(error: reqwest::Error) -> HttpError {
    if error.is_timeout() {
        HttpError::Timeout
    } else if error.is_connect() {
        HttpError::Connect(error)
    } else {
        HttpError::Request(error)
    }
}

async fn read_snippet(mut response: Response) -> Result<String, HttpError> {
    const MAX_SNIPPET: usize = 512;

    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(HttpError::Body)? {
        let remaining = MAX_SNIPPET.saturating_sub(body.len());
        if remaining == 0 {
            break;
        }
        if chunk.len() >= remaining {
            body.extend_from_slice(&chunk[..remaining]);
            break;
        }
        body.extend_from_slice(&chunk);
    }

    Ok(String::from_utf8_lossy(&body).into_owned())
}

/// HTTP boundary failure.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum HttpError {
    #[error("failed to build the HTTP client")]
    Build(#[source] reqwest::Error),

    #[error("invalid proxy configuration")]
    Proxy(#[source] reqwest::Error),

    #[error("the request timed out")]
    Timeout,

    #[error("the connection failed")]
    Connect(#[source] reqwest::Error),

    #[error("the HTTP request failed")]
    Request(#[source] reqwest::Error),

    #[error("unexpected HTTP status {status}")]
    Status { status: StatusCode, snippet: String },

    #[error("the response body exceeds {limit} bytes")]
    BodyTooLarge { limit: u64 },

    #[error("failed to read the response body")]
    Body(#[source] reqwest::Error),

    #[error("failed to decode a JSON response")]
    Json(#[source] serde_json::Error),
}

impl HttpError {
    /// Whether a request that is known to be idempotent may be retried.
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Timeout | Self::Connect(_) => true,
            Self::Status { status, .. } => {
                status.is_server_error() || *status == StatusCode::TOO_MANY_REQUESTS
            }
            _ => false,
        }
    }
}

/// An explicit exponential backoff policy.
///
/// This type never applies itself automatically. Callers wrap only operations for which a
/// repeated request is safe.
#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub initial_backoff: Duration,
    pub max_backoff: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_backoff: Duration::from_millis(200),
            max_backoff: Duration::from_secs(2),
        }
    }
}

impl RetryPolicy {
    pub async fn run<F, Fut, T>(&self, mut operation: F) -> Result<T, HttpError>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<T, HttpError>>,
    {
        let attempts = self.max_attempts.max(1);
        let mut backoff = self.initial_backoff;
        let mut attempt = 1;

        loop {
            match operation().await {
                Ok(value) => return Ok(value),
                Err(error) if attempt < attempts && error.is_retryable() => {
                    let jitter = Duration::from_millis(fastrand::u64(0..=100));
                    tokio::time::sleep((backoff + jitter).min(self.max_backoff)).await;
                    backoff = (backoff * 2).min(self.max_backoff);
                    attempt += 1;
                }
                Err(error) => return Err(error),
            }
        }
    }
}
