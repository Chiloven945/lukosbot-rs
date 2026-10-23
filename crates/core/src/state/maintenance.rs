use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use chrono::Utc;
use thiserror::Error;
use tokio::time::MissedTickBehavior;
use tracing::{debug, warn};

use crate::error::TaskFailure;
use crate::ports::StateStore;
use crate::runtime::{RuntimeWorker, WorkerContext};

/// A supervised worker that removes physically expired state records.
#[derive(Clone)]
pub struct StateMaintenanceWorker {
    store: Arc<dyn StateStore>,
    interval: Duration,
}

impl StateMaintenanceWorker {
    pub fn new(
        store: Arc<dyn StateStore>,
        interval: Duration,
    ) -> Result<Self, StateMaintenanceError> {
        if interval.is_zero() {
            return Err(StateMaintenanceError::ZeroInterval);
        }
        Ok(Self { store, interval })
    }
}

#[async_trait]
impl RuntimeWorker for StateMaintenanceWorker {
    fn name(&self) -> &'static str {
        "state-maintenance"
    }

    async fn run(&self, context: WorkerContext) -> Result<(), TaskFailure> {
        let mut ticker = tokio::time::interval(self.interval);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        // The first tick completes immediately; wait one full interval before the first cleanup.
        ticker.tick().await;

        loop {
            tokio::select! {
                _ = context.cancel().cancelled() => return Ok(()),
                _ = ticker.tick() => {
                    let started = Instant::now();
                    match self.store.delete_expired(Utc::now()).await {
                        Ok(deleted) => {
                            debug!(
                                deleted,
                                elapsed = ?started.elapsed(),
                                "expired state cleanup completed"
                            );
                        }
                        Err(error) => {
                            warn!(
                                %error,
                                elapsed = ?started.elapsed(),
                                "expired state cleanup failed; retrying next interval"
                            );
                        }
                    }
                }
            }
        }
    }
}

/// State maintenance configuration failure.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum StateMaintenanceError {
    #[error("expiry cleanup interval must be greater than zero")]
    ZeroInterval,
}
