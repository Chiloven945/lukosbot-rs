use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;

use lukosbot_core::ports::{StateKey, StateScope, StateScopeKind};
use lukosbot_domain::{PlatformChatId, PlatformKind, PlatformUserId};

use super::StateImportError;

#[derive(Debug, Deserialize)]
#[serde(tag = "record")]
pub(super) enum ExportLine {
    #[serde(rename = "header")]
    Header {
        format: String,
        version: u32,
        exported_at: String,
    },
    #[serde(rename = "state")]
    State {
        scope_type: String,
        scope_id: String,
        namespace: String,
        key: String,
        value: Value,
        revision: u64,
        created_at: String,
        updated_at: String,
        expires_at: Option<String>,
    },
}

pub(crate) struct ImportedRecord {
    pub(crate) line: usize,
    pub(crate) key: StateKey,
    pub(crate) value_json: String,
    pub(crate) revision: u64,
    pub(crate) created_at_ms: i64,
    pub(crate) updated_at_ms: i64,
    pub(crate) expires_at_ms: Option<i64>,
}

pub(super) fn parse_export(text: &str) -> Result<(Vec<ImportedRecord>, usize), StateImportError> {
    let mut header_seen = false;
    let mut total_lines = 0;
    let mut records = Vec::new();
    let mut seen: std::collections::BTreeMap<StateKey, usize> = std::collections::BTreeMap::new();

    for (index, raw_line) in text.lines().enumerate() {
        let line_number = index + 1;
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        total_lines += 1;

        let parsed: ExportLine = serde_json::from_str(line).map_err(|error| {
            StateImportError::invalid(line_number, format!("invalid JSON: {error}"))
        })?;

        match parsed {
            ExportLine::Header {
                format,
                version,
                exported_at,
            } => {
                if header_seen || !records.is_empty() {
                    return Err(StateImportError::invalid(
                        line_number,
                        "header must appear exactly once before state records",
                    ));
                }
                if format != "lukosbot-state-export" {
                    return Err(StateImportError::invalid(
                        line_number,
                        format!("unsupported export format `{format}`"),
                    ));
                }
                if version != 1 {
                    return Err(StateImportError::invalid(
                        line_number,
                        format!("unsupported export version {version}"),
                    ));
                }
                parse_timestamp(line_number, &exported_at, "exported_at")?;
                header_seen = true;
            }
            ExportLine::State {
                scope_type,
                scope_id,
                namespace,
                key,
                value,
                revision,
                created_at,
                updated_at,
                expires_at,
            } => {
                if !header_seen {
                    return Err(StateImportError::invalid(
                        line_number,
                        "state record appears before the header",
                    ));
                }

                let scope = parse_scope(line_number, &scope_type, &scope_id)?;
                let key = StateKey::new(scope, namespace, key)
                    .map_err(|error| StateImportError::invalid(line_number, error.to_string()))?;

                if let Some(previous_line) = seen.insert(key.clone(), line_number) {
                    return Err(StateImportError::Conflict {
                        line: line_number,
                        key: format!("{} (first seen at line {previous_line})", key_label(&key)),
                    });
                }

                let value_json = serde_json::to_string(&value).map_err(|error| {
                    StateImportError::invalid(
                        line_number,
                        format!("value cannot be encoded as JSON: {error}"),
                    )
                })?;

                records.push(ImportedRecord {
                    line: line_number,
                    key,
                    value_json,
                    revision,
                    created_at_ms: parse_timestamp(line_number, &created_at, "created_at")?
                        .timestamp_millis(),
                    updated_at_ms: parse_timestamp(line_number, &updated_at, "updated_at")?
                        .timestamp_millis(),
                    expires_at_ms: expires_at
                        .as_deref()
                        .map(|value| parse_timestamp(line_number, value, "expires_at"))
                        .transpose()?
                        .map(|value| value.timestamp_millis()),
                });
            }
        }
    }

    if !header_seen {
        return Err(StateImportError::invalid(
            1,
            "missing required export header",
        ));
    }

    Ok((records, total_lines))
}

