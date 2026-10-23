//! Discord to domain mapping.
//!
//! Pure mapping functions with JSON fixtures so tests never contact Discord.

use chrono::{DateTime, Utc};
use lukosbot_core::PlatformError;
use lukosbot_domain::{
    ChatAddress, ChatKind, InboundMessage, InboundPart, MediaRef, MediaSource, MessageContext,
    PlatformChatId, PlatformKind, PlatformMessageId, PlatformUserId, Sender,
};
use serenity::model::channel::Message;
use url::Url;

const PLATFORM: PlatformKind = PlatformKind::Discord;

pub(crate) fn map_message(
    message: &Message,
    received_at: DateTime<Utc>,
    nsfw: Option<bool>,
) -> Result<Option<InboundMessage>, PlatformError> {
    if message.author.bot {
        return Ok(None);
    }

    let address = ChatAddress::new(
        PLATFORM,
        PlatformChatId::new(message.channel_id.get().to_string())
            .map_err(|error| mapping(format!("channel id: {error}")))?,
        if message.guild_id.is_some() {
            ChatKind::Group
        } else {
            ChatKind::Private
        },
    );

    let sender = Sender {
        id: PlatformUserId::new(message.author.id.get().to_string())
            .map_err(|error| mapping(format!("user id: {error}")))?,
        display_name: message
            .author
            .global_name
            .clone()
            .or_else(|| Some(message.author.name.clone())),
        username: Some(message.author.name.clone()),
    };

    let mut parts = Vec::new();
    if !message.content.is_empty() {
        parts.push(InboundPart::Text(message.content.clone()));
    }

    for attachment in &message.attachments {
        let url = Url::parse(&attachment.url)
            .map_err(|error| mapping(format!("attachment url `{}`: {error}", attachment.url)))?;

        let media = MediaRef {
            source: MediaSource::Url(url),
            file_name: Some(attachment.filename.clone()),
            mime_type: attachment.content_type.clone(),
            size_bytes: Some(u64::from(attachment.size)),
        };

        let is_image = attachment
            .content_type
            .as_deref()
            .is_some_and(|mime_type| mime_type.starts_with("image/"));

        parts.push(if is_image {
            InboundPart::Image(media)
        } else {
            InboundPart::File(media)
        });
    }

    let message_id = PlatformMessageId::new(message.id.get().to_string())
        .map_err(|error| mapping(format!("message id: {error}")))?;

    let reply_to = message
        .referenced_message
        .as_ref()
        .map(|reply| PlatformMessageId::new(reply.id.get().to_string()))
        .transpose()
        .map_err(|error| mapping(format!("reply message id: {error}")))?;

    Ok(Some(InboundMessage {
        address,
        sender,
        message_id: Some(message_id),
        received_at,
        parts,
        reply_to,
        context: MessageContext::new(nsfw),
    }))
}

fn mapping(message: impl Into<String>) -> PlatformError {
    PlatformError::Mapping {
        platform: PLATFORM,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn parse(value: Value) -> Message {
        serde_json::from_value(value).expect("fixture is a valid Discord message")
    }

    fn message_value(content: &str) -> Value {
        json!({
            "id": "10",
            "channel_id": "20",
            "guild_id": "40",
            "author": {
                "id": "30",
                "username": "alice",
                "global_name": "Alice",
                "bot": false,
            },
            "content": content,
            "timestamp": "2024-01-01T00:00:00Z",
            "edited_timestamp": null,
            "tts": false,
            "mention_everyone": false,
            "mentions": [],
            "mention_roles": [],
            "attachments": [],
            "embeds": [],
            "pinned": false,
            "type": 0,
        })
    }

    fn attachment(filename: &str, content_type: &str, url: &str, size: u64) -> Value {
        json!({
            "id": "1",
            "filename": filename,
            "description": null,
            "height": null,
            "proxy_url": url,
            "size": size,
            "url": url,
            "width": null,
            "content_type": content_type,
        })
    }

    fn referenced_message() -> Value {
        let mut message = message_value("original");
        message["id"] = json!("9");
        message["author"] = json!({
            "id": "31",
            "username": "bob",
            "bot": false,
        });
        message
    }

    #[test]
    fn maps_guild_message() {
        let message = map_message(&parse(message_value("hello")), Utc::now(), Some(false))
            .unwrap()
            .unwrap();

        assert_eq!(message.address.platform, PlatformKind::Discord);
        assert_eq!(message.address.chat_id.as_str(), "20");
        assert_eq!(message.address.kind, ChatKind::Group);
        assert_eq!(message.sender.id.as_str(), "30");
        assert_eq!(message.sender.display_name.as_deref(), Some("Alice"));
        assert_eq!(message.sender.username.as_deref(), Some("alice"));
        assert_eq!(
            message.message_id.as_ref().map(PlatformMessageId::as_str),
            Some("10")
        );
        assert_eq!(message.text_parts().collect::<Vec<_>>(), vec!["hello"]);
        assert_eq!(message.context.nsfw, Some(false));
    }

    #[test]
    fn maps_unknown_nsfw_context() {
        let message = map_message(&parse(message_value("hello")), Utc::now(), None)
            .unwrap()
            .unwrap();

        assert_eq!(message.context.nsfw, None);
    }

    #[test]
    fn maps_direct_message() {
        let mut value = message_value("hello");
        value.as_object_mut().unwrap().remove("guild_id");

        let message = map_message(&parse(value), Utc::now(), None)
            .unwrap()
            .unwrap();
        assert_eq!(message.address.kind, ChatKind::Private);
    }

    #[test]
    fn ignores_bot_authored_messages() {
        let mut value = message_value("hello");
        value["author"]["bot"] = json!(true);

        assert!(
            map_message(&parse(value), Utc::now(), None)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn maps_image_attachment_and_text() {
        let mut value = message_value("look");
        value["attachments"] = json!([attachment(
            "a.png",
            "image/png",
            "https://cdn.example/a.png",
            128,
        )]);

        let message = map_message(&parse(value), Utc::now(), None)
            .unwrap()
            .unwrap();

        assert_eq!(message.parts.len(), 2);
        assert_eq!(message.text_parts().collect::<Vec<_>>(), vec!["look"]);
        match &message.parts[1] {
            InboundPart::Image(media) => {
                assert_eq!(media.file_name.as_deref(), Some("a.png"));
                assert_eq!(media.mime_type.as_deref(), Some("image/png"));
                assert_eq!(media.size_bytes, Some(128));
                assert!(matches!(media.source, MediaSource::Url(_)));
            }
            other => panic!("unexpected part: {other:?}"),
        }
    }

    #[test]
    fn maps_non_image_attachment_as_file() {
        let mut value = message_value("");
        value["attachments"] = json!([attachment(
            "a.pdf",
            "application/pdf",
            "https://cdn.example/a.pdf",
            256,
        )]);

        let message = map_message(&parse(value), Utc::now(), None)
            .unwrap()
            .unwrap();
        assert!(matches!(message.parts[0], InboundPart::File(_)));
    }

    #[test]
    fn maps_reply_reference() {
        let mut value = message_value("reply");
        value["referenced_message"] = referenced_message();

        let message = map_message(&parse(value), Utc::now(), None)
            .unwrap()
            .unwrap();
        assert_eq!(
            message.reply_to.as_ref().map(PlatformMessageId::as_str),
            Some("9")
        );
    }
}
