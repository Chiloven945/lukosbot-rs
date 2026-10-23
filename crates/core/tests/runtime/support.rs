use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tokio_util::sync::CancellationToken;

use lukosbot_core::handler::MessageHandler;
use lukosbot_core::testing::MemoryStateStore;
use lukosbot_core::{
    AppRuntime, PlatformAdapter, RuntimeError, RuntimeMetrics, RuntimeSettings, StateStore,
};
use lukosbot_domain::{
    ChatAddress, ChatKind, InboundMessage, InboundPart, PlatformChatId, PlatformKind,
    PlatformMessageId, PlatformUserId, Sender,
};

pub(crate) fn address(platform: PlatformKind) -> ChatAddress {
    ChatAddress::new(
        platform,
        PlatformChatId::new("100").expect("valid chat id"),
        ChatKind::Private,
    )
}

pub(crate) fn inbound(address: &ChatAddress, text: &str) -> InboundMessage {
    InboundMessage {
        address: address.clone(),
        sender: Sender {
            id: PlatformUserId::new("7").expect("valid user id"),
            display_name: Some("Tester".to_owned()),
            username: None,
        },
        message_id: Some(PlatformMessageId::new("11").expect("valid message id")),
        received_at: Utc::now(),
        parts: vec![InboundPart::Text(text.to_owned())],
        reply_to: None,
        context: Default::default(),
    }
}

pub(crate) fn settings() -> RuntimeSettings {
    RuntimeSettings {
        inbound_capacity: 8,
        outbound_capacity: 4,
        handler_timeout: Duration::from_millis(200),
        send_timeout: Duration::from_millis(200),
        shutdown_timeout: Duration::from_secs(2),
    }
}

pub(crate) fn build_runtime(
    adapter: Arc<dyn PlatformAdapter>,
    handler: Arc<dyn MessageHandler>,
    settings: RuntimeSettings,
) -> (AppRuntime, Arc<RuntimeMetrics>) {
    let runtime = AppRuntime::builder(
        settings,
        handler,
        Arc::new(MemoryStateStore::new()) as Arc<dyn StateStore>,
    )
    .add_platform(adapter)
    .expect("adapter registers")
    .build();
    let metrics = runtime.metrics();
    (runtime, metrics)
}

pub(crate) fn spawn_runtime(
    runtime: AppRuntime,
) -> (
    CancellationToken,
    tokio::task::JoinHandle<Result<lukosbot_core::ShutdownReport, RuntimeError>>,
) {
    let token = CancellationToken::new();
    let shutdown = token.clone();
    let task =
        tokio::spawn(async move { runtime.run(async move { shutdown.cancelled().await }).await });
    (token, task)
}
