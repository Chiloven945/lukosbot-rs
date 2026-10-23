use std::future::pending;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};
use tokio::time::{Instant as TokioInstant, sleep_until};
use tracing::warn;

use lukosbot_core::error::TaskFailure;
use lukosbot_core::runtime::{RuntimeWorker, WorkerContext};
use lukosbot_domain::{ChatAddress, OutboundMessage};

use super::manager::{GameManager, GameSessionKey};
use crate::basic::random::{RandomSource, ThreadRandom};

const SERVICE_NAME: &str = "twenty-four";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum TwentyFourError {
    #[error("the 24-point service is closed")]
    Closed,

    #[error("the 24-point service dropped the reply")]
    ReplyDropped,
}

/// A cloneable command-side handle for the 24-point game actor.
#[derive(Clone)]
pub struct TwentyFourHandle {
    sender: mpsc::Sender<GameEvent>,
}

impl TwentyFourHandle {
    pub async fn start(
        &self,
        key: GameSessionKey,
        address: ChatAddress,
    ) -> Result<String, TwentyFourError> {
        let (reply, response) = oneshot::channel();
        self.sender
            .send(GameEvent::Start {
                key,
                address,
                reply,
            })
            .await
            .map_err(|_| TwentyFourError::Closed)?;
        response.await.map_err(|_| TwentyFourError::ReplyDropped)
    }

    pub async fn submit(
        &self,
        key: GameSessionKey,
        input: String,
    ) -> Result<String, TwentyFourError> {
        let (reply, response) = oneshot::channel();
        self.sender
            .send(GameEvent::Submit { key, input, reply })
            .await
            .map_err(|_| TwentyFourError::Closed)?;
        response.await.map_err(|_| TwentyFourError::ReplyDropped)
    }

    pub async fn give_up(&self, key: GameSessionKey) -> Result<String, TwentyFourError> {
        let (reply, response) = oneshot::channel();
        self.sender
            .send(GameEvent::GiveUp { key, reply })
            .await
            .map_err(|_| TwentyFourError::Closed)?;
        response.await.map_err(|_| TwentyFourError::ReplyDropped)
    }
}

enum GameEvent {
    Start {
        key: GameSessionKey,
        address: ChatAddress,
        reply: oneshot::Sender<String>,
    },
    Submit {
        key: GameSessionKey,
        input: String,
        reply: oneshot::Sender<String>,
    },
    GiveUp {
        key: GameSessionKey,
        reply: oneshot::Sender<String>,
    },
}

/// The paired command handle and supervised worker for one 24-point service.
pub struct TwentyFourService {
    handle: TwentyFourHandle,
    worker: TwentyFourWorker,
}

impl TwentyFourService {
    pub fn new(time_limit: Duration) -> Self {
        Self::with_prefix(time_limit, "/")
    }

    pub fn with_prefix(time_limit: Duration, prefix: impl Into<String>) -> Self {
        Self::with_random_and_prefix(time_limit, prefix, Arc::new(ThreadRandom))
    }

    pub fn with_random(time_limit: Duration, random: Arc<dyn RandomSource>) -> Self {
        Self::with_random_and_prefix(time_limit, "/", random)
    }

    fn with_random_and_prefix(
        time_limit: Duration,
        prefix: impl Into<String>,
        random: Arc<dyn RandomSource>,
    ) -> Self {
        let (sender, receiver) = mpsc::channel(32);
        Self {
            handle: TwentyFourHandle { sender },
            worker: TwentyFourWorker {
                receiver: Mutex::new(Some(receiver)),
                time_limit,
                prefix: prefix.into(),
                random,
            },
        }
    }

    pub fn into_parts(self) -> (TwentyFourHandle, TwentyFourWorker) {
        (self.handle, self.worker)
    }
}

pub struct TwentyFourWorker {
    receiver: Mutex<Option<mpsc::Receiver<GameEvent>>>,
    time_limit: Duration,
    prefix: String,
    random: Arc<dyn RandomSource>,
}

