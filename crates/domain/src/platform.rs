use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// The chat platform a message originates from or is sent to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlatformKind {
    Telegram,
    Discord,
}

impl PlatformKind {
    /// Stable, lower-case identifier used in configuration, logs, and storage keys.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Telegram => "telegram",
            Self::Discord => "discord",
        }
    }
}

impl fmt::Display for PlatformKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Returned when a platform identifier string is not recognised.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("unknown platform `{0}`")]
pub struct ParsePlatformKindError(pub String);

impl FromStr for PlatformKind {
    type Err = ParsePlatformKindError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "telegram" => Ok(Self::Telegram),
            "discord" => Ok(Self::Discord),
            other => Err(ParsePlatformKindError(other.to_owned())),
        }
    }
}

/// How a chat is exposed by the platform.
///
/// `Unknown` exists so adapters never have to guess: it is better to surface an unknown chat
/// kind than to silently claim a message came from a group or a private chat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatKind {
    Private,
    Group,
    Supergroup,
    Channel,
    Unknown,
}
