use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;

use super::secret::deserialize_optional_secret;

/// Top-level third-party service settings.
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct IntegrationsSection {
    pub github: GitHubSettings,
    pub music: MusicSettings,
    pub whois: WhoisSettings,
    pub e621: E621Settings,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GitHubSettings {
    #[serde(deserialize_with = "deserialize_optional_secret")]
    pub token: Option<SecretString>,
}

impl GitHubSettings {
    pub fn token(&self) -> Option<&str> {
        self.token.as_ref().map(|token| token.expose_secret())
    }

    pub fn has_token(&self) -> bool {
        self.token().is_some_and(|token| !token.trim().is_empty())
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MusicSettings {
    pub spotify: SpotifySettings,
    pub soundcloud: SoundCloudSettings,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SpotifySettings {
    pub enabled: bool,
    #[serde(deserialize_with = "deserialize_optional_secret")]
    pub client_id: Option<SecretString>,
    #[serde(deserialize_with = "deserialize_optional_secret")]
    pub client_secret: Option<SecretString>,
}

impl SpotifySettings {
    pub fn available(&self) -> bool {
        self.credentials().is_some()
    }

    /// Returns trimmed credentials only when the provider is enabled and both values are set.
    pub fn credentials(&self) -> Option<(SecretString, SecretString)> {
        if !self.enabled {
            return None;
        }
        let client_id = self.client_id.as_ref()?.expose_secret().trim();
        let client_secret = self.client_secret.as_ref()?.expose_secret().trim();
        if client_id.is_empty() || client_secret.is_empty() {
            return None;
        }
        Some((
            SecretString::from(client_id.to_owned()),
            SecretString::from(client_secret.to_owned()),
        ))
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SoundCloudSettings {
    pub enabled: bool,
    #[serde(deserialize_with = "deserialize_optional_secret")]
    pub client_id: Option<SecretString>,
}

impl SoundCloudSettings {
    pub fn available(&self) -> bool {
        self.client_id_value().is_some()
    }

    pub fn client_id_value(&self) -> Option<SecretString> {
        if !self.enabled {
            return None;
        }
        let client_id = self.client_id.as_ref()?.expose_secret().trim();
        if client_id.is_empty() {
            return None;
        }
        Some(SecretString::from(client_id.to_owned()))
    }
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WhoisSettings {
    pub connect_timeout_secs: u64,
    pub total_timeout_secs: u64,
    pub max_response_bytes: u64,
}

impl Default for WhoisSettings {
    fn default() -> Self {
        Self {
            connect_timeout_secs: 3,
            total_timeout_secs: 8,
            max_response_bytes: 64 * 1024,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct E621Settings {
    pub request_interval_ms: u64,
    pub max_results_per_page: u8,
    pub user_agent: String,
}

impl Default for E621Settings {
    fn default() -> Self {
        Self {
            request_interval_ms: 1_000,
            max_results_per_page: 12,
            user_agent: "LukosBot/0.1 (+https://github.com/Chiloven945/lukosbot-rs)".to_owned(),
        }
    }
}
