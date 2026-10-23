//! Minimal teloxide-backed Telegram adapter.

use std::time::Duration;

use async_trait::async_trait;
use lukosbot_core::{InboundSender, PlatformAdapter, PlatformError};
use lukosbot_domain::{
    ChatAddress, OutboundMessage, OutboundPart, PlatformKind, PlatformMessageId, SentMessage,
};
use reqwest012::Proxy;
use teloxide::net::default_reqwest_settings;
use teloxide::payloads::GetUpdatesSetters;
use teloxide::prelude::Requester;
use teloxide::types::ChatId;
use teloxide::{ApiError, Bot, RequestError};
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};

const POLL_TIMEOUT_SECS: u32 = 30;
const CLIENT_TIMEOUT_MARGIN_SECS: u64 = 15;
const INITIAL_BACKOFF: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(30);
const PLATFORM: PlatformKind = PlatformKind::Telegram;

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

        let text = text_content(&message)?;
        if text.is_empty() {
            return Err(PlatformError::Api {
                platform: PLATFORM,
                message: "outbound message has no text".to_owned(),
            });
        }

        let chat_id = target
            .chat_id
            .as_str()
            .parse::<i64>()
            .map_err(|error| mapping_error(format!("chat id `{}`: {error}", target.chat_id)))?;

        let sent = self
            .bot
            .send_message(ChatId(chat_id), text)
            .await
            .map_err(map_request_error)?;

        let message_id = PlatformMessageId::new(sent.id.0.to_string())
            .map_err(|error| mapping_error(format!("sent message id: {error}")))?;

        Ok(SentMessage {
            platform: PLATFORM,
            message_id,
        })
    }

    async fn shutdown(&self) -> Result<(), PlatformError> {
        // The polling loop owns all resources; dropping `Bot` is sufficient.
        Ok(())
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
mod tests {
    use super::*;
    use lukosbot_domain::{ChatKind, MediaRef, MediaSource, PlatformChatId};
    use url::Url;

    fn address(platform: PlatformKind) -> ChatAddress {
        ChatAddress::new(
            platform,
            PlatformChatId::new("123").expect("valid chat id"),
            ChatKind::Private,
        )
    }

    #[tokio::test]
    async fn rejects_targets_for_other_platforms() {
        let adapter = TelegramAdapter::new("123:fake-token");
        let error = adapter
            .send(
                &address(PlatformKind::Discord),
                OutboundMessage::text("hello"),
            )
            .await
            .expect_err("wrong platform is rejected");

        assert!(matches!(
            error,
            PlatformError::WrongPlatform {
                target: PlatformKind::Discord,
                adapter: PlatformKind::Telegram
            }
        ));
    }

    #[tokio::test]
    async fn rejects_media_without_attempting_network_io() {
        let adapter = TelegramAdapter::new("123:fake-token");
        let message = OutboundMessage {
            parts: vec![OutboundPart::Image {
                media: MediaRef {
                    source: MediaSource::Url(Url::parse("https://example.com/a.png").unwrap()),
                    file_name: None,
                    mime_type: None,
                    size_bytes: None,
                },
                caption: None,
            }],
        };

        let error = adapter
            .send(&address(PlatformKind::Telegram), message)
            .await
            .expect_err("media is unsupported");

        assert!(matches!(
            error,
            PlatformError::Unsupported {
                platform: PlatformKind::Telegram,
                part: "image"
            }
        ));
    }

    #[tokio::test]
    async fn rejects_non_numeric_chat_ids_before_network_io() {
        let adapter = TelegramAdapter::new("123:fake-token");
        let address = ChatAddress::new(
            PlatformKind::Telegram,
            PlatformChatId::new("not-a-number").unwrap(),
            ChatKind::Private,
        );

        let error = adapter
            .send(&address, OutboundMessage::text("hello"))
            .await
            .expect_err("invalid chat id");

        assert!(matches!(error, PlatformError::Mapping { .. }));
    }

    #[test]
    fn next_offset_advances_past_the_update() {
        use teloxide::types::{Message, Update, UpdateId, UpdateKind};

        let message: Message = serde_json::from_value(serde_json::json!({
            "message_id": 1,
            "date": 1_700_000_000u64,
            "chat": { "id": 1, "type": "private", "first_name": "A" },
            "from": { "id": 2, "is_bot": false, "first_name": "B" },
            "text": "x",
        }))
        .expect("fixture is a valid message");

        let update = Update {
            id: UpdateId(41),
            kind: UpdateKind::Message(message),
        };

        assert_eq!(next_offset(&update), 42);
    }
}

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
