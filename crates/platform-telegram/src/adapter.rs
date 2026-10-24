//! Minimal teloxide-backed Telegram adapter.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use lukosbot_core::auth::{ChatAdminLookupError, ChatAdminResolver};
use lukosbot_core::{InboundSender, PlatformAdapter, PlatformError};
use lukosbot_domain::{
    ChatAddress, ChatKind, InboundMessage, MediaContent, MediaRef, OutboundMessage, OutboundPart,
    PlatformKind, SentMessage,
};
use reqwest012::Proxy;
use teloxide::net::{Download, default_reqwest_settings};
use teloxide::payloads::{GetUpdatesSetters, SendDocumentSetters, SendPhotoSetters};
use teloxide::prelude::Requester;
use teloxide::types::{ChatId, FileId, InputFile, UserId};
use teloxide::{ApiError, Bot, RequestError};
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};

const POLL_TIMEOUT_SECS: u32 = 30;
const CLIENT_TIMEOUT_MARGIN_SECS: u64 = 15;
const INITIAL_BACKOFF: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(30);
const PLATFORM: PlatformKind = PlatformKind::Telegram;

mod media;

/// HTTP settings used by the Telegram adapter.
#[derive(Debug, Clone)]
pub struct TelegramHttpSettings {
    pub connect_timeout: Duration,
    pub request_timeout: Duration,
    pub proxy: Option<String>,
}

impl Default for TelegramHttpSettings {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(5),
            request_timeout: Duration::from_secs(20),
            proxy: None,
        }
    }
}

/// Telegram adapter using explicit long polling.
///
/// The polling loop is deliberately written in terms of `get_updates` rather than teloxide's
/// `Dispatcher`: the runtime owns task lifetime and cancellation, and this keeps the adapter's
/// shutdown semantics explicit. Webhook mode can be added later without changing the port.
pub struct TelegramAdapter {
    bot: Bot,
    client_timeout: Duration,
    proxy_configured: bool,
    admin_cache: Arc<crate::admin::TelegramAdminCache>,
}

impl TelegramAdapter {
    /// Builds an adapter with the default HTTP settings and a long-poll-safe timeout.
    pub fn new(token: impl AsRef<str>) -> Self {
        Self::with_http(token, TelegramHttpSettings::default())
            .expect("default Telegram HTTP client must be constructible")
    }

    /// Builds an adapter with explicit proxy and timeout settings.
    ///
    /// The client timeout is always greater than the long-poll timeout, otherwise teloxide can
    /// abort every successful long poll after its own default 17-second client timeout.
    pub fn with_http(
        token: impl AsRef<str>,
        settings: TelegramHttpSettings,
    ) -> Result<Self, PlatformError> {
        let client_timeout = effective_client_timeout(settings.request_timeout);
        let proxy_configured = settings
            .proxy
            .as_deref()
            .is_some_and(|proxy| !proxy.trim().is_empty());

        let mut builder = default_reqwest_settings()
            .connect_timeout(settings.connect_timeout)
            .timeout(client_timeout);

        if let Some(proxy) = settings.proxy.filter(|proxy| !proxy.trim().is_empty()) {
            let proxy =
                Proxy::all(proxy).map_err(|error| PlatformError::transport(PLATFORM, error))?;
            builder = builder.proxy(proxy);
        }

        let client = builder
            .build()
            .map_err(|error| PlatformError::transport(PLATFORM, error))?;

        Ok(Self {
            bot: Bot::with_client(token.as_ref(), client),
            client_timeout,
            proxy_configured,
            admin_cache: Arc::new(crate::admin::TelegramAdminCache::default()),
        })
    }
}

fn effective_client_timeout(request_timeout: Duration) -> Duration {
    request_timeout.max(Duration::from_secs(
        u64::from(POLL_TIMEOUT_SECS) + CLIENT_TIMEOUT_MARGIN_SECS,
    ))
}

#[async_trait]
impl PlatformAdapter for TelegramAdapter {
    fn kind(&self) -> PlatformKind {
        PLATFORM
    }