fn parse_scope(
    line: usize,
    scope_type: &str,
    scope_id: &str,
) -> Result<StateScope, StateImportError> {
    let kind = StateScopeKind::from_str(scope_type)
        .map_err(|error| StateImportError::invalid(line, error.to_string()))?;

    match kind {
        StateScopeKind::Global => {
            if scope_id.is_empty() || scope_id == "_" {
                Ok(StateScope::Global)
            } else {
                Err(StateImportError::invalid(
                    line,
                    format!("unexpected global scope id `{scope_id}`"),
                ))
            }
        }
        StateScopeKind::User => {
            let Some((platform, user_id)) = scope_id.split_once(':') else {
                return Err(StateImportError::invalid(
                    line,
                    format!("invalid user scope id `{scope_id}`"),
                ));
            };
            let platform = parse_platform(line, platform)?;
            let user_id = PlatformUserId::new(user_id)
                .map_err(|error| StateImportError::invalid(line, error.to_string()))?;
            Ok(StateScope::user(platform, user_id))
        }
        StateScopeKind::Chat => {
            let parts: Vec<&str> = scope_id.splitn(3, ':').collect();
            if parts.len() != 3 || !matches!(parts[1].to_ascii_lowercase().as_str(), "g" | "p") {
                return Err(StateImportError::invalid(
                    line,
                    format!("invalid chat scope id `{scope_id}`"),
                ));
            }
            let platform = parse_platform(line, parts[0])?;
            let chat_id = PlatformChatId::new(parts[2])
                .map_err(|error| StateImportError::invalid(line, error.to_string()))?;
            Ok(StateScope::Chat { platform, chat_id })
        }
    }
}

fn parse_platform(line: usize, value: &str) -> Result<PlatformKind, StateImportError> {
    PlatformKind::from_str(value.trim().to_ascii_lowercase().as_str())
        .map_err(|error| StateImportError::invalid(line, error.to_string()))
}

fn parse_timestamp(
    line: usize,
    value: &str,
    field: &str,
) -> Result<DateTime<Utc>, StateImportError> {
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|error| {
            StateImportError::invalid(
                line,
                format!("invalid {field} timestamp `{value}`: {error}"),
            )
        })
}

pub(super) fn key_label(key: &StateKey) -> String {
    let platform = key.scope.platform().map_or("", PlatformKind::as_str);
    let scope_id = key.scope.scope_id().unwrap_or("");
    format!(
        "{}/{platform}/{scope_id}/{}/{}",
        key.scope.scope_type(),
        key.namespace,
        key.key
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope(scope_type: &str, scope_id: &str) -> StateScope {
        parse_scope(1, scope_type, scope_id).unwrap()
    }

    #[test]
    fn maps_legacy_scopes() {
        assert_eq!(scope("GLOBAL", "_"), StateScope::Global);
        assert_eq!(
            scope("USER", "TELEGRAM:123"),
            StateScope::User {
                platform: PlatformKind::Telegram,
                user_id: PlatformUserId::new("123").unwrap()
            }
        );
        assert_eq!(
            scope("CHAT", "TELEGRAM:g:-100123"),
            StateScope::Chat {
                platform: PlatformKind::Telegram,
                chat_id: PlatformChatId::new("-100123").unwrap()
            }
        );
        assert_eq!(
            scope("CHAT", "DISCORD:p:123"),
            StateScope::Chat {
                platform: PlatformKind::Discord,
                chat_id: PlatformChatId::new("123").unwrap()
            }
        );
    }

    #[test]
    fn rejects_unknown_scope_formats() {
        assert!(parse_scope(1, "CHAT", "TELEGRAM:123").is_err());
        assert!(parse_scope(1, "USER", "TELEGRAM").is_err());
        assert!(parse_scope(1, "GLOBAL", "other").is_err());
        assert!(parse_scope(1, "UNKNOWN", "_").is_err());
    }
}
