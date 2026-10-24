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
