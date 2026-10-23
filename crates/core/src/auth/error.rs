use thiserror::Error;

use crate::ports::StateStoreError;

/// A bot-administrator management failure.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum AdminError {
    #[error("stored bot administrator list is corrupt")]
    CorruptAdminList,

    #[error(transparent)]
    Storage(#[from] StateStoreError),

    #[error("refusing to remove the last effective bot administrator")]
    LastAdminProtection,
}

/// A platform chat-administrator lookup failure.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ChatAdminLookupError {
    #[error("chat administrator lookup is unavailable")]
    Unavailable,

    #[error("chat administrator lookup failed")]
    Failed(#[source] crate::error::BoxError),

    #[error("invalid platform identity for chat administrator lookup: {0}")]
    InvalidIdentity(String),
}

/// A failure while evaluating authorization for a message.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum AuthorizationError {
    #[error("permission denied")]
    Unauthorized,

    #[error(transparent)]
    Admin(#[from] AdminError),

    #[error("chat administrator status could not be verified")]
    ChatAdminUnavailable(#[source] ChatAdminLookupError),

    #[error("chat administrator lookup timed out")]
    ChatAdminTimeout,
}
