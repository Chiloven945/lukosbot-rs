use std::time::Duration;

use chrono::{TimeZone, Utc};
use lukosbot_core::StateStore;
use lukosbot_core::ports::{StateKey, StateScope, StateStoreError};
use lukosbot_domain::{ChatAddress, ChatKind, PlatformChatId, PlatformKind, PlatformUserId};
use lukosbot_infra::storage::{SqliteSettings, SqliteStateStore};
use serde_json::json;
use sqlx::SqlitePool;
use sqlx::sqlite::SqliteConnectOptions;
use tempfile::TempDir;

fn settings(dir: &TempDir) -> SqliteSettings {
    SqliteSettings::new(dir.path().join("state.sqlite3"))
}

async fn open(dir: &TempDir) -> SqliteStateStore {
    SqliteStateStore::connect(&settings(dir))
        .await
        .expect("SQLite store opens")
}

#[tokio::test]
async fn upgrades_a_legacy_0001_database_without_losing_data() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy.sqlite3");
    let pool = SqlitePool::connect_with(
        SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true),
    )
    .await
    .unwrap();

    sqlx::raw_sql(
        r#"
        CREATE TABLE state (
            scope_type     TEXT    NOT NULL CHECK (scope_type IN ('global', 'chat', 'user')),
            scope_platform TEXT    NOT NULL DEFAULT '',
            scope_id       TEXT    NOT NULL DEFAULT '',
            namespace      TEXT    NOT NULL,
            "key"          TEXT    NOT NULL,
            value_json     TEXT    NOT NULL,
            version        INTEGER NOT NULL,
            created_at     INTEGER NOT NULL,
            updated_at     INTEGER NOT NULL,
            expires_at     INTEGER,
            PRIMARY KEY (scope_type, scope_platform, scope_id, namespace, "key")
        ) STRICT;

        INSERT INTO state (
            scope_type, scope_platform, scope_id, namespace, "key",
            value_json, version, created_at, updated_at, expires_at
        ) VALUES (
            'global', '', '', 'auth', 'bot_admins',
            '{"TELEGRAM":[7]}', 1, 1000, 2000, NULL
        );
        "#,
    )
    .execute(&pool)
    .await
    .unwrap();
    pool.close().await;

    let store = SqliteStateStore::connect(&SqliteSettings {
        path,
        ..SqliteSettings::new(dir.path().join("unused"))
    })
    .await
    .unwrap();

    let value = store
        .get(&StateKey::global("auth", "bot_admins").unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(value.value, json!({"TELEGRAM": [7]}));
    assert_eq!(value.schema_version, 1);
    assert_eq!(value.revision, 0);
    assert_eq!(value.created_at.timestamp_millis(), 1_000);
    assert_eq!(value.updated_at.timestamp_millis(), 2_000);
}

#[tokio::test]
async fn migrations_are_idempotent_and_data_survives_reconnect() {
    let dir = tempfile::tempdir().unwrap();
    let key = StateKey::global("test", "persistent").unwrap();

    {
        let store = open(&dir).await;
        store.put(key.clone(), json!({"n": 1}), None).await.unwrap();
        store.close().await.unwrap();
    }

    let store = open(&dir).await;
    let value = store.get(&key).await.unwrap().expect("value survives");
    assert_eq!(value.value, json!({"n": 1}));
}

#[tokio::test]
async fn round_trips_values_and_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(&dir).await;
    let key = StateKey::global("prefs", "language").unwrap();

    store.put(key.clone(), json!("zh-CN"), None).await.unwrap();

    let stored = store.get(&key).await.unwrap().expect("value exists");
    assert_eq!(stored.value, json!("zh-CN"));
    assert_eq!(stored.schema_version, 1);
    assert_eq!(stored.revision, 0);
    assert!(stored.expires_at.is_none());
    assert!(
        (Utc::now() - stored.updated_at).num_seconds() < 5,
        "updated_at should be close to now"
    );

    store.put(key.clone(), json!("en-US"), None).await.unwrap();
    let updated = store.get(&key).await.unwrap().unwrap();
    assert_eq!(updated.value, json!("en-US"));
    assert_eq!(updated.revision, 1);
    assert_eq!(updated.created_at, stored.created_at);

    store.delete(&key).await.unwrap();
    assert!(store.get(&key).await.unwrap().is_none());

    store.close().await.unwrap();
}

#[tokio::test]
async fn scopes_are_isolated() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(&dir).await;

    let telegram_chat = ChatAddress::new(
        PlatformKind::Telegram,
        PlatformChatId::new("42").unwrap(),
        ChatKind::Group,
    );
    let discord_chat = ChatAddress::new(
        PlatformKind::Discord,
        PlatformChatId::new("42").unwrap(),
        ChatKind::Group,
    );

    let telegram_key = StateKey::new(StateScope::chat(&telegram_chat), "settings", "mode").unwrap();
    let discord_key = StateKey::new(StateScope::chat(&discord_chat), "settings", "mode").unwrap();
    let user_key = StateKey::new(
        StateScope::user(PlatformKind::Telegram, PlatformUserId::new("42").unwrap()),
        "settings",
        "mode",
    )
    .unwrap();

    store
        .put(telegram_key.clone(), json!("tg"), None)
        .await
        .unwrap();
    store
        .put(discord_key.clone(), json!("dc"), None)
        .await
        .unwrap();
    store
        .put(user_key.clone(), json!("user"), None)
        .await
        .unwrap();

    assert_eq!(
        store.get(&telegram_key).await.unwrap().unwrap().value,
        json!("tg")
    );
    assert_eq!(
        store.get(&discord_key).await.unwrap().unwrap().value,
        json!("dc")
    );
    assert_eq!(
        store.get(&user_key).await.unwrap().unwrap().value,
        json!("user")
    );

    store.close().await.unwrap();
}

