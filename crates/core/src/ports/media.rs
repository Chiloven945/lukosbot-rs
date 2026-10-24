//! Media loading boundary used by command handlers.
//!
//! Commands describe what media they need; the application supplies an implementation that can
//! resolve platform files and perform bounded HTTP downloads. Core deliberately does not know
//! about reqwest, platform SDKs, or rendering libraries.

use async_trait::async_trait;

use lukosbot_domain::{MediaContent, MediaRef, PlatformKind};

use crate::error::BoxError;
use crate::ports::PlatformError;

/// Restrictions applied to a media load.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediaLoadPolicy {
    /// Exact hostnames allowed for URL sources. An empty list rejects all URLs.
    pub allowed_hosts: &'static [&'static str],
    /// Maximum materialized size in bytes.
    pub max_bytes: u64,
    /// Require HTTPS for URL sources.
    pub https_only: bool,
}

impl MediaLoadPolicy {
    pub const fn new(allowed_hosts: &'static [&'static str], max_bytes: u64) -> Self {
        Self {
            allowed_hosts,
            max_bytes,
            https_only: true,
        }
    }

    /// Policy for sources that do not fetch a URL, such as platform file IDs.
    pub const fn platform(max_bytes: u64) -> Self {
        Self::new(&[], max_bytes)
    }
}

/// Loads a media reference into bytes.
#[async_trait]
pub trait MediaLoader: Send + Sync {
    async fn load(
        &self,
        reference: &MediaRef,
        policy: MediaLoadPolicy,
    ) -> Result<MediaContent, MediaLoadError>;
}

/// A media loading failure.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum MediaLoadError {
    #[error("media source is not supported")]
    UnsupportedSource { part: &'static str },

    #[error("the adapter for {platform} is not available")]
    PlatformUnavailable { platform: PlatformKind },

    #[error("the media URL is invalid")]
    InvalidUrl,

    #[error("the media host is not allowed")]
    RejectedHost,

    #[error("media exceeds the {limit} byte limit")]
    TooLarge { limit: u64 },

    #[error("media loading timed out")]
    Timeout,

    #[error("media transfer failed")]
    Http {
        #[source]
        source: BoxError,
    },

    #[error("platform media loading failed")]
    Platform {
        #[source]
        source: PlatformError,
    },

    #[error("media content is invalid: {message}")]
    InvalidContent { message: String },

    #[error("media loading was cancelled")]
    Cancelled,
}

/// Default loader for runtimes whose handlers do not use media.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnsupportedMediaLoader;

#[async_trait]
impl MediaLoader for UnsupportedMediaLoader {
    async fn load(
        &self,
        _reference: &MediaRef,
        _policy: MediaLoadPolicy,
    ) -> Result<MediaContent, MediaLoadError> {
        Err(MediaLoadError::UnsupportedSource {
            part: "media loading is not configured",
        })
    }
}
