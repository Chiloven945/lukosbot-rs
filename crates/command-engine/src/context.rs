use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use lukosbot_core::handler::HandlerContext;
use lukosbot_core::messaging::outbound::SendError;
use lukosbot_core::ports::StateStore;
use lukosbot_domain::{ChatAddress, InboundMessage, OutboundMessage, Sender, SentMessage};

use crate::registry::CommandRegistry;

/// Dependencies exposed to command executors.
///
/// The context intentionally does not expose platform SDK clients, the whole application
/// configuration, or database connections.
pub struct CommandContext<'a> {
    message: &'a InboundMessage,
    handler: &'a HandlerContext,
    registry: &'a CommandRegistry,
    prefix: &'a str,
}

impl<'a> CommandContext<'a> {
    pub fn new(
        message: &'a InboundMessage,
        handler: &'a HandlerContext,
        registry: &'a CommandRegistry,
        prefix: &'a str,
    ) -> Self {
        Self {
            message,
            handler,
            registry,
            prefix,
        }
    }

    pub fn message(&self) -> &InboundMessage {
        self.message
    }

    pub fn address(&self) -> &ChatAddress {
        &self.message.address
    }

    pub fn sender(&self) -> &Sender {
        &self.message.sender
    }

    pub fn registry(&self) -> &CommandRegistry {
        self.registry
    }

    pub fn prefix(&self) -> &str {
        self.prefix
    }

    pub fn state(&self) -> &Arc<dyn StateStore> {
        self.handler.state()
    }

    pub fn cancellation(&self) -> &CancellationToken {
        self.handler.cancel()
    }

    /// Sends a text reply to the chat that produced the current message.
    pub async fn reply_text(&self, text: impl Into<String>) -> Result<SentMessage, SendError> {
        self.handler
            .outbound()
            .send(self.message.address.clone(), OutboundMessage::text(text))
            .await
    }
}
