use async_trait::async_trait;
use bytes::Bytes;
use chrono::Utc;
use lukosbot_commands::media::cave::CaveService;
use lukosbot_core::StateStore;
use lukosbot_core::ports::{MediaLoadError, MediaLoadPolicy, MediaLoader, UnsupportedMediaLoader};
use lukosbot_core::testing::MemoryStateStore;
use lukosbot_domain::{
    ChatAddress, ChatKind, InboundMessage, InboundPart, MediaContent, MediaRef, MediaSource,
    PlatformChatId, PlatformKind, PlatformMessageId, PlatformUserId, QuotedMessage, Sender,
};
use std::sync::Arc;

fn address() -> ChatAddress {
    ChatAddress::new(
        PlatformKind::Telegram,
        PlatformChatId::new("100").unwrap(),
        ChatKind::Private,
    )
}

fn sender() -> Sender {
    Sender {
        id: PlatformUserId::new("7").unwrap(),
        display_name: None,
        username: None,
    }
}

fn message(parts: Vec<InboundPart>, quoted: Option<QuotedMessage>) -> InboundMessage {
    InboundMessage {
        address: address(),
        sender: sender(),
        message_id: Some(PlatformMessageId::new("10").unwrap()),
        received_at: Utc::now(),
        parts,
        reply_to: None,
        quoted,
        context: Default::default(),
    }
}

struct ImageLoader;

#[async_trait]
impl MediaLoader for ImageLoader {
    async fn load(
        &self,
        _reference: &MediaRef,
        _policy: MediaLoadPolicy,
    ) -> Result<MediaContent, MediaLoadError> {
        Ok(MediaContent {
            bytes: Bytes::from_static(b"png"),
            file_name: Some("image.png".to_owned()),
            mime_type: Some("image/png".to_owned()),
        })
    }
}

#[tokio::test]
async fn adds_gets_and_deletes_text_entries() {
    let store: Arc<dyn StateStore> = Arc::new(MemoryStateStore::new());
    let service = CaveService::new(Arc::clone(&store), 1024);

    let entry = service
        .add(
            &message(vec![InboundPart::Text("/cave add".to_owned())], None),
            Some("hello"),
            &UnsupportedMediaLoader,
            MediaLoadPolicy::platform(1024),
        )
        .await
        .unwrap();
    assert_eq!(entry.no, 1);
    assert_eq!(entry.text.as_deref(), Some("hello"));

    assert_eq!(
        service.get(1).await.unwrap().unwrap().text.as_deref(),
        Some("hello")
    );
    assert_eq!(service.random().await.unwrap().unwrap().no, 1);
    assert!(service.delete(1).await.unwrap());
    assert!(service.get(1).await.unwrap().is_none());
}

#[tokio::test]
async fn quoted_content_wins_over_current_content() {
    let store: Arc<dyn StateStore> = Arc::new(MemoryStateStore::new());
    let service = CaveService::new(store, 1024);
    let quoted = QuotedMessage {
        message_id: Some(PlatformMessageId::new("9").unwrap()),
        sender: Some(sender()),
        parts: vec![InboundPart::Text("quoted".to_owned())],
    };

    let entry = service
        .add(
            &message(vec![InboundPart::Text("current".to_owned())], Some(quoted)),
            Some("current"),
            &UnsupportedMediaLoader,
            MediaLoadPolicy::platform(1024),
        )
        .await
        .unwrap();

    assert_eq!(entry.text.as_deref(), Some("quoted"));
    assert_eq!(entry.source_message_id.as_deref(), Some("9"));
}

#[tokio::test]
async fn stores_and_replays_image_bytes() {
    let store: Arc<dyn StateStore> = Arc::new(MemoryStateStore::new());
    let service = CaveService::new(Arc::clone(&store), 1024);
    let image = MediaRef {
        source: MediaSource::PlatformFile {
            platform: PlatformKind::Telegram,
            file_id: "file".to_owned(),
        },
        file_name: None,
        mime_type: None,
        size_bytes: None,
    };

    let entry = service
        .add(
            &message(vec![InboundPart::Image(image)], None),
            Some("look"),
            &ImageLoader,
            MediaLoadPolicy::platform(1024),
        )
        .await
        .unwrap();
    assert!(entry.image.is_some());

    let outbound = service.to_outbound(&address(), &entry).unwrap();
    assert!(matches!(
        &outbound.parts[0],
        lukosbot_domain::OutboundPart::Image { .. }
    ));

    let reopened = CaveService::new(store, 1024);
    assert!(reopened.get(1).await.unwrap().unwrap().image.is_some());
}

#[tokio::test]
async fn empty_payload_is_rejected_before_writing() {
    let store: Arc<dyn StateStore> = Arc::new(MemoryStateStore::new());
    let service = CaveService::new(store, 1024);
    let error = service
        .add(
            &message(Vec::new(), None),
            Some(""),
            &UnsupportedMediaLoader,
            MediaLoadPolicy::platform(1024),
        )
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        lukosbot_commands::media::cave::service::CaveError::Empty
    ));
}