    async fn run(
        &self,
        inbound: InboundSender,
        cancel: CancellationToken,
    ) -> Result<(), PlatformError> {
        debug!(
            client_timeout = ?self.client_timeout,
            proxy = self.proxy_configured,
            "Telegram polling started"
        );

        let mut offset: Option<i32> = None;
        let mut backoff = INITIAL_BACKOFF;

        loop {
            let mut request = self.bot.get_updates().timeout(POLL_TIMEOUT_SECS);
            if let Some(offset) = offset {
                request = request.offset(offset);
            }

            let updates = tokio::select! {
                biased;
                _ = cancel.cancelled() => return Ok(()),
                result = request => result,
            };

            match updates {
                Ok(updates) => {
                    backoff = INITIAL_BACKOFF;

                    for update in &updates {
                        offset = Some(next_offset(update));

                        match crate::mapping::map_update(update) {
                            Ok(Some(message)) => {
                                if inbound.send(message).await.is_err() {
                                    return Err(PlatformError::Disconnected { platform: PLATFORM });
                                }
                            }
                            Ok(None) => {}
                            Err(error) => {
                                warn!(%error, "skipping an unmappable Telegram update");
                            }
                        }
                    }
                }
                Err(error) => {
                    let error = map_request_error(error);

                    if matches!(error, PlatformError::Authentication { .. }) {
                        return Err(error);
                    }

                    warn!(%error, "Telegram polling failed; backing off");
                    tokio::select! {
                        biased;
                        _ = cancel.cancelled() => return Ok(()),
                        _ = tokio::time::sleep(backoff) => {}
                    }
                    backoff = (backoff * 2).min(MAX_BACKOFF);
                }
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

        let chat_id = target
            .chat_id
            .as_str()
            .parse::<i64>()
            .map_err(|error| mapping_error(format!("chat id `{}`: {error}", target.chat_id)))?;
        let chat_id = ChatId(chat_id);

        let mut text_parts = Vec::new();
        let mut media: Option<(&MediaRef, Option<&str>, bool)> = None;
        for part in &message.parts {
            match part {
                OutboundPart::Text(text) => text_parts.push(text.as_str()),
                OutboundPart::Image {
                    media: image,
                    caption,
                } => {
                    if media.is_some() {
                        return Err(PlatformError::Api {
                            platform: PLATFORM,
                            message: "an outbound message may contain only one media part"
                                .to_owned(),
                        });
                    }
                    media = Some((image, caption.as_deref(), true));
                }
                OutboundPart::File {
                    media: file,
                    caption,
                } => {
                    if media.is_some() {
                        return Err(PlatformError::Api {
                            platform: PLATFORM,
                            message: "an outbound message may contain only one media part"
                                .to_owned(),
                        });
                    }
                    media = Some((file, caption.as_deref(), false));
                }
            }
        }

        let text = text_parts.join("\n");
        if let Some((media, caption, image)) = media {
            let bytes = materialized_bytes(media, "image")?;
            let caption = join_caption(&text, caption);
            if caption
                .as_ref()
                .is_some_and(|caption| caption.chars().count() > 1024)
            {
                return Err(PlatformError::Api {
                    platform: PLATFORM,
                    message: "Telegram captions must not exceed 1024 characters".to_owned(),
                });
            }

            let sent = if image {
                let mut request = self.bot.send_photo(chat_id, InputFile::memory(bytes));
                if let Some(caption) = caption {
                    request = request.caption(caption);
                }
                request.await
            } else {
                let name = media
                    .file_name
                    .clone()
                    .unwrap_or_else(|| "attachment.bin".to_owned());
                let mut request = self
                    .bot
                    .send_document(chat_id, InputFile::memory(bytes).file_name(name));
                if let Some(caption) = caption {
                    request = request.caption(caption);
                }
                request.await
            }
            .map_err(map_request_error)?;
            return sent_message(sent);
        }

        if text.is_empty() {
            return Err(PlatformError::Api {
                platform: PLATFORM,
                message: "outbound message has no content".to_owned(),
            });
        }

        let sent = self
            .bot
            .send_message(chat_id, text)
            .await
            .map_err(map_request_error)?;
        sent_message(sent)
    }

    async fn load_platform_file(&self, file_id: &str) -> Result<MediaContent, PlatformError> {
        let file = self
            .bot
            .get_file(FileId(file_id.to_owned()))
            .await
            .map_err(map_request_error)?;
        let mut bytes = Vec::new();
        self.bot
            .download_file(&file.path, &mut bytes)
            .await
            .map_err(|error| PlatformError::transport(PLATFORM, error))?;
        let file_name = file
            .path
            .rsplit('/')
            .next()
            .filter(|name| !name.is_empty())
            .map(str::to_owned);

        Ok(MediaContent {
            bytes: bytes.into(),
            file_name,
            mime_type: None,
        })
    }

    async fn shutdown(&self) -> Result<(), PlatformError> {
        // The polling loop owns all resources; dropping `Bot` is sufficient.
        Ok(())
    }
}

#[async_trait]
impl ChatAdminResolver for TelegramAdapter {
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

        let chat_id = message
            .address
            .chat_id
            .as_str()
            .parse::<i64>()
            .map_err(|error| ChatAdminLookupError::InvalidIdentity(format!("chat id: {error}")))?;
        let user_id =
            message.sender.id.as_str().parse::<u64>().map_err(|error| {
                ChatAdminLookupError::InvalidIdentity(format!("user id: {error}"))
            })?;

        let key = crate::admin::AdminCacheKey::new(PLATFORM, chat_id, UserId(user_id));
        if let Some(value) = self.admin_cache.get(&key).await {
            return Ok(value);
        }

        let member = self
            .bot
            .get_chat_member(ChatId(chat_id), UserId(user_id))
            .await
            .map_err(|error| ChatAdminLookupError::Failed(Box::new(error)))?;

        let value = crate::admin::is_admin_member(&member.kind);
        self.admin_cache.put(key, value).await;
        Ok(value)
    }
}

use self::media::{join_caption, materialized_bytes, sent_message};

fn next_offset(update: &teloxide::types::Update) -> i32 {
    i32::try_from(update.id.0.saturating_add(1)).unwrap_or(i32::MAX)
}

fn map_request_error(error: RequestError) -> PlatformError {
    match &error {
        RequestError::Api(ApiError::InvalidToken) => PlatformError::authentication(
            PLATFORM,
            std::io::Error::other("Telegram rejected the bot token"),
        ),
        RequestError::Network(_) | RequestError::Io(_) => PlatformError::transport(PLATFORM, error),
        _ => PlatformError::Api {
            platform: PLATFORM,
            message: error.to_string(),
        },
    }
}

fn mapping_error(message: impl Into<String>) -> PlatformError {
    PlatformError::Mapping {
        platform: PLATFORM,
        message: message.into(),
    }
}

#[cfg(test)]
mod behavior_tests;

#[cfg(test)]
mod http_settings_tests {
    use super::*;

    #[test]
    fn client_timeout_is_longer_than_long_poll() {
        let timeout = effective_client_timeout(Duration::from_secs(20));
        assert!(timeout > Duration::from_secs(u64::from(POLL_TIMEOUT_SECS)));
        assert_eq!(timeout, Duration::from_secs(45));
        assert_eq!(
            effective_client_timeout(Duration::from_secs(60)),
            Duration::from_secs(60)
        );
    }

    #[test]
    fn invalid_proxy_is_rejected_without_network_io() {
        let result = TelegramAdapter::with_http(
            "123:fake-token",
            TelegramHttpSettings {
                proxy: Some("socks5h://[".to_owned()),
                ..TelegramHttpSettings::default()
            },
        );
        assert!(result.is_err());
    }

    #[test]
    fn socks_proxy_is_accepted() {
        let result = TelegramAdapter::with_http(
            "123:fake-token",
            TelegramHttpSettings {
                proxy: Some("socks5h://127.0.0.1:1080".to_owned()),
                ..TelegramHttpSettings::default()
            },
        );
        assert!(result.is_ok());
    }
}
