use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::address::{ChatAddress, InvalidPlatformId, Sender, validate_id};
use crate::media::MediaRef;
use crate::platform::PlatformKind;

/// A platform-specific message identifier.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PlatformMessageId(String);

impl PlatformMessageId {
    pub fn new(value: impl Into<String>) -> Result<Self, InvalidPlatformId> {
        let value = value.into();
        validate_id("message id", &value)?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

impl fmt::Display for PlatformMessageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Trusted platform context attached to an inbound message.
///
/// `nsfw` is `None` when the adapter could not determine the channel state. Policy resolution
/// treats an unknown state as non-NSFW for compatibility with the legacy bot.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MessageContext {
    pub nsfw: Option<bool>,
}

impl MessageContext {
    pub const fn new(nsfw: Option<bool>) -> Self {
        Self { nsfw }
    }
}

/// One level of a replied-to message.
///
/// The contents are intentionally not recursive: a quoted message never carries another quoted
/// message. Adapters map only the immediate reply target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuotedMessage {
    pub message_id: Option<PlatformMessageId>,
    pub sender: Option<Sender>,
    pub parts: Vec<InboundPart>,
}

/// A message received from a platform, normalised into platform-independent parts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboundMessage {
    pub address: ChatAddress,
    pub sender: Sender,
    pub message_id: Option<PlatformMessageId>,
    pub received_at: DateTime<Utc>,
    pub parts: Vec<InboundPart>,
    pub reply_to: Option<PlatformMessageId>,
    pub quoted: Option<QuotedMessage>,
    pub context: MessageContext,
}

impl InboundMessage {
    /// Iterates over text parts in order.
    pub fn text_parts(&self) -> impl Iterator<Item = &str> {
        self.parts.iter().filter_map(|part| match part {
            InboundPart::Text(text) => Some(text.as_str()),
            InboundPart::Image(_) | InboundPart::File(_) => None,
        })
    }

    pub fn has_text(&self) -> bool {
        self.text_parts().any(|text| !text.is_empty())
    }

    /// Returns the first non-blank text part, trimmed.
    ///
    /// Platform adapters map both normal message text and media captions into text parts, so this
    /// is the single text entry point for command processing.
    pub fn primary_text(&self) -> Option<&str> {
        self.text_parts()
            .map(str::trim)
            .find(|text| !text.is_empty())
    }
}

/// One part of an inbound message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InboundPart {
    Text(String),
    Image(MediaRef),
    File(MediaRef),
}

/// A message to be delivered by the outbound service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboundMessage {
    pub parts: Vec<OutboundPart>,
}

impl OutboundMessage {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            parts: vec![OutboundPart::Text(text.into())],
        }
    }

    pub fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }
}

/// One part of an outbound message.
///
/// Adapters may reject parts they cannot represent; they must not silently drop them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutboundPart {
    Text(String),
    Image {
        media: MediaRef,
        caption: Option<String>,
    },
    File {
        media: MediaRef,
        caption: Option<String>,
    },
}

/// A successfully delivered message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SentMessage {
    pub platform: PlatformKind,
    pub message_id: PlatformMessageId,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::address::{PlatformChatId, PlatformUserId};
    use crate::media::MediaSource;
    use crate::platform::ChatKind;
    use url::Url;

    fn text_message(parts: Vec<InboundPart>) -> InboundMessage {
        InboundMessage {
            address: ChatAddress::new(
                PlatformKind::Telegram,
                PlatformChatId::new("1").unwrap(),
                ChatKind::Private,
            ),
            sender: Sender {
                id: PlatformUserId::new("2").unwrap(),
                display_name: None,
                username: None,
            },
            message_id: None,
            received_at: Utc::now(),
            parts,
            reply_to: None,
            quoted: None,
            context: MessageContext::default(),
        }
    }

    #[test]
    fn text_only_message_has_text() {
        let message = text_message(vec![InboundPart::Text("hello".to_owned())]);
        assert!(message.has_text());
        assert_eq!(message.text_parts().collect::<Vec<_>>(), vec!["hello"]);
    }

    #[test]
    fn attachment_only_message_has_no_text() {
        let message = text_message(vec![InboundPart::Image(MediaRef {
            source: MediaSource::Url(Url::parse("https://example.com/a.png").unwrap()),
            file_name: Some("a.png".to_owned()),
            mime_type: Some("image/png".to_owned()),
            size_bytes: Some(12),
        })]);

        assert!(!message.has_text());
    }

    #[test]
    fn mixed_message_exposes_only_text_parts() {
        let message = text_message(vec![
            InboundPart::Text("caption".to_owned()),
            InboundPart::File(MediaRef {
                source: MediaSource::PlatformFile {
                    platform: PlatformKind::Telegram,
                    file_id: "file-1".to_owned(),
                },
                file_name: None,
                mime_type: None,
                size_bytes: None,
            }),
        ]);

        assert_eq!(message.text_parts().collect::<Vec<_>>(), vec!["caption"]);
    }

    #[test]
    fn missing_optional_fields_do_not_panic() {
        let message = text_message(Vec::new());
        assert!(message.message_id.is_none());
        assert!(message.reply_to.is_none());
        assert!(message.sender.display_name.is_none());
        assert!(!message.has_text());
    }

    #[test]
    fn rejects_invalid_message_ids() {
        assert!(PlatformMessageId::new("").is_err());
        assert!(PlatformMessageId::new("a\tb").is_err());
    }

    #[test]
    fn primary_text_skips_blank_parts_and_trims() {
        let message = text_message(vec![
            InboundPart::Text("   ".to_owned()),
            InboundPart::Text("  hello  ".to_owned()),
        ]);

        assert_eq!(message.primary_text(), Some("hello"));
    }

    #[test]
    fn attachment_only_message_has_no_primary_text() {
        let message = text_message(vec![InboundPart::File(MediaRef {
            source: MediaSource::PlatformFile {
                platform: PlatformKind::Discord,
                file_id: "f".to_owned(),
            },
            file_name: None,
            mime_type: None,
            size_bytes: None,
        })]);

        assert_eq!(message.primary_text(), None);
    }
}
