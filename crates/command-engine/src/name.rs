use std::fmt;

use thiserror::Error;

/// A validated, normalized command or alias name.
///
/// Names are matched case-insensitively, so the stored representation is always ASCII
/// lowercase. Only ASCII letters, digits, `_`, and `-` are accepted.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CommandName(String);

/// Why a command name was rejected.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CommandNameError {
    #[error("command name must not be empty")]
    Empty,

    #[error("command name contains an invalid character: {0:?}")]
    InvalidCharacter(char),
}

impl CommandName {
    pub fn parse(raw: impl AsRef<str>) -> Result<Self, CommandNameError> {
        let raw = raw.as_ref();
        if raw.is_empty() {
            return Err(CommandNameError::Empty);
        }

        for character in raw.chars() {
            if !(character.is_ascii_alphanumeric() || character == '_' || character == '-') {
                return Err(CommandNameError::InvalidCharacter(character));
            }
        }

        Ok(Self(raw.to_ascii_lowercase()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn matches(&self, token: &str) -> bool {
        self.0.eq_ignore_ascii_case(token)
    }
}

impl fmt::Display for CommandName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl AsRef<str> for CommandName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// Validates a name that is not necessarily normalized, such as a child command node.
pub(crate) fn validate_name(raw: &str) -> Result<(), CommandNameError> {
    CommandName::parse(raw).map(|_| ())
}
