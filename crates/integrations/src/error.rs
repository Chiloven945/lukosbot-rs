use std::error::Error;
use std::time::Duration;

use crate::ip::IpProviderId;

/// A boxed source error retained for diagnostics but never rendered to chat users.
pub type BoxError = Box<dyn Error + Send + Sync + 'static>;

/// Stable categories shared by every integration.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum IntegrationError {
    #[error("invalid integration input: {message}")]
    InvalidInput { message: String },

    #[error("the requested resource was not found")]
    NotFound,

    #[error("provider `{provider}` is not configured")]
    NotConfigured { provider: String },

    #[error("the upstream service rate limited the request")]
    RateLimited { retry_after: Option<Duration> },

    #[error("the upstream service rejected the credentials")]
    Unauthorized,

    #[error("the request timed out")]
    Timeout,

    #[error("a transport error occurred")]
    Transport {
        #[source]
        source: BoxError,
    },

    #[error("the upstream response was invalid")]
    InvalidResponse {
        #[source]
        source: BoxError,
    },

    #[error("the upstream service returned HTTP {status}")]
    Upstream { status: u16 },

    #[error("all IP providers failed")]
    AllProvidersFailed { providers: Vec<IpProviderId> },

    #[error("the response exceeds {limit} bytes")]
    ResponseTooLarge { limit: u64 },

    #[error("the integration is unavailable: {message}")]
    Unavailable { message: String },
}

impl IntegrationError {
    /// A bounded label suitable for structured logs.
    pub fn category(&self) -> &'static str {
        match self {
            Self::InvalidInput { .. } => "invalid_input",
            Self::NotFound => "not_found",
            Self::NotConfigured { .. } => "not_configured",
            Self::RateLimited { .. } => "rate_limited",
            Self::Unauthorized => "unauthorized",
            Self::Timeout => "timeout",
            Self::Transport { .. } => "transport",
            Self::InvalidResponse { .. } => "invalid_response",
            Self::Upstream { .. } => "upstream",
            Self::AllProvidersFailed { .. } => "all_providers_failed",
            Self::ResponseTooLarge { .. } => "response_too_large",
            Self::Unavailable { .. } => "unavailable",
        }
    }

    pub fn timeout() -> Self {
        Self::Timeout
    }
}

impl From<url::ParseError> for IntegrationError {
    fn from(error: url::ParseError) -> Self {
        Self::InvalidResponse {
            source: Box::new(error),
        }
    }
}
