use std::path::Path;

use chrono::{TimeZone, Utc};
use serde_json::{Value, json};

use lukosbot_core::StateStore;
use lukosbot_core::ports::{StateKey, StateScope};
use lukosbot_infra::storage::{
    ImportConflictMode, ImportOptions, SqliteSettings, SqliteStateStore, import_jsonl,
};

fn line(value: Value) -> String {
    serde_json::to_string(&value).unwrap()
}

fn export_text(value_override: Option<&str>) -> String {
    let mut records = vec![
        line(json!({
            "record": "header",
            "format": "lukosbot-state-export",
            "version": 1,
            "exported_at": "2026-10-23T00:00:00Z"
        })),
        line(json!({
            "record": "state",
            "scope_type": "GLOBAL",
            "scope_id": "_",
            "namespace": "auth",
            "key": "bot_admins",
            "value": {"TELEGRAM": [7]},
            "revision": 3,
            "created_at": "2026-10-20T10:00:00Z",
            "updated_at": "2026-10-22T10:00:00Z",
            "expires_at": null
        })),
        line(json!({
            "record": "state",
            "scope_type": "USER",
            "scope_id": "TELEGRAM:7",
            "namespace": "prefs",
            "key": "lang",
            "value": value_override.unwrap_or("en"),
            "revision": 0,
            "created_at": "2026-10-20T10:00:00Z",
            "updated_at": "2026-10-20T10:00:00Z",
            "expires_at": null
        })),
        line(json!({
            "record": "state",
            "scope_type": "CHAT",
            "scope_id": "TELEGRAM:g:-100",
            "namespace": "service",
            "key": "demo",
            "value": {"enabled": true},
            "revision": 1,
            "created_at": "2026-10-20T10:00:00Z",
            "updated_at": "2026-10-20T10:00:00Z",
            "expires_at": null
        })),
        line(json!({
            "record": "state",
            "scope_type": "GLOBAL",
            "scope_id": "_",
            "namespace": "cmd.cave.meta",
            "key": "meta",
            "value": {"next": 1},
            "revision": 0,
            "created_at": "2026-10-20T10:00:00Z",
            "updated_at": "2026-10-20T10:00:00Z",
            "expires_at": null
        })),
        line(json!({
            "record": "state",
            "scope_type": "GLOBAL",
            "scope_id": "_",
            "namespace": "cache",
            "key": "expired",
            "value": 1,
            "revision": 0,
            "created_at": "2026-10-20T10:00:00Z",
            "updated_at": "2026-10-20T10:00:00Z",
            "expires_at": "2020-01-01T00:00:00Z"
        })),
    ];
    records.push(String::new());
    records.join("\n")
}

async fn store(dir: &Path) -> SqliteStateStore {
    SqliteStateStore::connect(&SqliteSettings::new(dir.join("state.sqlite3")))
        .await
        .unwrap()
}

async fn write_export(dir: &Path, value: Option<&str>) -> std::path::PathBuf {
    let path = dir.join("export.jsonl");
    std::fs::write(&path, export_text(value)).unwrap();
    path
}

#[tokio::test]
async fn imports_the_checked_in_compatibility_fixture() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path()).await;
    let path = dir.path().join("fixture.jsonl");
    std::fs::write(&path, include_str!("fixtures/state-export.jsonl")).unwrap();

    let report = import_jsonl(
        &store,
        &path,
        ImportOptions {
            apply: true,
            on_conflict: ImportConflictMode::Fail,
        },
    )
    .await
    .unwrap();
    assert_eq!(report.valid_records, 1);

    let admins = store
        .get(&StateKey::global("auth", "bot_admins").unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(admins.value, json!({"TELEGRAM": [7]}));
    assert_eq!(admins.revision, 2);
}

