//! SQLite implementation of the [`StateStore`] port.
//!
//! Timestamps are stored as Unix milliseconds (`INTEGER`), which keeps range queries and the
//! expiry index independent of text formatting. The table is `STRICT` so accidental type drift
//! is rejected by SQLite rather than silently coerced.

use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::{Row, SqlitePool};
use tracing::{debug, info};

use lukosbot_core::StateStoreError;
use lukosbot_core::ports::{StateKey, StateStore, VALUE_VERSION, VersionedValue};

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
            SELECT value_json, version, updated_at, expires_at
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
        .bind(
            key.scope
                .platform()
                .map_or("", |platform| platform.as_str()),
        )
        .bind(key.scope.scope_id().unwrap_or(""))
        .bind(&key.namespace)
        .bind(&key.key)
        .bind(Utc::now().timestamp_millis())
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| StateStoreError::backend("read state entry", error))?;

        let Some(row) = row else {
            return Ok(None);
        };

        let value_json: String = row
            .try_get("value_json")
            .map_err(|error| StateStoreError::backend("decode state value", error))?;
        let value = serde_json::from_str(&value_json)?;

        let version: i64 = row
            .try_get("version")
            .map_err(|error| StateStoreError::backend("decode state version", error))?;
        let version = u32::try_from(version)
            .map_err(|error| StateStoreError::backend("decode state version", error))?;

        let updated_at_ms: i64 = row
            .try_get("updated_at")
            .map_err(|error| StateStoreError::backend("decode state timestamp", error))?;
        let expires_at_ms: Option<i64> = row
            .try_get("expires_at")
            .map_err(|error| StateStoreError::backend("decode state expiry", error))?;

        Ok(Some(VersionedValue {
            value,
            version,
            updated_at: timestamp_from_millis(updated_at_ms)?,
            expires_at: expires_at_ms.map(timestamp_from_millis).transpose()?,
        }))
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
                value_json, version, created_at, updated_at, expires_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8, ?9)
            ON CONFLICT (scope_type, scope_platform, scope_id, namespace, "key")
            DO UPDATE SET
                value_json = excluded.value_json,
                version = excluded.version,
                updated_at = excluded.updated_at,
                expires_at = excluded.expires_at
            "#,
        )
        .bind(key.scope.scope_type())
        .bind(
            key.scope
                .platform()
                .map_or("", |platform| platform.as_str()),
        )
        .bind(key.scope.scope_id().unwrap_or(""))
        .bind(&key.namespace)
        .bind(&key.key)
        .bind(value_json)
        .bind(i64::from(VALUE_VERSION))
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
        .bind(
            key.scope
                .platform()
                .map_or("", |platform| platform.as_str()),
        )
        .bind(key.scope.scope_id().unwrap_or(""))
        .bind(&key.namespace)
        .bind(&key.key)
        .execute(&self.pool)
        .await
        .map_err(|error| StateStoreError::backend("delete state entry", error))?;

        Ok(())
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
