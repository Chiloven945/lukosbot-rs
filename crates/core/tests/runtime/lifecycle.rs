use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use tokio::time::timeout;

use lukosbot_core::handler::{HandlerContext, HandlerError, MessageHandler};
use lukosbot_core::testing::{AdapterBehavior, MemoryStateStore, RecordingAdapter, wait_until};
use lukosbot_core::{AppRuntime, PingHandler, RuntimeError, StateStore};
use lukosbot_domain::{InboundMessage, OutboundPart, PlatformKind};

use super::support::{address, build_runtime, inbound, settings, spawn_runtime};

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
