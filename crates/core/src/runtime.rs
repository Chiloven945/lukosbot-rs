//! Application runtime: supervision, dispatch wiring, and graceful shutdown.

mod app_runtime;
pub mod metrics;
mod task_supervisor;
mod worker;

pub use app_runtime::{AppRuntime, RuntimeBuilder, RuntimeError, RuntimeSettings, ShutdownReport};
pub use metrics::{MetricsSnapshot, RuntimeMetrics};
pub use worker::{RuntimeWorker, WorkerContext};
