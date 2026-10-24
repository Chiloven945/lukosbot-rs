//! Telegram to domain mapping.
//!
//! The functions in this module are pure and are tested against JSON fixtures, so the mapping can
//! be verified without a bot token or network access.

use lukosbot_core::PlatformError;
use lukosbot_domain::{
    ChatAddress, ChatKind, InboundMessage, InboundPart, MediaRef, MediaSource, MessageContext,
    PlatformChatId, PlatformKind, PlatformMessageId, PlatformUserId, QuotedMessage, Sender,
};
use teloxide::types::{
    Chat, ChatKind as TelegramChatKind, Message, PublicChatKind, Update, UpdateKind,
};

const PLATFORM: PlatformKind = PlatformKind::Telegram;

/// Maps a Telegram update into a normalised inbound message.
///
/// Returns `Ok(None)` for updates the base adapter does not consume (edits, callbacks, and
/// messages authored by bots).
pub(crate) fn map_update(update: &Update) -> Result<Option<InboundMessage>, PlatformError> {
    match &update.kind {
        UpdateKind::Message(message) | UpdateKind::ChannelPost(message) => map_message(message),
        _ => Ok(None),
    }
}

fn map_message(message: &Message) -> Result<Option<InboundMessage>, PlatformError> {
    let Some(sender) = map_sender(message)? else {
        return Ok(None);
    };

    let address = ChatAddress::new(
        PLATFORM,
        PlatformChatId::new(message.chat.id.0.to_string())
            .map_err(|error| mapping(format!("chat id: {error}")))?,
        map_chat_kind(&message.chat),
    );

    let parts = map_parts(message)?;

    let message_id = PlatformMessageId::new(message.id.0.to_string())
        .map_err(|error| mapping(format!("message id: {error}")))?;

    let reply_to = message
        .reply_to_message()
        .map(|reply| PlatformMessageId::new(reply.id.0.to_string()))
        .transpose()
        .map_err(|error| mapping(format!("reply message id: {error}")))?;
    let quoted = message
        .reply_to_message()
        .map(map_quoted_message)
        .transpose()?;

    Ok(Some(InboundMessage {
        address,
        sender,
        message_id: Some(message_id),
        received_at: message.date,
        parts,
        reply_to,
        quoted,
        context: MessageContext::new(Some(false)),
    }))
}

fn map_parts(message: &Message) -> Result<Vec<InboundPart>, PlatformError> {
    let mut parts = Vec::new();
    if let Some(text) = message.text().or_else(|| message.caption())
        && !text.is_empty()
    {
        parts.push(InboundPart::Text(text.to_owned()));
    }

    if let Some(photo) = message.photo()
        && let Some(largest) = photo
            .iter()
            .max_by_key(|photo| u64::from(photo.width) * u64::from(photo.height))
    {
        parts.push(InboundPart::Image(MediaRef {
            source: MediaSource::PlatformFile {
                platform: PLATFORM,
                file_id: largest.file.id.0.clone(),
            },
            file_name: None,
            mime_type: Some("image/jpeg".to_owned()),
            size_bytes: file_size(largest.file.size),
        }));
    }

    if let Some(document) = message.document() {
        parts.push(InboundPart::File(MediaRef {
            source: MediaSource::PlatformFile {
                platform: PLATFORM,
                file_id: document.file.id.0.clone(),
            },
            file_name: document.file_name.clone(),
            mime_type: document
                .mime_type
                .as_ref()
                .map(|mime_type| mime_type.to_string()),
            size_bytes: file_size(document.file.size),
        }));
    }

    Ok(parts)
}

fn map_quoted_message(message: &Message) -> Result<QuotedMessage, PlatformError> {
    let message_id = PlatformMessageId::new(message.id.0.to_string())
        .map_err(|error| mapping(format!("quoted message id: {error}")))?;

    Ok(QuotedMessage {
        message_id: Some(message_id),
        sender: map_sender(message)?,
        parts: map_parts(message)?,
    })
}

fn map_sender(message: &Message) -> Result<Option<Sender>, PlatformError> {
    let (id, display_name, username) = if let Some(user) = message.from.as_ref() {
        if user.is_bot {
            return Ok(None);
        }
        (
            user.id.0.to_string(),
            Some(user.full_name()),
            user.username.clone(),
        )
    } else if let Some(chat) = message.sender_chat.as_ref() {
        (
            chat.id.0.to_string(),
            chat_display_name(chat),
            chat_username(chat),
        )
    } else {
        (
            message.chat.id.0.to_string(),
            chat_display_name(&message.chat),
            chat_username(&message.chat),
        )
    };

    let id = PlatformUserId::new(id).map_err(|error| mapping(format!("user id: {error}")))?;

    Ok(Some(Sender {
        id,
        display_name,
        username,
    }))
}

