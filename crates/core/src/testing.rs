//! In-memory test doubles for core runtime tests.
//!
//! Enabled by the `test-util` feature. These types are also used by application-level
//! integration tests and are intentionally kept free of external services.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use serde_json::Value;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

use lukosbot_domain::{
    ChatAddress, InboundMessage, OutboundMessage, PlatformKind, PlatformMessageId, SentMessage,
};

use crate::ports::{
    InboundSender, PlatformAdapter, PlatformError, SCHEMA_VERSION, StateEntry, StateKey,
    StateScope, StateScopeKind, StateStore, StateStoreError, VersionedValue,
};

/// Behaviour injected into a [`RecordingAdapter`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AdapterBehavior {
    #[default]
    Normal,
    FailSend,
    DelaySend(Duration),
    PanicOnRun,
    ExitImmediately,
    HangOnShutdown,
}

/// An adapter that records outbound messages and can simulate failures and delays.
#[derive(Debug)]
pub struct RecordingAdapter {
    kind: PlatformKind,
    behavior: AdapterBehavior,
    inbound: std::sync::Mutex<Vec<InboundMessage>>,
    sent: std::sync::Mutex<Vec<SentRecord>>,
    shutdowns: AtomicUsize,
    next_id: AtomicU64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SentRecord {
    pub address: ChatAddress,
    pub message: OutboundMessage,
}

impl RecordingAdapter {
    pub fn new(kind: PlatformKind) -> Arc<Self> {
        Self::with_behavior(kind, AdapterBehavior::Normal)
    }

    pub fn with_behavior(kind: PlatformKind, behavior: AdapterBehavior) -> Arc<Self> {
        Arc::new(Self {
            kind,
            behavior,
            inbound: std::sync::Mutex::new(Vec::new()),
            sent: std::sync::Mutex::new(Vec::new()),
            shutdowns: AtomicUsize::new(0),
            next_id: AtomicU64::new(1),
        })
    }

    /// Queues a message that will be emitted as soon as the adapter's `run` starts.
    pub fn push_inbound(&self, message: InboundMessage) {
        self.inbound
            .lock()
            .expect("recording adapter lock")
            .push(message);
    }

    pub fn sent(&self) -> Vec<SentRecord> {
        self.sent.lock().expect("recording adapter lock").clone()
    }

    pub fn shutdown_count(&self) -> usize {
        self.shutdowns.load(Ordering::Relaxed)
    }

    /// Waits until at least `count` outbound messages have been recorded.
    pub async fn wait_for_sends(&self, count: usize) {
        wait_until(|| self.sent().len() >= count).await;
    }
}

#[async_trait]
impl PlatformAdapter for RecordingAdapter {
    fn kind(&self) -> PlatformKind {
        self.kind
    }

    async fn run(
        &self,
        inbound: InboundSender,
        cancel: CancellationToken,
    ) -> Result<(), PlatformError> {
        match self.behavior {
            AdapterBehavior::PanicOnRun => panic!("recording adapter panic"),
            AdapterBehavior::ExitImmediately => return Ok(()),
            _ => {}
        }

        let pending = {
            let mut queue = self.inbound.lock().expect("recording adapter lock");
            std::mem::take(&mut *queue)
        };

        for message in pending {
            if inbound.send(message).await.is_err() {
                return Err(PlatformError::Disconnected {
                    platform: self.kind,
                });
            }
        }

        cancel.cancelled().await;
        Ok(())
    }

    async fn send(
        &self,
        target: &ChatAddress,
        message: OutboundMessage,
    ) -> Result<SentMessage, PlatformError> {
        if target.platform != self.kind {
            return Err(PlatformError::WrongPlatform {
                target: target.platform,
                adapter: self.kind,
            });
        }

        match self.behavior {
            AdapterBehavior::FailSend => {
                return Err(PlatformError::Api {
                    platform: self.kind,
                    message: "injected send failure".to_owned(),
                });
            }
            AdapterBehavior::DelaySend(delay) => tokio::time::sleep(delay).await,
            _ => {}
        }

        self.sent
            .lock()
            .expect("recording adapter lock")
            .push(SentRecord {
                address: target.clone(),
                message,
            });

        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let message_id = PlatformMessageId::new(format!("recording-{id}"))
            .expect("generated message id is valid");

        Ok(SentMessage {
            platform: self.kind,
            message_id,
        })
    }

