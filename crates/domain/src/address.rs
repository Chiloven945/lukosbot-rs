use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::platform::{ChatKind, PlatformKind};

const MAX_ID_LEN: usize = 64;

/// An invalid platform-provided identifier.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("invalid {kind}: {reason}")]
pub struct InvalidPlatformId {
    kind: &'static str,
    reason: &'static str,
}

impl InvalidPlatformId {
    /// The identifier family that failed validation (`chat id`, `user id`, `message id`).
    pub const fn kind(&self) -> &'static str {
        self.kind
    }

    /// Why the identifier was rejected.
    pub const fn reason(&self) -> &'static str {
        self.reason
    }
}

pub(crate) fn validate_id(kind: &'static str, value: &str) -> Result<(), InvalidPlatformId> {
    let reason = if value.is_empty() {
        "value must not be empty"
    } else if value.len() > MAX_ID_LEN {
        "value is longer than 64 bytes"
    } else if value.chars().any(char::is_control) {
        "value contains control characters"
    } else {
        return Ok(());
    };

    Err(InvalidPlatformId { kind, reason })
}

macro_rules! platform_id {
    ($(#[$meta:meta])* $name:ident, $kind:literal) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Validates and wraps a platform-provided identifier.
            pub fn new(value: impl Into<String>) -> Result<Self, InvalidPlatformId> {
                let value = value.into();
                validate_id($kind, &value)?;
                Ok(Self(value))
            }

            /// Borrows the identifier.
            pub fn as_str(&self) -> &str {
                &self.0
            }

            /// Consumes the identifier and returns the inner string.
            pub fn into_string(self) -> String {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl std::str::FromStr for $name {
            type Err = InvalidPlatformId;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::new(value)
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }
    };
}

platform_id!(
    /// A platform-specific chat identifier.
    PlatformChatId,
    "chat id"
);
platform_id!(
    /// A platform-specific user identifier.
    PlatformUserId,
    "user id"
);

/// A fully qualified chat address: a bare chat id is never globally unique.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChatAddress {
    pub platform: PlatformKind,
    pub chat_id: PlatformChatId,
    pub kind: ChatKind,
}

impl ChatAddress {
    pub fn new(platform: PlatformKind, chat_id: PlatformChatId, kind: ChatKind) -> Self {
        Self {
            platform,
            chat_id,
            kind,
        }
    }
}

/// The author of an inbound message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sender {
    pub id: PlatformUserId,
    pub display_name: Option<String>,
    pub username: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_platform_ids() {
        assert!(PlatformChatId::new("").is_err());
    }

    #[test]
    fn rejects_control_characters_in_platform_ids() {
        assert!(PlatformUserId::new("12\n34").is_err());
    }

    #[test]
    fn same_numeric_id_on_different_platforms_is_not_equal() {
        let telegram = ChatAddress::new(
            PlatformKind::Telegram,
            PlatformChatId::new("42").unwrap(),
            ChatKind::Private,
        );
        let discord = ChatAddress::new(
            PlatformKind::Discord,
            PlatformChatId::new("42").unwrap(),
            ChatKind::Private,
        );

        assert_ne!(telegram, discord);
    }

    #[test]
    fn chat_kinds_are_distinguishable() {
        assert_ne!(ChatKind::Private, ChatKind::Group);
        assert_ne!(ChatKind::Group, ChatKind::Supergroup);
        assert_ne!(ChatKind::Channel, ChatKind::Unknown);
    }
}