#[tokio::test]
async fn namespace_queries_filter_scope_expiry_and_order_stably() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(&dir).await;

    let chat = StateScope::Chat {
        platform: PlatformKind::Telegram,
        chat_id: PlatformChatId::new("-100").unwrap(),
    };
    let user = StateScope::User {
        platform: PlatformKind::Telegram,
        user_id: PlatformUserId::new("7").unwrap(),
    };
    let discord_chat = StateScope::Chat {
        platform: PlatformKind::Discord,
        chat_id: PlatformChatId::new("42").unwrap(),
    };

    let keys = [
        StateKey::new(chat.clone(), "service", "zeta").unwrap(),
        StateKey::new(chat.clone(), "service", "alpha").unwrap(),
        StateKey::new(discord_chat.clone(), "service", "middle").unwrap(),
        StateKey::new(user.clone(), "service", "user").unwrap(),
        StateKey::new(StateScope::Global, "service", "global").unwrap(),
    ];
    for (index, key) in keys.iter().enumerate() {
        store.put(key.clone(), json!(index), None).await.unwrap();
    }
    store
        .put(
            StateKey::new(chat.clone(), "service", "expired").unwrap(),
            json!(true),
            Some(Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap()),
        )
        .await
        .unwrap();

    let listed = store.list_namespace(&chat, "service").await.unwrap();
    assert_eq!(
        listed
            .iter()
            .map(|entry| entry.key.key.as_str())
            .collect::<Vec<_>>(),
        vec!["alpha", "zeta"]
    );

    let scanned = store
        .scan_namespace(lukosbot_core::StateScopeKind::Chat, "service")
        .await
        .unwrap();
    assert_eq!(
        scanned
            .iter()
            .map(|entry| (
                entry.key.scope.platform(),
                entry.key.scope.scope_id(),
                entry.key.key.as_str()
            ))
            .collect::<Vec<_>>(),
        vec![
            (Some(PlatformKind::Discord), Some("42"), "middle"),
            (Some(PlatformKind::Telegram), Some("-100"), "alpha"),
            (Some(PlatformKind::Telegram), Some("-100"), "zeta"),
        ]
    );

    assert!(
        store
            .list_namespace(&user, "missing")
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn expired_entries_are_hidden_and_deleted() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(&dir).await;
    let key = StateKey::global("cache", "short-lived").unwrap();

    let past = Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap();
    store
        .put(key.clone(), json!("value"), Some(past))
        .await
        .unwrap();

    assert!(store.get(&key).await.unwrap().is_none());
    assert_eq!(store.delete_expired(Utc::now()).await.unwrap(), 1);
    assert_eq!(store.delete_expired(Utc::now()).await.unwrap(), 0);

    store.close().await.unwrap();
}

#[tokio::test]
async fn special_characters_in_keys_are_stored_safely() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(&dir).await;
    let key = StateKey::new(
        StateScope::Global,
        "namespace with spaces",
        "key' with \"quotes\"; DROP TABLE state; --",
    )
    .unwrap();

    store
        .put(key.clone(), json!({"ok": true}), None)
        .await
        .unwrap();
    assert_eq!(
        store.get(&key).await.unwrap().unwrap().value,
        json!({"ok": true})
    );

    store.close().await.unwrap();
}

#[tokio::test]
async fn closed_store_rejects_operations() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(&dir).await;
    store.close().await.unwrap();

    let error = store
        .get(&StateKey::global("test", "key").unwrap())
        .await
        .unwrap_err();

    assert!(matches!(error, StateStoreError::Closed));
}

#[tokio::test]
async fn invalid_database_path_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let settings = SqliteSettings {
        path: dir.path().to_path_buf(),
        create_parent_dirs: false,
        ..SqliteSettings::new(dir.path().join("unused"))
    };

    let error = SqliteStateStore::connect(&settings).await.unwrap_err();
    assert!(matches!(error, StateStoreError::Backend { .. }));
}

#[tokio::test]
async fn busy_timeout_and_pool_size_are_applied() {
    let dir = tempfile::tempdir().unwrap();
    let mut settings = settings(&dir);
    settings.busy_timeout = Duration::from_millis(100);
    settings.max_connections = 2;

    let store = SqliteStateStore::connect(&settings).await.unwrap();
    let key = StateKey::global("test", "concurrency").unwrap();
    store.put(key.clone(), json!(1), None).await.unwrap();

    let (a, b) = tokio::join!(store.get(&key), store.get(&key));
    assert_eq!(a.unwrap().unwrap().value, json!(1));
    assert_eq!(b.unwrap().unwrap().value, json!(1));

    store.close().await.unwrap();
}
