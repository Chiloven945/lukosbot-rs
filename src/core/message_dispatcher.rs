use std::{
    collections::HashMap,
    sync::atomic::{AtomicBool, Ordering},
    sync::{Arc, Mutex},
};
use std::time::Instant;
use tokio::sync::{mpsc, Mutex as AsyncMutex};
use tracing::info;

use crate::core::message_sender_hub::MessageSenderHub;
use crate::core::pipeline_processor::PipelineProcessor;
use crate::model::MessageIn;

#[derive(Clone)]
pub struct MessageDispatcher {
    pipeline: Arc<PipelineProcessor>,
    hub: MessageSenderHub,
    prefix: Option<String>,
    running: Arc<AtomicBool>,
    chat_locks: Arc<Mutex<HashMap<i64, Arc<AsyncMutex<()>>>>>,
}

impl MessageDispatcher {
    pub fn new(pipeline: PipelineProcessor, hub: MessageSenderHub, prefix: String) -> Self {
        Self {
            pipeline: Arc::new(pipeline),
            hub,
            prefix: Some(prefix),
            running: Arc::new(AtomicBool::new(true)),
            chat_locks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    pub async fn run(self: Arc<Self>, mut rx: mpsc::UnboundedReceiver<MessageIn>) {
        while self.running.load(Ordering::SeqCst) {
            let Some(input) = rx.recv().await else {
                break;
            };

            if let Some(prefix) = &self.prefix {
                let t = input.text.trim();
                if !t.starts_with(prefix) {
                    continue;
                }
            }

            let hub = self.hub.clone();
            let pipeline = self.pipeline.clone();
            let chat_id = input.addr.chat_id;

            let lock = {
                let mut m = self.chat_locks.lock().unwrap();
                m.entry(chat_id)
                    .or_insert_with(|| Arc::new(AsyncMutex::new(())))
                    .clone()
            };

            tokio::spawn(async move {
                info!(
                    "IN <- [{:?}] user={:?} chat={} text=\"{}\"",
                    input.addr.platform, input.user_id, input.addr.chat_id, input.text
                );

                let t0 = Instant::now();
                let outs = pipeline.handle(input);
                let cost_ms = t0.elapsed().as_millis();

                if outs.is_empty() {
                    info!("PIPELINE result: empty ({} ms)", cost_ms);
                    return;
                }

                info!(
                    "PIPELINE result: {} message(s) ({} ms)",
                    outs.len(),
                    cost_ms
                );

                let _g = lock.lock().await;
                hub.send_batch(outs, true).await;
            });
        }
    }
}
