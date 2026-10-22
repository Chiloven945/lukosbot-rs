//! Unified outbound routing.
//!
//! Handlers never call a platform SDK directly. Every outbound message goes through a bounded
//! queue to a single worker, which resolves the adapter by [`PlatformKind`]. The worker is
//! deliberately serial in the base runtime: it provides a simple ordering baseline and clear
//! backpressure. Per-platform workers can be introduced later, based on measurements.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use thiserror::Error;
use tokio::sync::{mpsc, oneshot};
use tokio::time::timeout;
use tracing::debug;

use lukosbot_domain::{ChatAddress, OutboundMessage, PlatformKind, SentMessage};

use crate::ports::{PlatformAdapter, PlatformError};
use crate::runtime::metrics::RuntimeMetrics;

/// Adapters keyed by the platform they handle.
pub(crate) type AdapterRegistry = HashMap<PlatformKind, Arc<dyn PlatformAdapter>>;

/// Outbound delivery failure.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum SendError {
    #[error("no adapter is registered for {platform}")]
    NoAdapter { platform: PlatformKind },

    #[error("the outbound queue is closed")]
    Closed,

    #[error("the outbound service is shutting down")]
    ShuttingDown,

    #[error("sending to {platform} timed out")]
    Timeout { platform: PlatformKind },

    #[error(transparent)]
    Platform(#[from] PlatformError),
}

struct OutboundRequest {
    address: ChatAddress,
    message: OutboundMessage,
    reply: oneshot::Sender<Result<SentMessage, SendError>>,
}

/// A cloneable handle used by handlers to submit outbound messages.
#[derive(Debug, Clone)]
pub struct OutboundHandle {
    sender: mpsc::Sender<OutboundRequest>,
    closed: Arc<AtomicBool>,
}

impl OutboundHandle {
    /// Queues a message and waits for the worker's result.
    ///
    /// Waiting for the bounded queue provides backpressure: a full queue pauses the caller
    /// instead of growing memory without bound.
    pub async fn send(
        &self,
        address: ChatAddress,
        message: OutboundMessage,
    ) -> Result<SentMessage, SendError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(SendError::ShuttingDown);
        }

        let (reply, response) = oneshot::channel();
        self.sender
            .send(OutboundRequest {
                address,
                message,
                reply,
            })
            .await
            .map_err(|_| SendError::Closed)?;

        response.await.map_err(|_| SendError::Closed)?
    }

    /// Rejects new sends. In-flight requests already queued are still drained by the worker.
    pub(crate) fn close(&self) {
        self.closed.store(true, Ordering::Release);
    }
}

pub(crate) struct OutboundService;

impl OutboundService {
    pub(crate) fn start(
        registry: AdapterRegistry,
        capacity: usize,
        send_timeout: Duration,
        metrics: Arc<RuntimeMetrics>,
    ) -> (OutboundHandle, OutboundWorker) {
        let (sender, receiver) = mpsc::channel(capacity);
        let handle = OutboundHandle {
            sender,
            closed: Arc::new(AtomicBool::new(false)),
        };
        let worker = OutboundWorker {
            receiver,
            registry,
            send_timeout,
            metrics,
        };
        (handle, worker)
    }
}

pub(crate) struct OutboundWorker {
    receiver: mpsc::Receiver<OutboundRequest>,
    registry: AdapterRegistry,
    send_timeout: Duration,
    metrics: Arc<RuntimeMetrics>,
}

impl OutboundWorker {
    pub(crate) async fn run(mut self) {
        debug!("outbound worker started");

        while let Some(request) = self.receiver.recv().await {
            let platform = request.address.platform;

            let result = match self.registry.get(&platform) {
                None => Err(SendError::NoAdapter { platform }),
                Some(adapter) => {
                    match timeout(
                        self.send_timeout,
                        adapter.send(&request.address, request.message),
                    )
                    .await
                    {
                        Ok(Ok(sent)) => Ok(sent),
                        Ok(Err(error)) => Err(SendError::Platform(error)),
                        Err(_) => Err(SendError::Timeout { platform }),
                    }
                }
            };

            match &result {
                Ok(sent) => {
                    self.metrics.outbound_sent.fetch_add(1, Ordering::Relaxed);
                    debug!(
                        platform = %sent.platform,
                        message_id = %sent.message_id,
                        "outbound message sent"
                    );
                }
                Err(error) => {
                    self.metrics.outbound_failed.fetch_add(1, Ordering::Relaxed);
                    debug!(error = %error, "outbound delivery failed");
                }
            }

            // The caller may have gone away; that is not a runtime failure.
            let _ = request.reply.send(result);
        }

        debug!("outbound worker stopped");
    }
}

impl std::fmt::Debug for OutboundWorker {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("OutboundWorker")
            .field("pending", &self.receiver.len())
            .field("adapters", &self.registry.len())
            .finish()
    }
}
