//! Message handlers and their dependency context.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use thiserror::Error;
use tokio_util::sync::CancellationToken;

use lukosbot_domain::InboundMessage;

use crate::error::BoxError;
use crate::messaging::outbound::{OutboundHandle, SendError};
use crate::ports::{StateStore, StateStoreError};

mod ping;

pub use ping::PingHandler;

/// Dependencies available to message handlers.
///
/// The context intentionally exposes the outbound handle rather than platform SDK clients, and
/// the state store rather than SQLx. It does not carry the whole application configuration.
pub struct HandlerContext {
    outbound: OutboundHandle,
    state: Arc<dyn StateStore>,
    cancel: CancellationToken,
}

impl std::fmt::Debug for HandlerContext {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HandlerContext")
            .field("outbound", &self.outbound)
            .field("cancelled", &self.cancel.is_cancelled())
            .finish_non_exhaustive()
    }
}

impl HandlerContext {
    pub fn new(
        outbound: OutboundHandle,
        state: Arc<dyn StateStore>,
        cancel: CancellationToken,
    ) -> Self {
        Self {
            outbound,
            state,
            cancel,
        }
    }

    pub fn outbound(&self) -> &OutboundHandle {
        &self.outbound
    }

    pub fn state(&self) -> &Arc<dyn StateStore> {
        &self.state
    }

    /// Cancellation token for the current application lifetime.
    pub fn cancel(&self) -> &CancellationToken {
        &self.cancel
    }
}

/// A message handler invocation failure.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum HandlerError {
    #[error(transparent)]
    Outbound(#[from] SendError),

    #[error(transparent)]
    State(#[from] StateStoreError),

    #[error("handler timed out after {timeout:?}")]
    Timeout { timeout: Duration },

    #[error("{message}")]
    Failed {
        message: String,
        #[source]
        source: Option<BoxError>,
    },
}

impl HandlerError {
    pub fn failed(message: impl Into<String>) -> Self {
        Self::Failed {
            message: message.into(),
            source: None,
        }
    }
}

/// The stable entry point for message processing.
///
/// A handler failure affects only the current message; the dispatcher logs it and continues.
#[async_trait]
pub trait MessageHandler: Send + Sync {
    async fn handle(
        &self,
        context: &HandlerContext,
        message: InboundMessage,
    ) -> Result<(), HandlerError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_error_keeps_message() {
        let error = HandlerError::failed("boom");
        assert_eq!(error.to_string(), "boom");
    }
}
