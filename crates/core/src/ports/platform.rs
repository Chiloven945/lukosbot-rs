//! The platform adapter port.

use async_trait::async_trait;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use lukosbot_domain::{
    ChatAddress, InboundMessage, MediaContent, OutboundMessage, PlatformKind, SentMessage,
};

use crate::error::BoxError;

/// The channel adapters use to submit normalised inbound messages.
pub type InboundSender = mpsc::Sender<InboundMessage>;

/// A platform integration.
///
/// # Lifecycle contract
///
/// - [`PlatformAdapter::run`] returns `Ok(())` **only** when `cancel` was triggered and the
///   adapter stopped cleanly. Authentication failures, disconnects, and any other unexpected
///   termination must return an error so the runtime can shut the application down.
/// - [`PlatformAdapter::shutdown`] must be idempotent and bounded by the caller's timeout.
/// - [`PlatformAdapter::send`] must not silently drop unsupported message parts; it returns
///   [`PlatformError::Unsupported`] instead.
#[async_trait]
pub trait PlatformAdapter: Send + Sync {
    fn kind(&self) -> PlatformKind;

    async fn run(
        &self,
        inbound: InboundSender,
        cancel: CancellationToken,
    ) -> Result<(), PlatformError>;

    async fn send(
        &self,
        target: &ChatAddress,
        message: OutboundMessage,
    ) -> Result<SentMessage, PlatformError>;

    /// Downloads a platform file identifier into memory.
    ///
    /// Adapters that cannot resolve platform files keep the fail-closed default.
    async fn load_platform_file(&self, _file_id: &str) -> Result<MediaContent, PlatformError> {
        Err(PlatformError::Unsupported {
            platform: self.kind(),
            part: "platform file download",
        })
    }

    async fn shutdown(&self) -> Result<(), PlatformError>;
}

/// A platform boundary failure.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PlatformError {
    #[error("the {platform} adapter has not been started")]
    NotStarted { platform: PlatformKind },

    #[error("message targets {target}, but the adapter handles {adapter}")]
    WrongPlatform {
        target: PlatformKind,
        adapter: PlatformKind,
    },

    #[error("the {platform} adapter cannot represent {part}")]
    Unsupported {
        platform: PlatformKind,
        part: &'static str,
    },

    #[error("authentication with {platform} failed")]
    Authentication {
        platform: PlatformKind,
        #[source]
        source: BoxError,
    },

    #[error("transport failure while talking to {platform}")]
    Transport {
        platform: PlatformKind,
        #[source]
        source: BoxError,
    },

    #[error("{platform} rejected the request: {message}")]
    Api {
        platform: PlatformKind,
        message: String,
    },

    #[error("failed to map a {platform} object into the domain model: {message}")]
    Mapping {
        platform: PlatformKind,
        message: String,
    },

    #[error("the {platform} adapter stopped unexpectedly")]
    Disconnected { platform: PlatformKind },
}

impl PlatformError {
    pub fn transport(platform: PlatformKind, source: impl Into<BoxError>) -> Self {
        Self::Transport {
            platform,
            source: source.into(),
        }
    }

    pub fn authentication(platform: PlatformKind, source: impl Into<BoxError>) -> Self {
        Self::Authentication {
            platform,
            source: source.into(),
        }
    }
}
