use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

use lukosbot_core::handler::{HandlerContext, HandlerError, MessageHandler};
use lukosbot_core::testing::{AdapterBehavior, MemoryStateStore, RecordingAdapter, wait_until};
use lukosbot_core::{
    AppRuntime, PingHandler, PlatformAdapter, RuntimeError, RuntimeMetrics, RuntimeSettings,
    StateStore,
};
use lukosbot_domain::{
    ChatAddress, ChatKind, InboundMessage, InboundPart, OutboundPart, PlatformChatId, PlatformKind,
    PlatformMessageId, PlatformUserId, Sender,
};

fn address(platform: PlatformKind) -> ChatAddress {
    ChatAddress::new(
        platform,
        PlatformChatId::new("100").expect("valid chat id"),
        ChatKind::Private,
    )
}

fn inbound(address: &ChatAddress, text: &str) -> InboundMessage {
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
    }
}

fn settings() -> RuntimeSettings {
    RuntimeSettings {
        inbound_capacity: 8,
        outbound_capacity: 4,
        handler_timeout: Duration::from_millis(200),
        send_timeout: Duration::from_millis(200),
        shutdown_timeout: Duration::from_secs(2),
    }
}

fn build_runtime(
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

fn spawn_runtime(
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

#[tokio::test]
async fn dispatches_inbound_ping_and_returns_pong() {
    let adapter = RecordingAdapter::new(PlatformKind::Telegram);
    adapter.push_inbound(inbound(&address(PlatformKind::Telegram), "ping"));

    let (runtime, metrics) = build_runtime(adapter.clone(), Arc::new(PingHandler), settings());
    let (cancel, task) = spawn_runtime(runtime);

    adapter.wait_for_sends(1).await;
    let sent = adapter.sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].address, address(PlatformKind::Telegram));
    assert_eq!(
        sent[0].message.parts,
        vec![OutboundPart::Text("pong".to_owned())]
    );

    cancel.cancel();
    let report = timeout(Duration::from_secs(3), task)
        .await
        .expect("runtime stops")
        .expect("task joins")
        .expect("clean shutdown");

    assert_eq!(report.metrics.inbound_received, 1);
    assert_eq!(report.metrics.handled_ok, 1);
    assert_eq!(report.metrics.outbound_sent, 1);
    assert_eq!(metrics.snapshot().outbound_sent, 1);
    assert_eq!(adapter.shutdown_count(), 1);
}

#[tokio::test]
async fn cancellation_wakes_blocked_receiver_and_shuts_down() {
    let adapter = RecordingAdapter::new(PlatformKind::Telegram);
    let (runtime, _) = build_runtime(adapter.clone(), Arc::new(PingHandler), settings());
    let (cancel, task) = spawn_runtime(runtime);

    tokio::time::sleep(Duration::from_millis(20)).await;
    cancel.cancel();

    let report = timeout(Duration::from_secs(3), task)
        .await
        .expect("runtime stops")
        .expect("task joins")
        .expect("clean shutdown");

    assert_eq!(report.metrics.inbound_received, 0);
    assert!(!report.timed_out);
    assert_eq!(adapter.shutdown_count(), 1);
}

#[tokio::test]
async fn supervised_adapter_failure_stops_the_runtime() {
    let adapter =
        RecordingAdapter::with_behavior(PlatformKind::Telegram, AdapterBehavior::PanicOnRun);
    let (runtime, _) = build_runtime(adapter, Arc::new(PingHandler), settings());

    let result = timeout(
        Duration::from_secs(3),
        runtime.run(std::future::pending::<()>()),
    )
    .await
    .expect("runtime stops");

    assert!(matches!(result, Err(RuntimeError::TaskFailed(_))));
}

#[tokio::test]
async fn handler_failures_are_isolated() {
    struct FailingOnce {
        calls: AtomicUsize,
    }

    #[async_trait]
    impl MessageHandler for FailingOnce {
        async fn handle(
            &self,
            _context: &HandlerContext,
            _message: InboundMessage,
        ) -> Result<(), HandlerError> {
            if self.calls.fetch_add(1, Ordering::Relaxed) == 0 {
                Err(HandlerError::failed("injected handler failure"))
            } else {
                Ok(())
            }
        }
    }

    let adapter = RecordingAdapter::new(PlatformKind::Telegram);
    adapter.push_inbound(inbound(&address(PlatformKind::Telegram), "one"));
    adapter.push_inbound(inbound(&address(PlatformKind::Telegram), "two"));

    let handler = Arc::new(FailingOnce {
        calls: AtomicUsize::new(0),
    });
    let (runtime, metrics) = build_runtime(adapter, handler.clone(), settings());
    let (cancel, task) = spawn_runtime(runtime);

    wait_until(|| metrics.snapshot().handled_ok + metrics.snapshot().handled_failed >= 2).await;
    cancel.cancel();
    let report = timeout(Duration::from_secs(3), task)
        .await
        .expect("runtime stops")
        .expect("task joins")
        .expect("clean shutdown");

    assert_eq!(report.metrics.handled_failed, 1);
    assert_eq!(report.metrics.handled_ok, 1);
    assert_eq!(handler.calls.load(Ordering::Relaxed), 2);
}

