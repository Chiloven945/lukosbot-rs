//! Application ports, message dispatch, outbound routing, and the supervised runtime.
//!
//! This crate owns the stable boundaries between platform adapters, business handlers, and
//! infrastructure. It does not depend on any platform SDK, database driver, or HTTP client.

#![forbid(unsafe_code)]

pub mod error;
pub mod handler;
pub mod messaging;
pub mod ports;
pub mod runtime;
#[cfg(feature = "test-util")]
pub mod testing;

pub use error::TaskFailure;
pub use handler::{HandlerContext, HandlerError, MessageHandler, PingHandler};
pub use messaging::outbound::{OutboundHandle, SendError};
pub use ports::{
    InboundSender, PlatformAdapter, PlatformError, StateKey, StateScope, StateStore,
    StateStoreError, VersionedValue,
};
pub use runtime::{
    AppRuntime, MetricsSnapshot, RuntimeBuilder, RuntimeError, RuntimeMetrics, RuntimeSettings,
    RuntimeWorker, ShutdownReport, WorkerContext,
};
