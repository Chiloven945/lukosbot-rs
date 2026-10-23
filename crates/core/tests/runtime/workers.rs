use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

use lukosbot_core::testing::{MemoryStateStore, RecordingAdapter};
use lukosbot_core::{AppRuntime, PingHandler, RuntimeError, StateStore};
use lukosbot_domain::{OutboundPart, PlatformKind};

use super::support::{address, settings};

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