#[tokio::test]
async fn slow_handlers_are_timed_out_and_do_not_stop_the_dispatcher() {
    struct Slow {
        calls: AtomicUsize,
    }

    #[async_trait]
    impl MessageHandler for Slow {
        async fn handle(
            &self,
            _context: &HandlerContext,
            _message: InboundMessage,
        ) -> Result<(), HandlerError> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            tokio::time::sleep(Duration::from_millis(100)).await;
            Ok(())
        }
    }

    let adapter = RecordingAdapter::new(PlatformKind::Telegram);
    adapter.push_inbound(inbound(&address(PlatformKind::Telegram), "slow"));

    let handler = Arc::new(Slow {
        calls: AtomicUsize::new(0),
    });
    let mut settings = settings();
    settings.handler_timeout = Duration::from_millis(10);
    let (runtime, metrics) = build_runtime(adapter, handler, settings);
    let (cancel, task) = spawn_runtime(runtime);

    wait_until(|| metrics.snapshot().handled_failed >= 1).await;
    cancel.cancel();
    let report = timeout(Duration::from_secs(3), task)
        .await
        .expect("runtime stops")
        .expect("task joins")
        .expect("clean shutdown");

    assert_eq!(report.metrics.handled_failed, 1);
}

#[tokio::test]
async fn outbound_timeout_is_reported_without_retrying() {
    let adapter = RecordingAdapter::with_behavior(
        PlatformKind::Telegram,
        AdapterBehavior::DelaySend(Duration::from_millis(200)),
    );
    adapter.push_inbound(inbound(&address(PlatformKind::Telegram), "ping"));

    let mut settings = settings();
    settings.send_timeout = Duration::from_millis(10);
    let (runtime, metrics) = build_runtime(adapter.clone(), Arc::new(PingHandler), settings);
    let (cancel, task) = spawn_runtime(runtime);

    wait_until(|| metrics.snapshot().outbound_failed >= 1).await;
    cancel.cancel();
    let report = timeout(Duration::from_secs(3), task)
        .await
        .expect("runtime stops")
        .expect("task joins")
        .expect("clean shutdown");

    assert_eq!(report.metrics.outbound_failed, 1);
    assert_eq!(adapter.sent().len(), 0);
}

#[tokio::test]
async fn outbound_messages_are_delivered_in_submission_order() {
    struct Echo;

    #[async_trait]
    impl MessageHandler for Echo {
        async fn handle(
            &self,
            context: &HandlerContext,
            message: InboundMessage,
        ) -> Result<(), HandlerError> {
            let text = message.text_parts().next().unwrap_or_default();
            context
                .outbound()
                .send(
                    message.address.clone(),
                    lukosbot_domain::OutboundMessage::text(text),
                )
                .await?;
            Ok(())
        }
    }

    let adapter = RecordingAdapter::with_behavior(
        PlatformKind::Telegram,
        AdapterBehavior::DelaySend(Duration::from_millis(5)),
    );

    for text in ["a", "b", "c"] {
        adapter.push_inbound(inbound(&address(PlatformKind::Telegram), text));
    }

    let (runtime, metrics) = build_runtime(adapter.clone(), Arc::new(Echo), settings());
    let (cancel, task) = spawn_runtime(runtime);

    adapter.wait_for_sends(3).await;
    let sent = adapter.sent();
    let texts: Vec<_> = sent
        .iter()
        .map(|record| match &record.message.parts[0] {
            OutboundPart::Text(text) => text.clone(),
            other => panic!("unexpected outbound part: {other:?}"),
        })
        .collect();

    assert_eq!(texts, ["a", "b", "c"]);
    assert_eq!(metrics.snapshot().outbound_sent, 3);

    cancel.cancel();
    timeout(Duration::from_secs(3), task)
        .await
        .expect("runtime stops")
        .expect("task joins")
        .expect("clean shutdown");
}

#[tokio::test]
async fn shutdown_is_bounded_when_an_adapter_hangs() {
    let adapter =
        RecordingAdapter::with_behavior(PlatformKind::Telegram, AdapterBehavior::HangOnShutdown);
    let mut settings = settings();
    settings.shutdown_timeout = Duration::from_millis(50);

    let (runtime, _) = build_runtime(adapter, Arc::new(PingHandler), settings);
    let (cancel, task) = spawn_runtime(runtime);

    tokio::time::sleep(Duration::from_millis(10)).await;
    cancel.cancel();

    let report = timeout(Duration::from_secs(3), task)
        .await
        .expect("runtime stops")
        .expect("task joins")
        .expect("clean shutdown");

    assert!(report.timed_out);
}

