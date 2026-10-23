use std::fmt;
use std::path::Path;

use chrono::Utc;
use thiserror::Error;

use super::sqlite::SqliteStateStore;

mod apply;
mod format;

/// How target conflicts are handled during a migration import.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ImportConflictMode {
    #[default]
    Fail,
    Skip,
    Replace,
}

/// Options for [`import_jsonl`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImportOptions {
    pub apply: bool,
    pub on_conflict: ImportConflictMode,
}

impl Default for ImportOptions {
    fn default() -> Self {
        Self {
            apply: false,
            on_conflict: ImportConflictMode::Fail,
        }
    }
}

/// Counts produced by a dry-run or completed import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportReport {
    pub dry_run: bool,
    pub total_lines: usize,
    pub valid_records: usize,
    pub imported: usize,
    pub skipped: usize,
    pub conflicts: usize,
    pub expired: usize,
}

impl fmt::Display for ImportReport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            formatter,
            "{} state import:",
            if self.dry_run { "dry-run" } else { "applied" }
        )?;
        writeln!(formatter, "  lines: {}", self.total_lines)?;
        writeln!(formatter, "  valid records: {}", self.valid_records)?;
        writeln!(formatter, "  imported/would import: {}", self.imported)?;
        writeln!(formatter, "  skipped: {}", self.skipped)?;
        writeln!(formatter, "  conflicts: {}", self.conflicts)?;
        write!(formatter, "  expired: {}", self.expired)
    }
}

/// A JSONL migration failure.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum StateImportError {
    #[error("failed to read state export: {0}")]
    Io(#[from] std::io::Error),

    #[error("invalid state export at line {line}: {message}")]
    InvalidLine { line: usize, message: String },

    #[error("state export conflict at line {line} for key {key}")]
    Conflict { line: usize, key: String },

    #[error(transparent)]
    Storage(#[from] lukosbot_core::StateStoreError),

    #[error("state import database failure")]
    Database(#[from] sqlx::Error),
}

impl StateImportError {
    pub(super) fn invalid(line: usize, message: impl Into<String>) -> Self {
        Self::InvalidLine {
            line,
            message: message.into(),
        }
    }
}

/// Imports the JSONL format produced by the JVM `bot_state` exporter.
pub async fn import_jsonl(
    store: &SqliteStateStore,
    input: &Path,
    options: ImportOptions,
) -> Result<ImportReport, StateImportError> {
    let text = std::fs::read_to_string(input)?;
    let (records, total_lines) = format::parse_export(&text)?;

    let now = Utc::now();
    let expired = records
        .iter()
        .filter(|record| {
            record
                .expires_at_ms
                .is_some_and(|expires_at| expires_at <= now.timestamp_millis())
        })
        .count();

    let mut report = ImportReport {
        dry_run: !options.apply,
        total_lines,
        valid_records: records.len(),
        imported: 0,
        skipped: 0,
        conflicts: 0,
        expired,
    };

    if options.apply {
        apply::apply_records(store.pool(), &records, options.on_conflict, &mut report).await?;
    } else {
        apply::dry_run_records(store.pool(), &records, options.on_conflict, &mut report).await?;
    }

    Ok(report)
}
