use thiserror::Error;

/// A policy rule configuration failure.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum PolicyError {
    #[error("policy rule `{id}` is invalid: {message}")]
    InvalidRule { id: String, message: String },
}