#[tokio::test]
async fn dry_run_does_not_write_and_apply_preserves_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path()).await;
    let path = write_export(dir.path(), None).await;

    let dry = import_jsonl(&store, &path, ImportOptions::default())
        .await
        .unwrap();
    assert!(dry.dry_run);
    assert_eq!(dry.valid_records, 5);
    assert_eq!(dry.imported, 5);
    assert_eq!(dry.expired, 1);
    assert!(
        store
            .get(&StateKey::global("auth", "bot_admins").unwrap())
            .await
            .unwrap()
            .is_none()
    );

    let applied = import_jsonl(
        &store,
        &path,
        ImportOptions {
            apply: true,
            on_conflict: ImportConflictMode::Fail,
        },
    )
    .await
    .unwrap();
    assert_eq!(applied.imported, 5);

    let admins = store
        .get(&StateKey::global("auth", "bot_admins").unwrap())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(admins.value, json!({"TELEGRAM": [7]}));
    assert_eq!(admins.schema_version, 1);
    assert_eq!(admins.revision, 3);
    assert_eq!(
        admins.created_at,
        Utc.with_ymd_and_hms(2026, 10, 20, 10, 0, 0).unwrap()
    );

    let expired = store
        .get(&StateKey::global("cache", "expired").unwrap())
        .await
        .unwrap();
    assert!(expired.is_none());

    let service = store
        .list_namespace(
            &StateScope::Chat {
                platform: lukosbot_domain::PlatformKind::Telegram,
                chat_id: lukosbot_domain::PlatformChatId::new("-100").unwrap(),
            },
            "service",
        )
        .await
        .unwrap();
    assert_eq!(service.len(), 1);
    assert_eq!(service[0].key.key, "demo");
}

#[tokio::test]
async fn conflicts_default_to_fail_and_skip_preserves_target_values() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path()).await;
    let path = write_export(dir.path(), None).await;
    let apply = ImportOptions {
        apply: true,
        on_conflict: ImportConflictMode::Fail,
    };

    import_jsonl(&store, &path, apply).await.unwrap();

    let error = import_jsonl(&store, &path, apply).await.unwrap_err();
    assert!(error.to_string().contains("conflict"));

    let skipped = import_jsonl(
        &store,
        &path,
        ImportOptions {
            apply: true,
            on_conflict: ImportConflictMode::Skip,
        },
    )
    .await
    .unwrap();
    assert_eq!(skipped.conflicts, 5);
    assert_eq!(skipped.skipped, 5);
    assert_eq!(skipped.imported, 0);
}

#[tokio::test]
async fn replace_updates_target_values_and_revision() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path()).await;
    let first = write_export(dir.path(), None).await;
    let apply = ImportOptions {
        apply: true,
        on_conflict: ImportConflictMode::Fail,
    };
    import_jsonl(&store, &first, apply).await.unwrap();

    let second = dir.path().join("export-replace.jsonl");
    std::fs::write(&second, export_text(Some("fr"))).unwrap();
    let report = import_jsonl(
        &store,
        &second,
        ImportOptions {
            apply: true,
            on_conflict: ImportConflictMode::Replace,
        },
    )
    .await
    .unwrap();
    assert_eq!(report.conflicts, 5);
    assert_eq!(report.imported, 5);

    let lang = store
        .get(
            &StateKey::new(
                StateScope::user(
                    lukosbot_domain::PlatformKind::Telegram,
                    lukosbot_domain::PlatformUserId::new("7").unwrap(),
                ),
                "prefs",
                "lang",
            )
            .unwrap(),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(lang.value, json!("fr"));
    assert_eq!(lang.revision, 0);
}

#[tokio::test]
async fn scope_collision_and_invalid_json_are_reported() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path()).await;

    let collision = dir.path().join("collision.jsonl");
    std::fs::write(
        &collision,
        [
            line(json!({
                "record": "header",
                "format": "lukosbot-state-export",
                "version": 1,
                "exported_at": "2026-10-23T00:00:00Z"
            })),
            line(json!({
                "record": "state",
                "scope_type": "CHAT",
                "scope_id": "TELEGRAM:g:1",
                "namespace": "service",
                "key": "demo",
                "value": true,
                "revision": 0,
                "created_at": "2026-10-20T10:00:00Z",
                "updated_at": "2026-10-20T10:00:00Z",
                "expires_at": null
            })),
            line(json!({
                "record": "state",
                "scope_type": "CHAT",
                "scope_id": "TELEGRAM:p:1",
                "namespace": "service",
                "key": "demo",
                "value": false,
                "revision": 0,
                "created_at": "2026-10-20T10:00:00Z",
                "updated_at": "2026-10-20T10:00:00Z",
                "expires_at": null
            })),
        ]
        .join("\n"),
    )
    .unwrap();
    let error = import_jsonl(&store, &collision, ImportOptions::default())
        .await
        .unwrap_err();
    assert!(error.to_string().contains("conflict"));

    let invalid = dir.path().join("invalid.jsonl");
    std::fs::write(&invalid, "{not json}\n").unwrap();
    let error = import_jsonl(&store, &invalid, ImportOptions::default())
        .await
        .unwrap_err();
    assert!(error.to_string().contains("invalid state export"));
}
