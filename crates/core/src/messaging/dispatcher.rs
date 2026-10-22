//! Serial message dispatch.
//!
//! One dispatcher processes inbound messages in order. This is the correctness baseline for the
//! base runtime; cross-chat concurrency requires a separate design with explicit ordering,
//! mailbox limits, and shutdown semantics.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, debug, info_span, warn};

use lukosbot_domain::InboundMessage;

use crate::handler::{HandlerContext, MessageHandler};
use crate::runtime::metrics::RuntimeMetrics;

pub(crate) struct Dispatcher {
    inbound: mpsc::Receiver<InboundMessage>,
    handler: Arc<dyn MessageHandler>,
    context: Arc<HandlerContext>,
    handler_timeout: Duration,
    metrics: Arc<RuntimeMetrics>,
    cancel: CancellationToken,
}

impl Dispatcher {
    pub(crate) fn new(
        inbound: mpsc::Receiver<InboundMessage>,
        handler: Arc<dyn MessageHandler>,
        context: Arc<HandlerContext>,
        handler_timeout: Duration,
        metrics: Arc<RuntimeMetrics>,
        cancel: CancellationToken,
    ) -> Self {
        Self {
            inbound,
            handler,
            context,
            handler_timeout,
            metrics,
            cancel,
        }
    }

    pub(crate) async fn run(mut self) {
        debug!("dispatcher started");

        loop {
            let message = tokio::select! {
                biased;

                _ = self.cancel.cancelled() => {
                    let dropped = self.inbound.len();
                    if dropped > 0 {
                        self.metrics
                            .inbound_dropped
                            .fetch_add(dropped as u64, Ordering::Relaxed);
                        warn!(
                            dropped,
                            "discarding queued inbound messages during shutdown"
                        );
                    }
                    break;
                }

                message = self.inbound.recv() => message,
            };

            match message {
                Some(message) => self.handle(message).await,
                None => {
                    debug!("all inbound senders dropped; dispatcher stopping");
                    break;
                }
            }
        }

        debug!("dispatcher stopped");
    }

    async fn handle(&self, message: InboundMessage) {
        self.metrics
            .inbound_received
            .fetch_add(1, Ordering::Relaxed);

        let platform = message.address.platform;
        let chat_id = message.address.chat_id.clone();
        let span = info_span!(
            "handle_message",
            platform = %platform,
            chat_id = %chat_id
        );

        let result = timeout(
            self.handler_timeout,
            self.handler.handle(&self.context, message).instrument(span),
        )
        .await;

        match result {
            Ok(Ok(())) => {
                self.metrics.handled_ok.fetch_add(1, Ordering::Relaxed);
            }
            Ok(Err(error)) => {
                self.metrics.handled_failed.fetch_add(1, Ordering::Relaxed);
                warn!(error = %error, "message handler failed");
            }
            Err(_) => {
                self.metrics.handled_failed.fetch_add(1, Ordering::Relaxed);
                warn!(
                    timeout_secs = self.handler_timeout.as_secs_f64(),
                    "message handler timed out"
                );
            }
        }
    }
}