#[async_trait]
impl RuntimeWorker for TwentyFourWorker {
    fn name(&self) -> &'static str {
        SERVICE_NAME
    }

    async fn run(&self, context: WorkerContext) -> Result<(), TaskFailure> {
        let mut receiver = self
            .receiver
            .lock()
            .expect("24-point worker receiver lock")
            .take()
            .ok_or_else(|| TaskFailure::new("24-point worker has already started"))?;
        let mut manager = GameManager::with_prefix(
            self.time_limit,
            self.prefix.clone(),
            Arc::clone(&self.random),
        );

        loop {
            let deadline = manager.next_deadline();
            tokio::select! {
                _ = context.cancel().cancelled() => break,
                event = receiver.recv() => {
                    let Some(event) = event else {
                        if context.cancel().is_cancelled() {
                            break;
                        }
                        return Err(TaskFailure::new(
                            "24-point worker channel closed unexpectedly",
                        ));
                    };
                    handle_event(&mut manager, event);
                }
                _ = wait_for_deadline(deadline) => {
                    for notice in manager.expire(Instant::now()) {
                        if let Err(error) = context
                            .outbound()
                            .send(
                                notice.address,
                                OutboundMessage::text(notice.text),
                            )
                            .await
                        {
                            warn!(%error, "failed to send 24-point timeout notification");
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

fn handle_event(manager: &mut GameManager, event: GameEvent) {
    let now = Instant::now();
    match event {
        GameEvent::Start {
            key,
            address,
            reply,
        } => {
            let _ = reply.send(manager.start(key, address, now));
        }
        GameEvent::Submit { key, input, reply } => {
            let _ = reply.send(manager.submit(&key, &input, now));
        }
        GameEvent::GiveUp { key, reply } => {
            let _ = reply.send(manager.give_up(&key, now));
        }
    }
}

async fn wait_for_deadline(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => sleep_until(TokioInstant::from_std(deadline)).await,
        None => pending::<()>().await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lukosbot_core::testing::{MemoryStateStore, RecordingAdapter};
    use lukosbot_core::{AppRuntime, RuntimeSettings};
    use lukosbot_domain::{ChatKind, PlatformChatId, PlatformKind, PlatformUserId};
    use std::sync::Arc;
    use tokio_util::sync::CancellationToken;

    fn address() -> ChatAddress {
        ChatAddress::new(
            PlatformKind::Telegram,
            PlatformChatId::new("1").unwrap(),
            ChatKind::Private,
        )
    }

    fn key() -> GameSessionKey {
        GameSessionKey {
            platform: PlatformKind::Telegram,
            user_id: PlatformUserId::new("1").unwrap(),
        }
    }

    #[tokio::test]
    async fn worker_times_out_and_sends_to_the_original_chat() {
        let (handle, worker) = TwentyFourService::new(Duration::from_millis(50)).into_parts();
        let adapter = RecordingAdapter::new(PlatformKind::Telegram);
        let runtime = AppRuntime::builder(
            RuntimeSettings {
                inbound_capacity: 4,
                outbound_capacity: 4,
                handler_timeout: Duration::from_secs(1),
                send_timeout: Duration::from_secs(1),
                shutdown_timeout: Duration::from_secs(2),
            },
            Arc::new(lukosbot_core::PingHandler),
            Arc::new(MemoryStateStore::new()),
        )
        .add_worker(Arc::new(worker))
        .unwrap()
        .add_platform(adapter.clone())
        .unwrap()
        .build();

        let cancel = CancellationToken::new();
        let shutdown = cancel.clone();
        let task =
            tokio::spawn(
                async move { runtime.run(async move { shutdown.cancelled().await }).await },
            );

        let _ = handle.start(key(), address()).await.unwrap();
        adapter.wait_for_sends(1).await;
        cancel.cancel();
        task.await.unwrap().unwrap();
        assert_eq!(adapter.sent()[0].address, address());
    }
}
