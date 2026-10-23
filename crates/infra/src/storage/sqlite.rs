//! SQLite implementation of the [`StateStore`] port.
//!
//! Timestamps are stored as Unix milliseconds (`INTEGER`), which keeps range queries and the
//! expiry index independent of text formatting. The table is `STRICT` so accidental type drift
//! is rejected by SQLite rather than silently coerced.

use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Row, SqlitePool};
use tracing::{debug, info};

use lukosbot_core::StateStoreError;
use lukosbot_core::ports::{
    SCHEMA_VERSION, StateEntry, StateKey, StateScope, StateScopeKind, StateStore, VersionedValue,
};
use lukosbot_domain::{PlatformChatId, PlatformKind, PlatformUserId};

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./migrations");

/// Connection settings for the SQLite state store.
#[derive(Debug, Clone)]
pub struct SqliteSettings {
    pub path: PathBuf,
    pub busy_timeout: Duration,
    pub max_connections: u32,
    pub create_parent_dirs: bool,
}

impl SqliteSettings {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            busy_timeout: Duration::from_secs(5),
            max_connections: 5,
            create_parent_dirs: true,
        }
    }
}

/// SQLite-backed state storage.
#[derive(Debug, Clone)]
pub struct SqliteStateStore {
    pool: SqlitePool,
}

impl SqliteStateStore {
    /// Opens the database, creating parent directories and the file when configured, and applies
    /// all embedded migrations.
    pub async fn connect(settings: &SqliteSettings) -> Result<Self, StateStoreError> {
        if settings.create_parent_dirs {
            create_parent_directory(&settings.path)?;
        }

        let in_memory = settings.path.as_os_str() == ":memory:";
        let options = SqliteConnectOptions::new()
            .filename(&settings.path)
            .create_if_missing(true)
            .foreign_keys(true)
            .busy_timeout(settings.busy_timeout)
            .journal_mode(if in_memory {
                SqliteJournalMode::Memory
            } else {
                SqliteJournalMode::Wal
            })
            .synchronous(SqliteSynchronous::Normal);

        let pool = SqlitePoolOptions::new()
            .max_connections(if in_memory {
                1
            } else {
                settings.max_connections.max(1)
            })
            .acquire_timeout(Duration::from_secs(30))
            .connect_with(options)
            .await
            .map_err(|error| StateStoreError::backend("open SQLite database", error))?;

        let store = Self { pool };
        store.migrate().await?;

        info!(path = %settings.path.display(), "SQLite state store ready");
        Ok(store)
    }

    /// Applies pending migrations. Safe to call repeatedly.
    pub async fn migrate(&self) -> Result<(), StateStoreError> {
        MIGRATOR
            .run(&self.pool)
            .await
            .map_err(|error| StateStoreError::backend("run SQLite migrations", error))?;
        debug!("SQLite migrations are up to date");
        Ok(())
    }

    fn ensure_open(&self) -> Result<(), StateStoreError> {
        if self.pool.is_closed() {
            Err(StateStoreError::Closed)
        } else {
            Ok(())
        }
    }

    /// The importer is an infrastructure-only migration tool and needs a transaction on the same
    /// pool as the store. It is deliberately not exposed through the generic `StateStore` port.
    pub(crate) fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}

fn create_parent_directory(path: &std::path::Path) -> Result<(), StateStoreError> {
    if path.as_os_str() == ":memory:" {
        return Ok(());
    }

    let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    else {
        return Ok(());
    };

    std::fs::create_dir_all(parent)
        .map_err(|error| StateStoreError::backend("create SQLite parent directory", error))
}

#[async_trait]
impl StateStore for SqliteStateStore {
    async fn get(&self, key: &StateKey) -> Result<Option<VersionedValue>, StateStoreError> {
        self.ensure_open()?;

        let row = sqlx::query(
            r#"
            SELECT value_json, schema_version, revision, created_at, updated_at, expires_at
            FROM state
            WHERE scope_type = ?1
              AND scope_platform = ?2
              AND scope_id = ?3
              AND namespace = ?4
              AND "key" = ?5
              AND (expires_at IS NULL OR expires_at > ?6)
            "#,
        )
        .bind(key.scope.scope_type())
        .bind(key.scope.platform().map_or("", PlatformKind::as_str))
        .bind(key.scope.scope_id().unwrap_or(""))
        .bind(&key.namespace)
        .bind(&key.key)
        .bind(Utc::now().timestamp_millis())
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| StateStoreError::backend("read state entry", error))?;