#[tokio::test]
async fn duplicate_platforms_are_rejected() {
    let builder = AppRuntime::builder(
        settings(),
        Arc::new(PingHandler),
        Arc::new(MemoryStateStore::new()) as Arc<dyn StateStore>,
    )
    .add_platform(RecordingAdapter::new(PlatformKind::Telegram))
    .expect("first adapter registers");

    let error = match builder.add_platform(RecordingAdapter::new(PlatformKind::Telegram)) {
        Ok(_) => panic!("duplicate adapter should be rejected"),
        Err(error) => error,
    };

    assert!(matches!(
        error,
        RuntimeError::DuplicatePlatform(PlatformKind::Telegram)
    ));
}

#[tokio::test]
async fn runtime_requires_at_least_one_platform() {
    let runtime = AppRuntime::builder(
        settings(),
        Arc::new(PingHandler),
        Arc::new(MemoryStateStore::new()) as Arc<dyn StateStore>,
    )
    .build();

    let error = runtime
        .run(std::future::pending::<()>())
        .await
        .expect_err("no platforms is an error");

    assert!(matches!(error, RuntimeError::NoPlatforms));
}

struct SendWorker;

#[async_trait]
impl lukosbot_core::RuntimeWorker for SendWorker {
    fn name(&self) -> &'static str {
        "send-worker"
    }

    async fn run(
        &self,
        context: lukosbot_core::WorkerContext,
    ) -> Result<(), lukosbot_core::TaskFailure> {
        context
            .outbound()
            .send(
                address(PlatformKind::Telegram),
                lukosbot_domain::OutboundMessage::text("worker"),
            )
            .await
            .map_err(|error| {
                lukosbot_core::TaskFailure::new("worker send failed").with_source(error)
            })?;
        context.cancel().cancelled().await;
        Ok(())
    }
}

struct ExitWorker;

#[async_trait]
impl lukosbot_core::RuntimeWorker for ExitWorker {
    fn name(&self) -> &'static str {
        "exit-worker"
    }

    async fn run(
        &self,
        _context: lukosbot_core::WorkerContext,
    ) -> Result<(), lukosbot_core::TaskFailure> {
        Ok(())
    }
}

struct PanicWorker;

#[async_trait]
impl lukosbot_core::RuntimeWorker for PanicWorker {
    fn name(&self) -> &'static str {
        "panic-worker"
    }

    async fn run(
        &self,
        _context: lukosbot_core::WorkerContext,
    ) -> Result<(), lukosbot_core::TaskFailure> {
        panic!("worker panic");
    }
}

struct WaitWorker;

#[async_trait]
impl lukosbot_core::RuntimeWorker for WaitWorker {
    fn name(&self) -> &'static str {
        "wait-worker"
    }

    async fn run(
        &self,
        context: lukosbot_core::WorkerContext,
    ) -> Result<(), lukosbot_core::TaskFailure> {
        context.cancel().cancelled().await;
        Ok(())
    }
}

#[tokio::test]
async fn worker_receives_the_outbound_handle_and_cancels_cleanly() {
    let adapter = RecordingAdapter::new(PlatformKind::Telegram);
    let cancel = CancellationToken::new();
    let shutdown = cancel.clone();

    let runtime = AppRuntime::builder(
        settings(),
        Arc::new(PingHandler),
        Arc::new(MemoryStateStore::new()) as Arc<dyn StateStore>,
    )
    .add_worker(Arc::new(SendWorker))
    .unwrap()
    .add_platform(adapter.clone())
    .unwrap()
    .build();

    let task =
        tokio::spawn(async move { runtime.run(async move { shutdown.cancelled().await }).await });

    adapter.wait_for_sends(1).await;
    assert_eq!(
        adapter.sent()[0].message.parts,
        vec![OutboundPart::Text("worker".to_owned())]
    );
    cancel.cancel();
    let report = timeout(Duration::from_secs(3), task)
        .await
        .expect("runtime stops")
        .expect("task joins")
        .expect("clean shutdown");
    assert!(!report.timed_out);
}

#[tokio::test]
async fn worker_unexpected_exit_is_a_runtime_failure() {
    let adapter = RecordingAdapter::new(PlatformKind::Telegram);
    let runtime = AppRuntime::builder(
        settings(),
        Arc::new(PingHandler),
        Arc::new(MemoryStateStore::new()) as Arc<dyn StateStore>,
    )
    .add_worker(Arc::new(ExitWorker))
    .unwrap()
    .add_platform(adapter)
    .unwrap()
    .build();

    let result = timeout(
        Duration::from_secs(3),
        runtime.run(std::future::pending::<()>()),
    )
    .await
    .expect("runtime stops");

    assert!(matches!(result, Err(RuntimeError::TaskFailed(_))));
}

