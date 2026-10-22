//! Shared error utilities.

use thiserror::Error;

/// A boxed, thread-safe error used at boundaries where the concrete source type is not part of
/// the public contract (for example, a platform SDK error).
pub type BoxError = Box<dyn std::error::Error + Send + Sync + 'static>;

/// A failure reported by a supervised task.
#[derive(Debug, Error)]
#[error("{message}")]
pub struct TaskFailure {
    message: String,
    #[source]
    source: Option<BoxError>,
}

impl TaskFailure {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            source: None,
        }
    }

    pub fn with_source(mut self, source: impl Into<BoxError>) -> Self {
        self.source = Some(source.into());
        self
    }
}
