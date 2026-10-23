use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tokio::time::timeout;

use lukosbot_core::state::StateMaintenanceWorker;
use lukosbot_core::testing::{MemoryStateStore, RecordingAdapter};
use lukosbot_core::{AppRuntime, PingHandler, StateStore};
use lukosbot_domain::PlatformKind;

use super::support::{settings, spawn_runtime};

#[tokio::test]
async fn state_maintenance_worker_deletes_expired_rows_and_cancels() {
    let store = Arc::new(MemoryStateStore::new());
    let key = lukosbot_core::StateKey::global("test", "expired").unwrap();
    store
        .put(
            key,
            serde_json::json!(1),
            Some(Utc::now() - chrono::Duration::seconds(1)),
        )
        .await
        .unwrap();

    let worker = Arc::new(
        StateMaintenanceWorker::new(
            Arc::clone(&store) as Arc<dyn StateStore>,
            Duration::from_millis(10),
        )
        .unwrap(),
    );
    let adapter = RecordingAdapter::new(PlatformKind::Telegram);
    let runtime = AppRuntime::builder(
        settings(),
        Arc::new(PingHandler),
        Arc::clone(&store) as Arc<dyn StateStore>,
    )
    .add_worker(worker)
    .unwrap()
    .add_platform(adapter)
    .unwrap()
    .build();

    let (cancel, task) = spawn_runtime(runtime);
    timeout(Duration::from_secs(2), async {
        loop {
            if store.is_empty().await {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("cleanup worker removes expired rows");

    cancel.cancel();
    timeout(Duration::from_secs(3), task)
        .await
        .expect("runtime stops")
        .expect("task joins")
        .expect("clean shutdown");
}
