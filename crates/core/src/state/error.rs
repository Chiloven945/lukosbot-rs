use thiserror::Error;

use crate::ports::{StateScopeKind, StateStoreError};

/// A malformed state definition or value.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum StateDefinitionError {
    #[error("invalid state definition: {0}")]
    InvalidDefinition(String),

    #[error("invalid value: {0}")]
    Parse(String),

    #[error("value is not valid: {0}")]
    Validation(String),

    #[error("state value codec failure")]
    Codec(#[from] serde_json::Error),
}

impl StateDefinitionError {
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::InvalidDefinition(message.into())
    }

    pub fn parse(message: impl Into<String>) -> Self {
        Self::Parse(message.into())
    }

    pub fn validation(message: impl Into<String>) -> Self {
        Self::Validation(message.into())
    }
}

/// A state definition registration failure.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum StateRegistryError {
    #[error("state `{name}` is already registered")]
    DuplicateName { name: String },

    #[error("state definition `{name}` is invalid: {message}")]
    InvalidDefinition { name: String, message: String },
}

/// A failure while resolving or mutating state.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum StateServiceError {
    #[error("state definition `{state}` is invalid")]
    Definition {
        state: String,
        #[source]
        source: StateDefinitionError,
    },

    #[error("state `{state}` does not support the {scope} scope")]
    UnsupportedScope {
        state: String,
        scope: StateScopeKind,
    },

    #[error("state `{state}` value could not be parsed")]
    Parse {
        state: String,
        #[source]
        source: StateDefinitionError,
    },

    #[error("state `{state}` value is invalid")]
    Validation {
        state: String,
        #[source]
        source: StateDefinitionError,
    },

    #[error("stored state `{state}` in {scope} scope is invalid")]
    InvalidStoredValue {
        state: String,
        scope: StateScopeKind,
    },

    #[error(transparent)]
    Storage(#[from] StateStoreError),
}
