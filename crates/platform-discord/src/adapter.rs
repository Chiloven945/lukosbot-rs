//! Minimal serenity-backed Discord adapter.

use std::sync::{Arc, OnceLock};

use async_trait::async_trait;
use lukosbot_core::auth::{ChatAdminLookupError, ChatAdminResolver};
use lukosbot_core::{InboundSender, PlatformAdapter, PlatformError};
use lukosbot_domain::{
    ChatAddress, ChatKind, InboundMessage, OutboundMessage, OutboundPart, PlatformKind,
    PlatformMessageId, SentMessage,
};
use secrecy::{ExposeSecret, SecretString};
use serenity::all::{
    ChannelId, Client, Context, CreateMessage, EventHandler, GatewayIntents, GuildId, Http,
    Message, ShardManager,
};
use serenity::async_trait as serenity_async_trait;
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};

const PLATFORM: PlatformKind = PlatformKind::Discord;

/// Discord adapter with a minimal message-only gateway handler.
///
/// `MESSAGE_CONTENT` is a privileged intent and must be enabled for the application in the
/// Discord developer portal. Missing privileges surface as a gateway error rather than silently
/// dropping message content.
pub struct DiscordAdapter {
    token: SecretString,
    http: Arc<Http>,
    intents: GatewayIntents,
    shard_manager: OnceLock<Arc<ShardManager>>,
    permission_cache: Arc<crate::admin::DiscordPermissionCache>,
}

impl DiscordAdapter {
    pub fn new(token: impl Into<String>) -> Self {
        let token = SecretString::from(token.into());
        let http = Arc::new(Http::new(token.expose_secret()));

        Self {
            token,
            http,
            intents: GatewayIntents::GUILD_MESSAGES
                | GatewayIntents::DIRECT_MESSAGES
                | GatewayIntents::MESSAGE_CONTENT,
            shard_manager: OnceLock::new(),
            permission_cache: Arc::new(crate::admin::DiscordPermissionCache::default()),
        }
    }
}

#[async_trait]
impl PlatformAdapter for DiscordAdapter {
    fn kind(&self) -> PlatformKind {
        PLATFORM
    }

    async fn run(
        &self,
        inbound: InboundSender,
        cancel: CancellationToken,
    ) -> Result<(), PlatformError> {
        debug!("Discord gateway starting");

        let handler = GatewayHandler {
            inbound,
            permissions: Arc::clone(&self.permission_cache),
        };
        let mut client = Client::builder(self.token.expose_secret(), self.intents)
            .event_handler(handler)
            .await
            .map_err(|error| PlatformError::transport(PLATFORM, error))?;

        let shard_manager = Arc::clone(&client.shard_manager);
        let _ = self.shard_manager.set(Arc::clone(&shard_manager));

        tokio::select! {
            biased;
            _ = cancel.cancelled() => {
                shard_manager.shutdown_all().await;
                Ok(())
            }
            result = client.start() => {
                result.map_err(|error| PlatformError::transport(PLATFORM, error))
            }
        }
    }

    async fn send(
        &self,
        target: &ChatAddress,
        message: OutboundMessage,
    ) -> Result<SentMessage, PlatformError> {
        if target.platform != PLATFORM {
            return Err(PlatformError::WrongPlatform {
                target: target.platform,
                adapter: PLATFORM,
            });
        }

        let text = text_content(&message)?;
        if text.is_empty() {
            return Err(PlatformError::Api {
                platform: PLATFORM,
                message: "outbound message has no text".to_owned(),
            });
        }

        let channel_id =
            target.chat_id.as_str().parse::<u64>().map_err(|error| {
                mapping_error(format!("channel id `{}`: {error}", target.chat_id))
            })?;

        if channel_id == 0 {
            return Err(mapping_error("channel id must not be zero"));
        }

        let sent = ChannelId::new(channel_id)
            .send_message(&*self.http, CreateMessage::new().content(text))
            .await
            .map_err(|error| PlatformError::Api {
                platform: PLATFORM,
                message: error.to_string(),
            })?;

        let message_id = PlatformMessageId::new(sent.id.get().to_string())
            .map_err(|error| mapping_error(format!("sent message id: {error}")))?;

        Ok(SentMessage {
            platform: PLATFORM,
            message_id,
        })
    }

    async fn shutdown(&self) -> Result<(), PlatformError> {
        if let Some(shard_manager) = self.shard_manager.get() {
            shard_manager.shutdown_all().await;
        }
        Ok(())
    }
}

struct GatewayHandler {
    inbound: InboundSender,
    permissions: Arc<crate::admin::DiscordPermissionCache>,
}

#[serenity_async_trait]
impl EventHandler for GatewayHandler {
    async fn message(&self, context: Context, message: Message) {
        let nsfw = if let Some(guild_id) = message.guild_id {
            context.cache.guild(guild_id).and_then(|guild| {
                guild
                    .channels
                    .get(&message.channel_id)
                    .map(|channel| channel.nsfw)
            })
        } else {
            Some(false)
        };

        if let Some(guild_id) = message.guild_id
            && let Some(chat_admin) = chat_admin_snapshot(&context, &message, guild_id)
        {
            self.permissions
                .put(
                    crate::admin::PermissionCacheKey::new(
                        PLATFORM,
                        message.channel_id.get(),
                        message.author.id.get(),
                    ),
                    chat_admin,
                )
                .await;
        }

        match crate::mapping::map_message(&message, chrono::Utc::now(), nsfw) {
            Ok(Some(message)) => {
                if self.inbound.send(message).await.is_err() {
                    warn!("inbound queue is closed; Discord message dropped");
                }
            }
            Ok(None) => {}
            Err(error) => {
                warn!(%error, "skipping an unmappable Discord message");
            }
        }
    }
}