    async fn shutdown(&self) -> Result<(), PlatformError> {
        self.shutdowns.fetch_add(1, Ordering::Relaxed);
        if self.behavior == AdapterBehavior::HangOnShutdown {
            std::future::pending::<()>().await;
        }
        Ok(())
    }
}

/// Polls a predicate until it is true or the deadline expires.
pub async fn wait_until(mut predicate: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while !predicate() {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .expect("condition was not met before the deadline");
}

/// An in-memory [`StateStore`] with the same expiry semantics as the SQLite implementation.
#[derive(Debug, Clone, Default)]
pub struct MemoryStateStore {
    entries: Arc<RwLock<HashMap<StateKey, VersionedValue>>>,
}

impl MemoryStateStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn len(&self) -> usize {
        self.entries.read().await.len()
    }

    pub async fn is_empty(&self) -> bool {
        self.len().await == 0
    }
}

#[async_trait]
impl StateStore for MemoryStateStore {
    async fn get(&self, key: &StateKey) -> Result<Option<VersionedValue>, StateStoreError> {
        let now = Utc::now();
        let entries = self.entries.read().await;
        let value = entries
            .get(key)
            .filter(|value| value.expires_at.is_none_or(|expires_at| expires_at > now));
        Ok(value.cloned())
    }

    async fn put(
        &self,
        key: StateKey,
        value: Value,
        expires_at: Option<chrono::DateTime<Utc>>,
    ) -> Result<(), StateStoreError> {
        let now = Utc::now();
        let mut entries = self.entries.write().await;
        match entries.entry(key) {
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                let existing = entry.get_mut();
                existing.value = value;
                existing.schema_version = SCHEMA_VERSION;
                existing.revision = existing.revision.saturating_add(1);
                existing.updated_at = now;
                existing.expires_at = expires_at;
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(VersionedValue {
                    value,
                    schema_version: SCHEMA_VERSION,
                    revision: 0,
                    created_at: now,
                    updated_at: now,
                    expires_at,
                });
            }
        }
        Ok(())
    }

    async fn delete(&self, key: &StateKey) -> Result<(), StateStoreError> {
        self.entries.write().await.remove(key);
        Ok(())
    }

    async fn list_namespace(
        &self,
        scope: &StateScope,
        namespace: &str,
    ) -> Result<Vec<StateEntry>, StateStoreError> {
        StateKey::validate_namespace(namespace)?;
        let now = Utc::now();
        let entries = self.entries.read().await;
        let mut values: Vec<StateEntry> = entries
            .iter()
            .filter(|(key, value)| {
                &key.scope == scope
                    && key.namespace == namespace
                    && value.expires_at.is_none_or(|expires_at| expires_at > now)
            })
            .map(|(key, value)| StateEntry {
                key: key.clone(),
                value: value.clone(),
            })
            .collect();
        values.sort_by(|left, right| left.key.key.cmp(&right.key.key));
        Ok(values)
    }

    async fn scan_namespace(
        &self,
        scope_kind: StateScopeKind,
        namespace: &str,
    ) -> Result<Vec<StateEntry>, StateStoreError> {
        StateKey::validate_namespace(namespace)?;
        let now = Utc::now();
        let entries = self.entries.read().await;
        let mut values: Vec<StateEntry> = entries
            .iter()
            .filter(|(key, value)| {
                key.scope.kind() == scope_kind
                    && key.namespace == namespace
                    && value.expires_at.is_none_or(|expires_at| expires_at > now)
            })
            .map(|(key, value)| StateEntry {
                key: key.clone(),
                value: value.clone(),
            })
            .collect();
        values.sort_by(|left, right| {
            let left_platform = left.key.scope.platform().map_or("", PlatformKind::as_str);
            let right_platform = right.key.scope.platform().map_or("", PlatformKind::as_str);
            left_platform
                .cmp(right_platform)
                .then_with(|| {
                    left.key
                        .scope
                        .scope_id()
                        .unwrap_or("")
                        .cmp(right.key.scope.scope_id().unwrap_or(""))
                })
                .then_with(|| left.key.key.cmp(&right.key.key))
        });
        Ok(values)
    }

