//! The generic persistent state port.
//!
//! The port is deliberately small: it stores opaque JSON values under a scoped, namespaced key.
//! Domain-specific state semantics (defaults, precedence, permissions) belong to later feature
//! plans and are built on top of this boundary.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;

use lukosbot_domain::{ChatAddress, PlatformChatId, PlatformKind, PlatformUserId};

use crate::error::BoxError;

/// Envelope version written by the current implementation.
pub const VALUE_VERSION: u32 = 1;

/// The scope a state entry belongs to.
///
/// A chat's `ChatKind` is descriptive metadata and intentionally not part of the scope: changing
/// how a chat is classified must not orphan previously stored state.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum StateScope {
    Global,
    Chat {
        platform: PlatformKind,
        chat_id: PlatformChatId,
    },
    User {
        platform: PlatformKind,
        user_id: PlatformUserId,
    },
}

impl StateScope {
    pub fn chat(address: &ChatAddress) -> Self {
        Self::Chat {
            platform: address.platform,
            chat_id: address.chat_id.clone(),
        }
    }

    pub fn user(platform: PlatformKind, user_id: PlatformUserId) -> Self {
        Self::User { platform, user_id }
    }

    /// Stable storage discriminator. Kept in `core` so adapters never invent their own encoding.
    pub fn scope_type(&self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Chat { .. } => "chat",
            Self::User { .. } => "user",
        }
    }

    pub fn platform(&self) -> Option<PlatformKind> {
        match self {
            Self::Global => None,
            Self::Chat { platform, .. } | Self::User { platform, .. } => Some(*platform),
        }
    }

    pub fn scope_id(&self) -> Option<&str> {
        match self {
            Self::Global => None,
            Self::Chat { chat_id, .. } => Some(chat_id.as_str()),
            Self::User { user_id, .. } => Some(user_id.as_str()),
        }
    }
}

/// A fully qualified state key.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StateKey {
    pub scope: StateScope,
    pub namespace: String,
    pub key: String,
}

impl StateKey {
    pub fn new(
        scope: StateScope,
        namespace: impl Into<String>,
        key: impl Into<String>,
    ) -> Result<Self, StateStoreError> {
        let namespace = namespace.into();
        let key = key.into();
        validate_component("namespace", &namespace)?;
        validate_component("key", &key)?;
        Ok(Self {
            scope,
            namespace,
            key,
        })
    }

    pub fn global(
        namespace: impl Into<String>,
        key: impl Into<String>,
    ) -> Result<Self, StateStoreError> {
        Self::new(StateScope::Global, namespace, key)
    }
}

fn validate_component(label: &str, value: &str) -> Result<(), StateStoreError> {
    if value.is_empty() {
        return Err(StateStoreError::InvalidKey(format!(
            "{label} must not be empty"
        )));
    }
    if value.len() > 128 {
        return Err(StateStoreError::InvalidKey(format!(
            "{label} must not exceed 128 bytes"
        )));
    }
    if value.chars().any(char::is_control) {
        return Err(StateStoreError::InvalidKey(format!(
            "{label} must not contain control characters"
        )));
    }
    Ok(())
}

/// A stored value together with its envelope metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct VersionedValue {
    pub value: Value,
    pub version: u32,
    pub updated_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
}

/// Persistent state storage.
#[async_trait]
pub trait StateStore: Send + Sync {
    async fn get(&self, key: &StateKey) -> Result<Option<VersionedValue>, StateStoreError>;

    async fn put(
        &self,
        key: StateKey,
        value: Value,
        expires_at: Option<DateTime<Utc>>,
    ) -> Result<(), StateStoreError>;

    async fn delete(&self, key: &StateKey) -> Result<(), StateStoreError>;

    async fn delete_expired(&self, now: DateTime<Utc>) -> Result<u64, StateStoreError>;

    /// Releases backend resources. Implementations must tolerate repeated calls.
    async fn close(&self) -> Result<(), StateStoreError> {
        Ok(())
    }
}

/// A state storage failure.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum StateStoreError {
    #[error("state store backend failure: {context}")]
    Backend {
        context: &'static str,
        #[source]
        source: BoxError,
    },

    #[error("state key is invalid: {0}")]
    InvalidKey(String),

    #[error("state value could not be encoded or decoded")]
    Codec(#[from] serde_json::Error),

    #[error("the state store has been closed")]
    Closed,
}

impl StateStoreError {
    pub fn backend(context: &'static str, source: impl Into<BoxError>) -> Self {
        Self::Backend {
            context,
            source: source.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_key_components() {
        assert!(StateKey::new(StateScope::Global, "", "value").is_err());
        assert!(StateKey::new(StateScope::Global, "ns", "").is_err());
    }

    #[test]
    fn rejects_control_characters() {
        assert!(StateKey::new(StateScope::Global, "ns", "a\nb").is_err());
    }

    #[test]
    fn chat_scope_uses_platform_and_chat_id_only() {
        use lukosbot_domain::{ChatAddress, ChatKind};

        let private = ChatAddress::new(
            PlatformKind::Telegram,
            PlatformChatId::new("7").unwrap(),
            ChatKind::Private,
        );
        let group = ChatAddress::new(
            PlatformKind::Telegram,
            PlatformChatId::new("7").unwrap(),
            ChatKind::Group,
        );

        assert_eq!(StateScope::chat(&private), StateScope::chat(&group));
    }
}
