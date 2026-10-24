use std::collections::BTreeSet;
use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use lukosbot_core::handler::HandlerContext;
use lukosbot_core::messaging::outbound::SendError;
use lukosbot_core::policy::CommandPolicy;
use lukosbot_core::ports::{MediaLoadError, MediaLoadPolicy, MediaLoader, StateStore};
use lukosbot_domain::{
    ChatAddress, InboundMessage, MediaContent, MediaRef, OutboundMessage, Sender, SentMessage,
};

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
    policy: &'a dyn CommandPolicy,
}

impl<'a> CommandContext<'a> {
    pub fn new(
        message: &'a InboundMessage,
        handler: &'a HandlerContext,
        registry: &'a CommandRegistry,
        prefix: &'a str,
        policy: &'a dyn CommandPolicy,
    ) -> Self {
        Self {
            message,
            handler,
            registry,
            prefix,
            policy,
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

    pub fn media(&self) -> &Arc<dyn MediaLoader> {
        self.handler.media()
    }

    pub async fn load_media(
        &self,
        reference: &MediaRef,
        policy: MediaLoadPolicy,
    ) -> Result<MediaContent, MediaLoadError> {
        self.handler.media().load(reference, policy).await
    }

    pub fn cancellation(&self) -> &CancellationToken {
        self.handler.cancel()
    }

    /// Returns whether the current message context permits a command.
    pub fn is_command_allowed(&self, command_name: &str) -> bool {
        self.policy.is_command_allowed(self.message, command_name)
    }

    /// Returns the effective allowed values for a policy key in this message context.
    pub fn allowed_values(&self, key: &str, defaults: Option<&[String]>) -> BTreeSet<String> {
        self.policy.allowed_values(self.message, key, defaults)
    }

    /// Sends a text reply to the chat that produced the current message.
    pub async fn reply_text(&self, text: impl Into<String>) -> Result<SentMessage, SendError> {
        self.reply_message(OutboundMessage::text(text)).await
    }

    /// Sends an arbitrary outbound message to the chat that produced the current message.
    pub async fn reply_message(&self, message: OutboundMessage) -> Result<SentMessage, SendError> {
        self.handler
            .outbound()
            .send(self.message.address.clone(), message)
            .await
    }
}