    async fn delete_expired(&self, now: chrono::DateTime<Utc>) -> Result<u64, StateStoreError> {
        let mut entries = self.entries.write().await;
        let before = entries.len();
        entries.retain(|_, value| value.expires_at.is_none_or(|expires_at| expires_at > now));
        Ok((before - entries.len()) as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::StateScope;
    use chrono::Duration;

    #[tokio::test]
    async fn memory_store_filters_expired_values() {
        let store = MemoryStateStore::new();
        let key = StateKey::new(StateScope::Global, "test", "expired").unwrap();

        store
            .put(
                key.clone(),
                Value::from(1),
                Some(Utc::now() - Duration::seconds(1)),
            )
            .await
            .unwrap();

        assert!(store.get(&key).await.unwrap().is_none());
        assert_eq!(store.delete_expired(Utc::now()).await.unwrap(), 1);
        assert!(store.is_empty().await);
    }

    #[tokio::test]
    async fn memory_store_lists_and_scans_namespaces() {
        let store = MemoryStateStore::new();
        let chat_scope = StateScope::Chat {
            platform: PlatformKind::Telegram,
            chat_id: lukosbot_domain::PlatformChatId::new("100").unwrap(),
        };
        let user_scope = StateScope::user(
            PlatformKind::Telegram,
            lukosbot_domain::PlatformUserId::new("7").unwrap(),
        );

        store
            .put(
                StateKey::new(chat_scope.clone(), "service", "zeta").unwrap(),
                Value::from(1),
                None,
            )
            .await
            .unwrap();
        store
            .put(
                StateKey::new(chat_scope.clone(), "service", "alpha").unwrap(),
                Value::from(2),
                None,
            )
            .await
            .unwrap();
        store
            .put(
                StateKey::new(user_scope, "service", "user").unwrap(),
                Value::from(3),
                None,
            )
            .await
            .unwrap();

        let listed = store.list_namespace(&chat_scope, "service").await.unwrap();
        assert_eq!(
            listed
                .iter()
                .map(|entry| entry.key.key.as_str())
                .collect::<Vec<_>>(),
            vec!["alpha", "zeta"]
        );

        let scanned = store
            .scan_namespace(StateScopeKind::Chat, "service")
            .await
            .unwrap();
        assert_eq!(scanned.len(), 2);
        assert!(
            scanned
                .iter()
                .all(|entry| entry.key.scope.kind() == StateScopeKind::Chat)
        );
    }

    #[tokio::test]
    async fn memory_store_round_trips_values() {
        let store = MemoryStateStore::new();
        let key = StateKey::new(StateScope::Global, "test", "value").unwrap();

        store
            .put(key.clone(), Value::from("v"), None)
            .await
            .unwrap();
        let stored = store.get(&key).await.unwrap().unwrap();

        assert_eq!(stored.value, Value::from("v"));
        assert_eq!(stored.schema_version, SCHEMA_VERSION);
        assert_eq!(stored.revision, 0);

        store
            .put(key.clone(), Value::from("w"), None)
            .await
            .unwrap();
        let updated = store.get(&key).await.unwrap().unwrap();
        assert_eq!(updated.value, Value::from("w"));
        assert_eq!(updated.revision, 1);
        assert_eq!(updated.created_at, stored.created_at);

        store.delete(&key).await.unwrap();
        assert!(store.get(&key).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn recording_adapter_rejects_wrong_platform() {
        let adapter = RecordingAdapter::new(PlatformKind::Telegram);

        fn assert_adapter<T: PlatformAdapter>(_: &T) {}
        assert_adapter(adapter.as_ref());

        let address = ChatAddress::new(
            PlatformKind::Discord,
            lukosbot_domain::PlatformChatId::new("1").unwrap(),
            lukosbot_domain::ChatKind::Private,
        );
        let error = adapter
            .send(&address, OutboundMessage::text("hi"))
            .await
            .unwrap_err();

        assert!(matches!(
            error,
            PlatformError::WrongPlatform {
                target: PlatformKind::Discord,
                adapter: PlatformKind::Telegram
            }
        ));
    }
}
