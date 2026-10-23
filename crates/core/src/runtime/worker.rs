//! Supervised long-lived runtime workers.

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use crate::error::TaskFailure;
use crate::messaging::outbound::OutboundHandle;

/// Dependencies available to a long-lived runtime worker.
///
/// Workers run for the whole application lifetime and are supervised by the runtime. The
/// outbound handle is included because features such as game timers must be able to notify a chat
/// without an active inbound message.
pub struct WorkerContext {
    outbound: OutboundHandle,
    cancel: CancellationToken,
}

impl WorkerContext {
    pub(crate) fn new(outbound: OutboundHandle, cancel: CancellationToken) -> Self {
        Self { outbound, cancel }
    }

    pub fn outbound(&self) -> &OutboundHandle {
        &self.outbound
    }

    pub fn cancel(&self) -> &CancellationToken {
        &self.cancel
    }
}

/// A runtime-managed component that runs until cancellation.
#[async_trait]
pub trait RuntimeWorker: Send + Sync {
    fn name(&self) -> &'static str;

    async fn run(&self, context: WorkerContext) -> Result<(), TaskFailure>;
}
