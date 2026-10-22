//! Supervision of long-running tasks.
//!
//! Every runtime task is spawned through this type. No task is allowed to be detached: an
//! unexpected exit is surfaced to the runtime, which starts a coordinated shutdown.

use std::future::Future;

use tokio::task::{JoinError, JoinSet};
use tracing::Instrument;

use crate::error::TaskFailure;

pub(crate) struct TaskOutcome {
    pub name: &'static str,
    pub result: Result<(), TaskFailure>,
}

#[derive(Default)]
pub(crate) struct TaskSupervisor {
    tasks: JoinSet<TaskOutcome>,
}

impl TaskSupervisor {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn spawn<F>(&mut self, name: &'static str, future: F)
    where
        F: Future<Output = Result<(), TaskFailure>> + Send + 'static,
    {
        let span = tracing::info_span!("task", name);
        self.tasks.spawn(
            async move {
                let result = future.await;
                TaskOutcome { name, result }
            }
            .instrument(span),
        );
    }

    pub(crate) fn len(&self) -> usize {
        self.tasks.len()
    }

    pub(crate) async fn join_next(&mut self) -> Option<Result<TaskOutcome, JoinError>> {
        self.tasks.join_next().await
    }

    /// Aborts all remaining tasks and returns how many were abandoned.
    pub(crate) fn abort_all(&mut self) -> usize {
        let abandoned = self.tasks.len();
        self.tasks.abort_all();
        abandoned
    }
}