fn map_chat_kind(chat: &Chat) -> ChatKind {
    match &chat.kind {
        TelegramChatKind::Private(_) => ChatKind::Private,
        TelegramChatKind::Public(public) => match &public.kind {
            PublicChatKind::Group => ChatKind::Group,
            PublicChatKind::Supergroup(_) => ChatKind::Supergroup,
            PublicChatKind::Channel(_) => ChatKind::Channel,
        },
    }
}

fn chat_display_name(chat: &Chat) -> Option<String> {
    match &chat.kind {
        TelegramChatKind::Private(private) => {
            private
                .first_name
                .as_ref()
                .map(|first_name| match &private.last_name {
                    Some(last_name) => format!("{first_name} {last_name}"),
                    None => first_name.clone(),
                })
        }
        TelegramChatKind::Public(public) => public.title.clone(),
    }
}

fn chat_username(chat: &Chat) -> Option<String> {
    match &chat.kind {
        TelegramChatKind::Private(private) => private.username.clone(),
        TelegramChatKind::Public(public) => match &public.kind {
            PublicChatKind::Channel(channel) => channel.username.clone(),
            PublicChatKind::Supergroup(supergroup) => supergroup.username.clone(),
            PublicChatKind::Group => None,
        },
    }
}

fn file_size(size: u32) -> Option<u64> {
    // teloxide uses `u32::MAX` when Telegram omits the size.
    (size != u32::MAX).then_some(u64::from(size))
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
    use teloxide::types::UpdateId;

    fn parse_message(value: Value) -> Message {
        serde_json::from_value(value).expect("fixture is a valid Telegram message")
    }

    /// Builds an `Update` around a typed message.
    ///
    /// teloxide's `UpdateKind` deserializer only inspects the first key of a flattened map, which
    /// makes it order-sensitive through `serde_json::from_value`. Tests construct the `Update`
    /// directly; the message itself is still parsed from a `json!` fixture.
    fn update(kind: &str, update_id: u32, message: Value) -> Update {
        let message = parse_message(message);
        let kind = match kind {
            "message" => UpdateKind::Message(message),
            "channel_post" => UpdateKind::ChannelPost(message),
            other => panic!("unsupported update kind in fixture: {other}"),
        };

        Update {
            id: UpdateId(update_id),
            kind,
        }
    }

    fn user(id: u64, first_name: &str) -> Value {
        json!({
            "id": id,
            "is_bot": false,
            "first_name": first_name,
        })
    }

    fn private_chat() -> Value {
        json!({
            "id": 123,
            "type": "private",
            "first_name": "Chat",
            "username": "chat_user",
        })
    }

    fn text_message(chat: Value, from: Option<Value>, text: &str) -> Value {
        let mut message = json!({
            "message_id": 5,
            "date": 1_700_000_000u64,
            "chat": chat,
            "text": text,
        });

        if let Some(from) = from {
            message["from"] = from;
        }

        message
    }

    fn private_text_update(text: &str) -> Update {
        update(
            "message",
            10,
            text_message(
                private_chat(),
                Some(json!({
                    "id": 7,
                    "is_bot": false,
                    "first_name": "Alice",
                    "last_name": "A",
                    "username": "alice",
                })),
                text,
            ),
        )
    }

    #[test]
    fn maps_private_text_message() {
        let message = map_update(&private_text_update("ping"))
            .expect("mapping succeeds")
            .expect("message is consumed");

        assert_eq!(message.address.platform, PlatformKind::Telegram);
        assert_eq!(message.address.chat_id.as_str(), "123");
        assert_eq!(message.address.kind, ChatKind::Private);
        assert_eq!(message.sender.id.as_str(), "7");
        assert_eq!(message.sender.display_name.as_deref(), Some("Alice A"));
        assert_eq!(message.sender.username.as_deref(), Some("alice"));
        assert_eq!(
            message.message_id.as_ref().map(PlatformMessageId::as_str),
            Some("5")
        );
        assert_eq!(message.text_parts().collect::<Vec<_>>(), vec!["ping"]);
        assert_eq!(message.context.nsfw, Some(false));
    }

    #[test]
    fn maps_chat_kinds() {
        let group = update(
            "message",
            1,
            text_message(
                json!({ "id": -10, "type": "group", "title": "Group" }),
                Some(user(7, "A")),
                "hi",
            ),
        );
        let supergroup = update(
            "message",
            2,
            text_message(
                json!({
                    "id": -11,
                    "type": "supergroup",
                    "title": "Super",
                    "username": "super",
                }),
                Some(user(7, "A")),
                "hi",
            ),
        );
        // Channel posts have no `from`; the channel itself is the author.
        let channel = update(
            "channel_post",
            3,
            text_message(
                json!({
                    "id": -12,
                    "type": "channel",
                    "title": "Channel"
                }),
                None,
                "hi",
            ),
        );

        assert_eq!(
            map_update(&group).unwrap().unwrap().address.kind,
            ChatKind::Group
        );
        assert_eq!(
            map_update(&supergroup).unwrap().unwrap().address.kind,
            ChatKind::Supergroup
        );

        let channel = map_update(&channel).unwrap().unwrap();
        assert_eq!(channel.address.kind, ChatKind::Channel);
        assert_eq!(channel.sender.id.as_str(), "-12");
        assert_eq!(channel.sender.display_name.as_deref(), Some("Channel"));
    }

    #[test]
    fn maps_photo_with_caption_and_largest_size() {
        let mut message = text_message(private_chat(), Some(user(7, "A")), "ignored");
        message.as_object_mut().unwrap().remove("text");
        message["caption"] = json!("look");
        message["photo"] = json!([
            {
                "file_id": "small",
                "file_unique_id": "u1",
                "width": 10,
                "height": 10,
                "file_size": 100,
            },
            {
                "file_id": "large",
                "file_unique_id": "u2",
                "width": 100,
                "height": 100,
                "file_size": 1_000,
            },
        ]);

        let message = map_update(&update("message", 20, message))
            .unwrap()
            .unwrap();

        assert_eq!(message.text_parts().collect::<Vec<_>>(), vec!["look"]);
        assert_eq!(message.parts.len(), 2);
        match &message.parts[1] {
            InboundPart::Image(media) => match &media.source {
                MediaSource::PlatformFile { file_id, .. } => assert_eq!(file_id, "large"),
                other => panic!("unexpected media source: {other:?}"),
            },
            other => panic!("unexpected part: {other:?}"),
        }
    }

    #[test]
    fn maps_document_with_metadata() {
        let mut message = text_message(private_chat(), Some(user(7, "A")), "ignored");
        message.as_object_mut().unwrap().remove("text");
        message["document"] = json!({
            "file_id": "doc-1",
            "file_unique_id": "du1",
            "file_name": "report.pdf",
            "mime_type": "application/pdf",
            "file_size": 42,
        });

        let message = map_update(&update("message", 21, message))
            .unwrap()
            .unwrap();

        match &message.parts[0] {
            InboundPart::File(media) => {
                assert_eq!(media.file_name.as_deref(), Some("report.pdf"));
                assert_eq!(media.mime_type.as_deref(), Some("application/pdf"));
                assert_eq!(media.size_bytes, Some(42));
            }
            other => panic!("unexpected part: {other:?}"),
        }
    }

    #[test]
    fn ignores_bot_authored_messages() {
        let update = update(
            "message",
            22,
            text_message(
                private_chat(),
                Some(json!({
                    "id": 9,
                    "is_bot": true,
                    "first_name": "Bot",
                })),
                "ping",
            ),
        );

        assert!(map_update(&update).unwrap().is_none());
    }

    #[test]
    fn maps_reply_reference() {
        let mut message = text_message(private_chat(), Some(user(7, "A")), "reply");
        message["reply_to_message"] = text_message(private_chat(), Some(user(8, "B")), "original");
        message["reply_to_message"]["message_id"] = json!(4);

        let message = map_update(&update("message", 23, message))
            .unwrap()
            .unwrap();

        assert_eq!(
            message.reply_to.as_ref().map(PlatformMessageId::as_str),
            Some("4")
        );
        let quoted = message.quoted.expect("quoted content is mapped");
        assert_eq!(
            quoted.message_id.as_ref().map(PlatformMessageId::as_str),
            Some("4")
        );
        assert_eq!(quoted.parts.len(), 1);
        assert_eq!(quoted.parts[0], InboundPart::Text("original".to_owned()));
    }

    #[test]
    fn ignores_unrelated_update_kinds() {
        let query: teloxide::types::CallbackQuery = serde_json::from_value(json!({
            "id": "cb",
            "from": {
                "id": 7,
                "is_bot": false,
                "first_name": "A"
            },
            "chat_instance": "instance",
            "data": "x",
        }))
        .expect("fixture is a valid callback query");

        let update = Update {
            id: UpdateId(24),
            kind: UpdateKind::CallbackQuery(query),
        };

        assert!(map_update(&update).unwrap().is_none());
    }
}
