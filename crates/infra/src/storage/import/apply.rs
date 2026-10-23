use sqlx::{Sqlite, SqlitePool, Transaction};

use lukosbot_core::ports::{SCHEMA_VERSION, StateKey, StateStoreError};
use lukosbot_domain::PlatformKind;

use super::format::{ImportedRecord, key_label};
use super::{ImportConflictMode, ImportReport, StateImportError};

pub(super) async fn dry_run_records(
    pool: &SqlitePool,
    records: &[ImportedRecord],
    mode: ImportConflictMode,
    report: &mut ImportReport,
) -> Result<(), StateImportError> {
    for record in records {
        let exists = key_exists_pool(pool, &record.key).await?;
        if exists {
            report.conflicts += 1;
            match mode {
                ImportConflictMode::Fail => {
                    return Err(StateImportError::Conflict {
                        line: record.line,
                        key: key_label(&record.key),
                    });
                }
                ImportConflictMode::Skip => report.skipped += 1,
                ImportConflictMode::Replace => report.imported += 1,
            }
        } else {
            report.imported += 1;
        }
    }
    Ok(())
}

pub(super) async fn apply_records(
    pool: &SqlitePool,
    records: &[ImportedRecord],
    mode: ImportConflictMode,
    report: &mut ImportReport,
) -> Result<(), StateImportError> {
    let mut transaction = pool.begin().await?;

    for record in records {
        let exists = key_exists_tx(&mut transaction, &record.key).await?;
        if exists {
            report.conflicts += 1;
            match mode {
                ImportConflictMode::Fail => {
                    transaction.rollback().await?;
                    return Err(StateImportError::Conflict {
                        line: record.line,
                        key: key_label(&record.key),
                    });
                }
                ImportConflictMode::Skip => {
                    report.skipped += 1;
                    continue;
                }
                ImportConflictMode::Replace => {}
            }
        }

        write_record(&mut transaction, record, exists).await?;
        report.imported += 1;
    }

    transaction.commit().await?;
    Ok(())
}

async fn key_exists_pool(pool: &SqlitePool, key: &StateKey) -> Result<bool, StateStoreError> {
    let row = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT 1 FROM state
        WHERE scope_type = ?1 AND scope_platform = ?2 AND scope_id = ?3
          AND namespace = ?4 AND "key" = ?5
        LIMIT 1
        "#,
    )
    .bind(key.scope.scope_type())
    .bind(key.scope.platform().map_or("", PlatformKind::as_str))
    .bind(key.scope.scope_id().unwrap_or(""))
    .bind(&key.namespace)
    .bind(&key.key)
    .fetch_optional(pool)
    .await
    .map_err(|error| StateStoreError::backend("check import conflict", error))?;
    Ok(row.is_some())
}

async fn key_exists_tx(
    connection: &mut Transaction<'_, Sqlite>,
    key: &StateKey,
) -> Result<bool, StateStoreError> {
    let row = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT 1 FROM state
        WHERE scope_type = ?1 AND scope_platform = ?2 AND scope_id = ?3
          AND namespace = ?4 AND "key" = ?5
        LIMIT 1
        "#,
    )
    .bind(key.scope.scope_type())
    .bind(key.scope.platform().map_or("", PlatformKind::as_str))
    .bind(key.scope.scope_id().unwrap_or(""))
    .bind(&key.namespace)
    .bind(&key.key)
    .fetch_optional(&mut **connection)
    .await
    .map_err(|error| StateStoreError::backend("check import conflict", error))?;
    Ok(row.is_some())
}

async fn write_record(
    connection: &mut Transaction<'_, Sqlite>,
    record: &ImportedRecord,
    replace: bool,
) -> Result<(), StateImportError> {
    let sql = if replace {
        r#"
        INSERT INTO state (
            scope_type, scope_platform, scope_id, namespace, "key",
            value_json, schema_version, revision, created_at, updated_at, expires_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
        ON CONFLICT (scope_type, scope_platform, scope_id, namespace, "key")
        DO UPDATE SET
            value_json = excluded.value_json,
            schema_version = excluded.schema_version,
            revision = excluded.revision,
            created_at = excluded.created_at,
            updated_at = excluded.updated_at,
            expires_at = excluded.expires_at
        "#
    } else {
        r#"
        INSERT INTO state (
            scope_type, scope_platform, scope_id, namespace, "key",
            value_json, schema_version, revision, created_at, updated_at, expires_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
        "#
    };

    sqlx::query(sql)
        .bind(record.key.scope.scope_type())
        .bind(record.key.scope.platform().map_or("", PlatformKind::as_str))
        .bind(record.key.scope.scope_id().unwrap_or(""))
        .bind(&record.key.namespace)
        .bind(&record.key.key)
        .bind(&record.value_json)
        .bind(i64::from(SCHEMA_VERSION))
        .bind(i64::try_from(record.revision).map_err(|error| {
            StateImportError::invalid(record.line, format!("revision is too large: {error}"))
        })?)
        .bind(record.created_at_ms)
        .bind(record.updated_at_ms)
        .bind(record.expires_at_ms)
        .execute(&mut **connection)
        .await
        .map_err(|error| StateStoreError::backend("insert imported state entry", error))?;

    Ok(())
}
