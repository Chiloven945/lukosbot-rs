use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;

use super::secret::deserialize_optional_secret;

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PlatformsSection {
    pub telegram: TelegramSection,
    pub discord: DiscordSection,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TelegramSection {
    pub enabled: bool,
    #[serde(deserialize_with = "deserialize_optional_secret")]
    pub token: Option<SecretString>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DiscordSection {
    pub enabled: bool,
    #[serde(deserialize_with = "deserialize_optional_secret")]
    pub token: Option<SecretString>,
}

impl TelegramSection {
    pub fn token(&self) -> Option<&str> {
        self.token.as_ref().map(|token| token.expose_secret())
    }
}

impl DiscordSection {
    pub fn token(&self) -> Option<&str> {
        self.token.as_ref().map(|token| token.expose_secret())
    }
}
