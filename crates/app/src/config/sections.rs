use std::fmt;
use std::path::PathBuf;

use serde::Deserialize;

use super::{PlatformsSection, PolicySection, SecuritySection};

/// Root configuration object.
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AppConfig {
    pub app: AppSection,
    pub logging: LoggingSection,
    pub runtime: RuntimeSection,
    pub http: HttpSection,
    pub storage: StorageSection,
    pub platforms: PlatformsSection,
    pub commands: CommandsSection,
    pub security: SecuritySection,
    pub policy: PolicySection,
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AppSection {
    pub environment: Environment,
    pub shutdown_timeout_secs: u64,
}

impl Default for AppSection {
    fn default() -> Self {
        Self {
            environment: Environment::Development,
            shutdown_timeout_secs: 15,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Environment {
    #[default]
    Development,
    Production,
}

impl fmt::Display for Environment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Development => "development",
            Self::Production => "production",
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LoggingSection {
    pub format: LogFormat,
    pub filter: String,
}

impl Default for LoggingSection {
    fn default() -> Self {
        Self {
            format: LogFormat::Pretty,
            filter: "info,sqlx=warn".to_owned(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    #[default]
    Pretty,
    Compact,
    Json,
}

impl fmt::Display for LogFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Pretty => "pretty",
            Self::Compact => "compact",
            Self::Json => "json",
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RuntimeSection {
    pub inbound_queue_capacity: usize,
    pub outbound_queue_capacity: usize,
    pub handler_timeout_secs: u64,
    pub send_timeout_secs: u64,
}

impl Default for RuntimeSection {
    fn default() -> Self {
        Self {
            inbound_queue_capacity: 256,
            outbound_queue_capacity: 128,
            handler_timeout_secs: 30,
            send_timeout_secs: 20,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HttpSection {
    pub connect_timeout_secs: u64,
    pub request_timeout_secs: u64,
    pub max_response_bytes: u64,
    pub user_agent: String,
    pub proxy: String,
}

impl HttpSection {
    /// Returns the configured proxy URL when one is set.
    pub fn proxy_url(&self) -> Option<&str> {
        let proxy = self.proxy.trim();
        (!proxy.is_empty()).then_some(proxy)
    }
}

impl Default for HttpSection {
    fn default() -> Self {
        Self {
            connect_timeout_secs: 5,
            request_timeout_secs: 20,
            max_response_bytes: 10 * 1024 * 1024,
            user_agent: "LukosBot/0.1 (+https://github.com/Chiloven945/lukosbot-rs)".to_owned(),
            proxy: String::new(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StorageSection {
    pub sqlite_path: PathBuf,
    pub busy_timeout_ms: u64,
    pub max_connections: u32,
    pub create_parent_dirs: bool,
    pub expiry_cleanup_interval_secs: u64,
}

impl Default for StorageSection {
    fn default() -> Self {
        Self {
            sqlite_path: PathBuf::from("./data/lukosbot.sqlite3"),
            busy_timeout_ms: 5_000,
            max_connections: 5,
            create_parent_dirs: true,
            expiry_cleanup_interval_secs: 3_600,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CommandsSection {
    pub prefix: String,
    pub basic: BasicCommandSettings,
    pub twenty_four: TwentyFourSettings,
    pub control: ControlCommandSettings,
}

impl Default for CommandsSection {
    fn default() -> Self {
        Self {
            prefix: "/".to_owned(),
            basic: BasicCommandSettings::default(),
            twenty_four: TwentyFourSettings::default(),
            control: ControlCommandSettings::default(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ControlCommandSettings {
    pub admin: bool,
    pub pref: bool,
    pub service: bool,
}

impl Default for ControlCommandSettings {
    fn default() -> Self {
        Self {
            admin: true,
            pref: true,
            service: true,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BasicCommandSettings {
    pub ping: bool,
    pub echo: bool,
    pub coin: bool,
    pub dice: bool,
    pub luck: bool,
    pub twenty_four: bool,
    pub weather: bool,
}

impl Default for BasicCommandSettings {
    fn default() -> Self {
        Self {
            ping: true,
            echo: true,
            coin: true,
            dice: true,
            luck: true,
            twenty_four: true,
            weather: true,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TwentyFourSettings {
    pub time_limit_secs: u64,
}

impl Default for TwentyFourSettings {
    fn default() -> Self {
        Self {
            time_limit_secs: 300,
        }
    }
}