fn chat_admin_snapshot(context: &Context, message: &Message, guild_id: GuildId) -> Option<bool> {
    let guild = context.cache.guild(guild_id)?;
    let channel = guild.channels.get(&message.channel_id)?;
    let member = guild.members.get(&message.author.id)?;
    Some(crate::admin::is_chat_admin_permissions(
        guild.user_permissions_in(channel, member),
    ))
}

#[async_trait]
impl ChatAdminResolver for DiscordAdapter {
    fn supports(&self, platform: PlatformKind) -> bool {
        platform == PLATFORM
    }

    async fn is_chat_admin(&self, message: &InboundMessage) -> Result<bool, ChatAdminLookupError> {
        if !matches!(
            message.address.kind,
            ChatKind::Group | ChatKind::Supergroup | ChatKind::Channel
        ) {
            return Ok(false);
        }

        let channel_id = message
            .address
            .chat_id
            .as_str()
            .parse::<u64>()
            .map_err(|error| {
                ChatAdminLookupError::InvalidIdentity(format!("channel id: {error}"))
            })?;
        let user_id =
            message.sender.id.as_str().parse::<u64>().map_err(|error| {
                ChatAdminLookupError::InvalidIdentity(format!("user id: {error}"))
            })?;

        let key = crate::admin::PermissionCacheKey::new(PLATFORM, channel_id, user_id);
        self.permission_cache
            .get(&key)
            .await
            .ok_or(ChatAdminLookupError::Unavailable)
    }
}

fn text_content(message: &OutboundMessage) -> Result<String, PlatformError> {
    let mut chunks = Vec::with_capacity(message.parts.len());

    for part in &message.parts {
        match part {
            OutboundPart::Text(text) => chunks.push(text.as_str()),
            OutboundPart::Image { .. } => {
                return Err(PlatformError::Unsupported {
                    platform: PLATFORM,
                    part: "image",
                });
            }
            OutboundPart::File { .. } => {
                return Err(PlatformError::Unsupported {
                    platform: PLATFORM,
                    part: "file",
                });
            }
        }
    }

    Ok(chunks.join("\n"))
}

fn mapping_error(message: impl Into<String>) -> PlatformError {
    PlatformError::Mapping {
        platform: PLATFORM,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lukosbot_domain::{ChatKind, MediaRef, MediaSource, PlatformChatId};
    use url::Url;

    fn address(platform: PlatformKind, chat_id: &str) -> ChatAddress {
        ChatAddress::new(
            platform,
            PlatformChatId::new(chat_id).expect("valid chat id"),
            ChatKind::Private,
        )
    }

    #[tokio::test]
    async fn rejects_targets_for_other_platforms() {
        let adapter = DiscordAdapter::new("fake-token");
        let error = adapter
            .send(
                &address(PlatformKind::Telegram, "123"),
                OutboundMessage::text("hello"),
            )
            .await
            .expect_err("wrong platform is rejected");

        assert!(matches!(
            error,
            PlatformError::WrongPlatform {
                target: PlatformKind::Telegram,
                adapter: PlatformKind::Discord
            }
        ));
    }

    #[tokio::test]
    async fn rejects_media_without_attempting_network_io() {
        let adapter = DiscordAdapter::new("fake-token");
        let message = OutboundMessage {
            parts: vec![OutboundPart::File {
                media: MediaRef {
                    source: MediaSource::Url(Url::parse("https://example.com/a.pdf").unwrap()),
                    file_name: None,
                    mime_type: None,
                    size_bytes: None,
                },
                caption: None,
            }],
        };

        let error = adapter
            .send(&address(PlatformKind::Discord, "123"), message)
            .await
            .expect_err("media is unsupported");

        assert!(matches!(
            error,
            PlatformError::Unsupported {
                platform: PlatformKind::Discord,
                part: "file"
            }
        ));
    }

    #[tokio::test]
    async fn rejects_invalid_channel_ids_before_network_io() {
        let adapter = DiscordAdapter::new("fake-token");
        let error = adapter
            .send(
                &address(PlatformKind::Discord, "not-a-number"),
                OutboundMessage::text("hello"),
            )
            .await
            .expect_err("invalid channel id");

        assert!(matches!(error, PlatformError::Mapping { .. }));
    }

    #[tokio::test]
    async fn rejects_zero_channel_ids() {
        let adapter = DiscordAdapter::new("fake-token");
        let error = adapter
            .send(
                &address(PlatformKind::Discord, "0"),
                OutboundMessage::text("hello"),
            )
            .await
            .expect_err("zero channel id is rejected");

        assert!(matches!(error, PlatformError::Mapping { .. }));
    }
}