        row.as_ref().map(decode_value).transpose()
    }

    async fn put(
        &self,
        key: StateKey,
        value: Value,
        expires_at: Option<DateTime<Utc>>,
    ) -> Result<(), StateStoreError> {
        self.ensure_open()?;

        let value_json = serde_json::to_string(&value)?;
        let now = Utc::now().timestamp_millis();
        let expires_at = expires_at.map(|time| time.timestamp_millis());

        sqlx::query(
            r#"
            INSERT INTO state (
                scope_type, scope_platform, scope_id, namespace, "key",
                value_json, schema_version, revision, created_at, updated_at, expires_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 0, ?8, ?8, ?9)
            ON CONFLICT (scope_type, scope_platform, scope_id, namespace, "key")
            DO UPDATE SET
                value_json = excluded.value_json,
                schema_version = excluded.schema_version,
                revision = revision + 1,
                updated_at = excluded.updated_at,
                expires_at = excluded.expires_at
            "#,
        )
        .bind(key.scope.scope_type())
        .bind(key.scope.platform().map_or("", PlatformKind::as_str))
        .bind(key.scope.scope_id().unwrap_or(""))
        .bind(&key.namespace)
        .bind(&key.key)
        .bind(value_json)
        .bind(i64::from(SCHEMA_VERSION))
        .bind(now)
        .bind(expires_at)
        .execute(&self.pool)
        .await
        .map_err(|error| StateStoreError::backend("write state entry", error))?;

        Ok(())
    }

    async fn delete(&self, key: &StateKey) -> Result<(), StateStoreError> {
        self.ensure_open()?;

        sqlx::query(
            r#"
            DELETE FROM state
            WHERE scope_type = ?1
              AND scope_platform = ?2
              AND scope_id = ?3
              AND namespace = ?4
              AND "key" = ?5
            "#,
        )
        .bind(key.scope.scope_type())
        .bind(key.scope.platform().map_or("", PlatformKind::as_str))
        .bind(key.scope.scope_id().unwrap_or(""))
        .bind(&key.namespace)
        .bind(&key.key)
        .execute(&self.pool)
        .await
        .map_err(|error| StateStoreError::backend("delete state entry", error))?;

        Ok(())
    }

    async fn list_namespace(
        &self,
        scope: &StateScope,
        namespace: &str,
    ) -> Result<Vec<StateEntry>, StateStoreError> {
        self.ensure_open()?;
        StateKey::validate_namespace(namespace)?;

        let rows = sqlx::query(
            r#"
            SELECT scope_type, scope_platform, scope_id, namespace, "key",
                   value_json, schema_version, revision, created_at, updated_at, expires_at
            FROM state
            WHERE scope_type = ?1
              AND scope_platform = ?2
              AND scope_id = ?3
              AND namespace = ?4
              AND (expires_at IS NULL OR expires_at > ?5)
            ORDER BY "key"
            "#,
        )
        .bind(scope.scope_type())
        .bind(scope.platform().map_or("", PlatformKind::as_str))
        .bind(scope.scope_id().unwrap_or(""))
        .bind(namespace)
        .bind(Utc::now().timestamp_millis())
        .fetch_all(&self.pool)
        .await
        .map_err(|error| StateStoreError::backend("list state namespace", error))?;

        rows.iter().map(decode_entry).collect()
    }

    async fn scan_namespace(
        &self,
        scope_kind: StateScopeKind,
        namespace: &str,
    ) -> Result<Vec<StateEntry>, StateStoreError> {
        self.ensure_open()?;
        StateKey::validate_namespace(namespace)?;

        let rows = sqlx::query(
            r#"
            SELECT scope_type, scope_platform, scope_id, namespace, "key",
                   value_json, schema_version, revision, created_at, updated_at, expires_at
            FROM state
            WHERE scope_type = ?1
              AND namespace = ?2
              AND (expires_at IS NULL OR expires_at > ?3)
            ORDER BY scope_platform, scope_id, "key"
            "#,
        )
        .bind(scope_kind.as_str())
        .bind(namespace)
        .bind(Utc::now().timestamp_millis())
        .fetch_all(&self.pool)
        .await
        .map_err(|error| StateStoreError::backend("scan state namespace", error))?;

        rows.iter().map(decode_entry).collect()
    }

    async fn delete_expired(&self, now: DateTime<Utc>) -> Result<u64, StateStoreError> {
        self.ensure_open()?;

        let result =
            sqlx::query("DELETE FROM state WHERE expires_at IS NOT NULL AND expires_at <= ?1")
                .bind(now.timestamp_millis())
                .execute(&self.pool)
                .await
                .map_err(|error| StateStoreError::backend("delete expired state entries", error))?;

        Ok(result.rows_affected())
    }

    async fn close(&self) -> Result<(), StateStoreError> {
        self.pool.close().await;
        Ok(())
    }
}