#[tokio::test]
async fn worker_panic_is_supervised() {
    let adapter = RecordingAdapter::new(PlatformKind::Telegram);
    let runtime = AppRuntime::builder(
        settings(),
        Arc::new(PingHandler),
        Arc::new(MemoryStateStore::new()) as Arc<dyn StateStore>,
    )
    .add_worker(Arc::new(PanicWorker))
    .unwrap()
    .add_platform(adapter)
    .unwrap()
    .build();

    let result = timeout(
        Duration::from_secs(3),
        runtime.run(std::future::pending::<()>()),
    )
    .await
    .expect("runtime stops");

    assert!(matches!(result, Err(RuntimeError::TaskFailed(_))));
}

#[test]
fn worker_names_are_validated() {
    let adapter = RecordingAdapter::new(PlatformKind::Telegram);
    let builder = AppRuntime::builder(
        settings(),
        Arc::new(PingHandler),
        Arc::new(MemoryStateStore::new()) as Arc<dyn StateStore>,
    )
    .add_worker(Arc::new(WaitWorker))
    .unwrap();

    assert!(builder.add_worker(Arc::new(WaitWorker)).is_err());
    assert!(
        AppRuntime::builder(
            settings(),
            Arc::new(PingHandler),
            Arc::new(MemoryStateStore::new()) as Arc<dyn StateStore>,
        )
        .add_worker(Arc::new(SendWorker))
        .unwrap()
        .add_platform(adapter)
        .is_ok()
    );
}

#[test]
fn reserved_worker_names_are_rejected() {
    struct Reserved;

    #[async_trait]
    impl lukosbot_core::RuntimeWorker for Reserved {
        fn name(&self) -> &'static str {
            "dispatcher"
        }

        async fn run(
            &self,
            _context: lukosbot_core::WorkerContext,
        ) -> Result<(), lukosbot_core::TaskFailure> {
            Ok(())
        }
    }

    assert!(
        AppRuntime::builder(
            settings(),
            Arc::new(PingHandler),
            Arc::new(MemoryStateStore::new()) as Arc<dyn StateStore>,
        )
        .add_worker(Arc::new(Reserved))
        .is_err()
    );
}

struct HangWorker;

#[async_trait]
impl lukosbot_core::RuntimeWorker for HangWorker {
    fn name(&self) -> &'static str {
        "hang-worker"
    }

    async fn run(
        &self,
        _context: lukosbot_core::WorkerContext,
    ) -> Result<(), lukosbot_core::TaskFailure> {
        std::future::pending::<()>().await;
        Ok(())
    }
}

#[tokio::test]
async fn shutdown_remains_bounded_when_a_worker_ignores_cancellation() {
    let adapter = RecordingAdapter::new(PlatformKind::Telegram);
    let mut settings = settings();
    settings.shutdown_timeout = Duration::from_millis(100);
    let runtime = AppRuntime::builder(
        settings,
        Arc::new(PingHandler),
        Arc::new(MemoryStateStore::new()) as Arc<dyn StateStore>,
    )
    .add_worker(Arc::new(HangWorker))
    .unwrap()
    .add_platform(adapter)
    .unwrap()
    .build();

    let cancel = CancellationToken::new();
    let shutdown = cancel.clone();
    let task =
        tokio::spawn(async move { runtime.run(async move { shutdown.cancelled().await }).await });

    tokio::time::sleep(Duration::from_millis(20)).await;
    cancel.cancel();

    let report = timeout(Duration::from_secs(3), task)
        .await
        .expect("runtime stops")
        .expect("task joins")
        .expect("shutdown report");
    assert!(report.timed_out);
    assert!(report.abandoned_tasks >= 1);
}

#[test]
fn worker_names_cannot_shadow_platform_names() {
    struct TelegramNamedWorker;

    #[async_trait]
    impl lukosbot_core::RuntimeWorker for TelegramNamedWorker {
        fn name(&self) -> &'static str {
            "telegram"
        }

        async fn run(
            &self,
            context: lukosbot_core::WorkerContext,
        ) -> Result<(), lukosbot_core::TaskFailure> {
            context.cancel().cancelled().await;
            Ok(())
        }
    }

    let adapter = RecordingAdapter::new(PlatformKind::Telegram);
    let builder = AppRuntime::builder(
        settings(),
        Arc::new(PingHandler),
        Arc::new(MemoryStateStore::new()) as Arc<dyn StateStore>,
    );

    assert!(
        builder
            .add_platform(adapter.clone())
            .unwrap()
            .add_worker(Arc::new(TelegramNamedWorker))
            .is_err()
    );
}
