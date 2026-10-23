//! The generic persistent state port.
//!
//! The port stores opaque JSON values under a scoped, namespaced key. Domain-specific semantics
//! (defaults, precedence, permissions) belong to [`crate::state`] and are built on top of this
//! boundary.

use std::fmt;
use std::str::FromStr;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;

use lukosbot_domain::{ChatAddress, PlatformChatId, PlatformKind, PlatformUserId};

use crate::error::BoxError;

/// Version of the JSON envelope written by the current implementation.
pub const SCHEMA_VERSION: u32 = 1;

/// The category of a [`StateScope`].
///
/// Kept separate from the concrete scope so storage implementations and maintenance code can
/// express "all chat state" without inventing sentinel platform or chat identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum StateScopeKind {
    Global,
    Chat,
    User,
}

impl StateScopeKind {
    /// Stable storage discriminator. Kept in `core` so adapters never invent their own encoding.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Chat => "chat",
            Self::User => "user",
        }
    }
}

impl fmt::Display for StateScopeKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Returned when a persisted scope kind is not recognised.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown state scope kind `{0}`")]
pub struct ParseStateScopeKindError(pub String);

impl FromStr for StateScopeKind {
    type Err = ParseStateScopeKindError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "global" => Ok(Self::Global),
            "chat" => Ok(Self::Chat),
            "user" => Ok(Self::User),
            _ => Err(ParseStateScopeKindError(value.to_owned())),
        }
    }
}

/// The scope a state entry belongs to.
///
/// A chat's `ChatKind` is descriptive metadata and intentionally not part of the scope: changing
/// how a chat is classified must not orphan previously stored state.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
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

    /// Returns the category of this scope.
    pub const fn kind(&self) -> StateScopeKind {
        match self {
            Self::Global => StateScopeKind::Global,
            Self::Chat { .. } => StateScopeKind::Chat,
            Self::User { .. } => StateScopeKind::User,
        }
    }

    /// Stable storage discriminator. Kept in `core` so adapters never invent their own encoding.
    pub fn scope_type(&self) -> &'static str {
        self.kind().as_str()
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
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
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

    /// Validates a namespace before a bulk query.
    ///
    /// Bulk query methods take a bare namespace rather than a [`StateKey`], so they need the same
    /// rules without fabricating a dummy key.
    pub fn validate_namespace(namespace: &str) -> Result<(), StateStoreError> {
        validate_component("namespace", namespace)
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

/// A key/value pair returned by a namespace query.
#[derive(Debug, Clone, PartialEq)]
pub struct StateEntry {
    pub key: StateKey,
    pub value: VersionedValue,
}

/// A stored value together with its envelope metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct VersionedValue {
    /// Version of the value format, not a record revision.
    pub schema_version: u32,
    /// Number of updates applied to this logical key.
    pub revision: u64,
    pub value: Value,
    pub created_at: DateTime<Utc>,
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

    /// Returns all unexpired entries in an exact scope and namespace, ordered by key.
    async fn list_namespace(
        &self,
        scope: &StateScope,
        namespace: &str,
    ) -> Result<Vec<StateEntry>, StateStoreError>;

    /// Returns all unexpired entries of a scope kind and namespace, ordered by platform, scope id,
    /// and key.
    async fn scan_namespace(
        &self,
        scope_kind: StateScopeKind,
        namespace: &str,
    ) -> Result<Vec<StateEntry>, StateStoreError>;

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
        assert_eq!(StateScope::chat(&private).kind(), StateScopeKind::Chat);
    }

    #[test]
    fn scope_kind_round_trips_through_storage_strings() {
        for kind in [
            StateScopeKind::Global,
            StateScopeKind::Chat,
            StateScopeKind::User,
        ] {
            assert_eq!(kind.as_str().parse::<StateScopeKind>().unwrap(), kind);
        }
        assert!("nope".parse::<StateScopeKind>().is_err());
    }
}