fn decode_entry(row: &sqlx::sqlite::SqliteRow) -> Result<StateEntry, StateStoreError> {
    Ok(StateEntry {
        key: decode_key(row)?,
        value: decode_value(row)?,
    })
}

fn decode_key(row: &sqlx::sqlite::SqliteRow) -> Result<StateKey, StateStoreError> {
    let scope_type: String = row
        .try_get("scope_type")
        .map_err(|error| StateStoreError::backend("decode state scope type", error))?;
    let scope_platform: String = row
        .try_get("scope_platform")
        .map_err(|error| StateStoreError::backend("decode state platform", error))?;
    let scope_id: String = row
        .try_get("scope_id")
        .map_err(|error| StateStoreError::backend("decode state scope id", error))?;
    let namespace: String = row
        .try_get("namespace")
        .map_err(|error| StateStoreError::backend("decode state namespace", error))?;
    let key: String = row
        .try_get("key")
        .map_err(|error| StateStoreError::backend("decode state key", error))?;

    let scope_kind = StateScopeKind::from_str(&scope_type)
        .map_err(|error| StateStoreError::backend("decode state scope", error))?;

    let scope = match scope_kind {
        StateScopeKind::Global => StateScope::Global,
        StateScopeKind::Chat => StateScope::Chat {
            platform: decode_platform(&scope_platform)?,
            chat_id: PlatformChatId::new(scope_id)
                .map_err(|error| StateStoreError::backend("decode chat id", error))?,
        },
        StateScopeKind::User => StateScope::User {
            platform: decode_platform(&scope_platform)?,
            user_id: PlatformUserId::new(scope_id)
                .map_err(|error| StateStoreError::backend("decode user id", error))?,
        },
    };

    StateKey::new(scope, namespace, key)
}

fn decode_platform(value: &str) -> Result<PlatformKind, StateStoreError> {
    PlatformKind::from_str(value)
        .map_err(|error| StateStoreError::backend("decode state platform", error))
}

fn decode_value(row: &sqlx::sqlite::SqliteRow) -> Result<VersionedValue, StateStoreError> {
    let value_json: String = row
        .try_get("value_json")
        .map_err(|error| StateStoreError::backend("decode state value", error))?;
    let value = serde_json::from_str(&value_json)?;

    let schema_version: i64 = row
        .try_get("schema_version")
        .map_err(|error| StateStoreError::backend("decode schema version", error))?;
    let schema_version = u32::try_from(schema_version)
        .map_err(|error| StateStoreError::backend("decode schema version", error))?;

    let revision: i64 = row
        .try_get("revision")
        .map_err(|error| StateStoreError::backend("decode state revision", error))?;
    let revision = u64::try_from(revision)
        .map_err(|error| StateStoreError::backend("decode state revision", error))?;

    let created_at_ms: i64 = row
        .try_get("created_at")
        .map_err(|error| StateStoreError::backend("decode created_at", error))?;
    let updated_at_ms: i64 = row
        .try_get("updated_at")
        .map_err(|error| StateStoreError::backend("decode state timestamp", error))?;
    let expires_at_ms: Option<i64> = row
        .try_get("expires_at")
        .map_err(|error| StateStoreError::backend("decode state expiry", error))?;

    Ok(VersionedValue {
        value,
        schema_version,
        revision,
        created_at: timestamp_from_millis(created_at_ms)?,
        updated_at: timestamp_from_millis(updated_at_ms)?,
        expires_at: expires_at_ms.map(timestamp_from_millis).transpose()?,
    })
}

fn timestamp_from_millis(millis: i64) -> Result<DateTime<Utc>, StateStoreError> {
    DateTime::from_timestamp_millis(millis).ok_or_else(|| {
        StateStoreError::backend(
            "decode timestamp",
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("timestamp {millis} is out of range"),
            ),
        )
    })
}
