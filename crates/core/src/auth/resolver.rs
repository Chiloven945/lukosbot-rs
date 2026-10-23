use async_trait::async_trait;

use lukosbot_domain::{InboundMessage, PlatformKind};

use super::error::ChatAdminLookupError;

/// Resolves whether the actor in a message is an administrator of the current chat/guild.
#[async_trait]
pub trait ChatAdminResolver: Send + Sync {
    fn supports(&self, platform: PlatformKind) -> bool;

    async fn is_chat_admin(&self, message: &InboundMessage) -> Result<bool, ChatAdminLookupError>;
}
