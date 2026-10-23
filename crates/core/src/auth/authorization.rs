use std::sync::Arc;
use std::time::Duration;

use lukosbot_domain::{ChatKind, InboundMessage};

use super::admin::{AdminIdentity, BotAdminService};
use super::context::AuthContext;
use super::error::AuthorizationError;
use super::resolver::ChatAdminResolver;

const RESOLVER_TIMEOUT: Duration = Duration::from_secs(5);

/// Combines bot-admin state with platform-specific chat-admin resolvers.
pub struct AuthorizationService {
    bot_admins: Arc<BotAdminService>,
    resolvers: Vec<Arc<dyn ChatAdminResolver>>,
}

impl AuthorizationService {
    pub fn new(
        bot_admins: Arc<BotAdminService>,
        resolvers: Vec<Arc<dyn ChatAdminResolver>>,
    ) -> Self {
        Self {
            bot_admins,
            resolvers,
        }
    }

    pub async fn inspect(
        &self,
        message: &InboundMessage,
    ) -> Result<AuthContext, AuthorizationError> {
        let identity = AdminIdentity::new(message.address.platform, message.sender.id.clone());

        if self.bot_admins.is_bot_admin(&identity).await? {
            return Ok(AuthContext {
                bot_admin: true,
                chat_admin: true,
            });
        }

        if !is_chat_context(message.address.kind) {
            return Ok(AuthContext {
                bot_admin: false,
                chat_admin: false,
            });
        }

        let Some(resolver) = self
            .resolvers
            .iter()
            .find(|resolver| resolver.supports(message.address.platform))
        else {
            return Ok(AuthContext {
                bot_admin: false,
                chat_admin: false,
            });
        };

        let chat_admin = tokio::time::timeout(RESOLVER_TIMEOUT, resolver.is_chat_admin(message))
            .await
            .map_err(|_| AuthorizationError::ChatAdminTimeout)?
            .map_err(AuthorizationError::ChatAdminUnavailable)?;

        Ok(AuthContext {
            bot_admin: false,
            chat_admin,
        })
    }

    pub async fn require_bot_admin(
        &self,
        message: &InboundMessage,
    ) -> Result<AuthContext, AuthorizationError> {
        let context = self.inspect(message).await?;
        if context.can_manage_global() {
            Ok(context)
        } else {
            Err(AuthorizationError::Unauthorized)
        }
    }

    pub async fn require_chat_manager(
        &self,
        message: &InboundMessage,
    ) -> Result<AuthContext, AuthorizationError> {
        let context = self.inspect(message).await?;
        if context.can_manage_chat() {
            Ok(context)
        } else {
            Err(AuthorizationError::Unauthorized)
        }
    }
}

fn is_chat_context(kind: ChatKind) -> bool {
    matches!(
        kind,
        ChatKind::Group | ChatKind::Supergroup | ChatKind::Channel
    )
}
