//! Typed application configuration.
//!
//! Configuration is layered as `CLI > environment > TOML file > defaults`. Environment keys use
//! the `LUKOSBOT` prefix and `__` as the nesting separator, for example
//! `LUKOSBOT__RUNTIME__INBOUND_QUEUE_CAPACITY=128`.
//!
//! Configuration is only read and validated at startup. There is no hot reload and the file is
//! never rewritten by the application.

use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use serde::de::{self, Deserializer, Visitor};
use thiserror::Error;

/// Environment variable prefix used for configuration overrides.
pub const ENV_PREFIX: &str = "LUKOSBOT";
/// Separator used between nested keys in environment variables.
pub const ENV_SEPARATOR: &str = "__";

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
}

impl Default for StorageSection {
    fn default() -> Self {
        Self {
            sqlite_path: PathBuf::from("./data/lukosbot.sqlite3"),
            busy_timeout_ms: 5_000,
            max_connections: 5,
            create_parent_dirs: true,
        }
    }
}

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

/// Nesting separators and prefixes are not applied to secrets by the generic config layer, so
/// this visitor accepts the scalar types that `try_parsing` may infer from an environment
/// variable. This keeps numeric-looking tokens working.
fn deserialize_optional_secret<'de, D>(deserializer: D) -> Result<Option<SecretString>, D::Error>
where
    D: Deserializer<'de>,
{
    struct OptionVisitor;

    impl<'de> Visitor<'de> for OptionVisitor {
        type Value = Option<SecretString>;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a secret string, an integer, or null")
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }

        fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
        where
            D: Deserializer<'de>,
        {
            deserializer.deserialize_any(ScalarVisitor).map(Some)
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Some(SecretString::from(value.to_owned())))
        }

        fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Some(SecretString::from(value)))
        }

        fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Some(SecretString::from(value.to_string())))
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Some(SecretString::from(value.to_string())))
        }
    }

    struct ScalarVisitor;

    impl Visitor<'_> for ScalarVisitor {
        type Value = SecretString;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a secret string or an integer")
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(SecretString::from(value.to_owned()))
        }

        fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(SecretString::from(value))
        }

        fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(SecretString::from(value.to_string()))
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(SecretString::from(value.to_string()))
        }
    }

    deserializer.deserialize_option(OptionVisitor)
}

/// Platforms compiled into this binary. Used to reject configurations that enable an adapter
/// which is not present in the build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompiledPlatforms {
    pub telegram: bool,
    pub discord: bool,
}

impl CompiledPlatforms {
    pub const fn current() -> Self {
        Self {
            telegram: cfg!(feature = "telegram"),
            discord: cfg!(feature = "discord"),
        }
    }
}

/// CLI overrides applied after the layered configuration has been deserialized.
#[derive(Debug, Default, Clone)]
pub struct CliOverrides {
    pub log_filter: Option<String>,
}

/// Inputs for [`load`]. Environment variables are supplied explicitly so tests never have to
/// mutate the process environment.
#[derive(Debug)]
pub struct LoadOptions {
    pub path: PathBuf,
    pub env: HashMap<String, String>,
    pub compiled: CompiledPlatforms,
    pub overrides: CliOverrides,
}

impl LoadOptions {
    pub fn from_process(
        path: PathBuf,
        compiled: CompiledPlatforms,
        overrides: CliOverrides,
    ) -> Self {
        Self {
            path,
            env: std::env::vars().collect(),
            compiled,
            overrides,
        }
    }
}

/// A validated configuration and the file it came from.
#[derive(Debug)]
pub struct LoadedConfig {
    pub config: AppConfig,
    pub path: PathBuf,
}

/// Configuration loading or validation failure.
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to load configuration")]
    Load(#[from] ::config::ConfigError),

    #[error("configuration is invalid:\n{0}")]
    Invalid(ConfigIssues),
}

impl ConfigError {
    /// Individual validation problems, when the error is a validation failure.
    pub fn issues(&self) -> &[String] {
        match self {
            Self::Invalid(issues) => &issues.0,
            Self::Load(_) => &[],
        }
    }
}

/// A list of validation problems, rendered as an indented block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigIssues(pub Vec<String>);

impl fmt::Display for ConfigIssues {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for issue in &self.0 {
            writeln!(f, "  - {issue}")?;
        }
        Ok(())
    }
}

/// Walks the default configuration file path and returns a helpful error when it does not exist.
pub fn load(options: LoadOptions) -> Result<LoadedConfig, ConfigError> {
    let mut builder = ::config::Config::builder();

    if options.path.exists() {
        builder = builder.add_source(
            ::config::File::from(options.path.clone()).format(::config::FileFormat::Toml),
        );
    }

    let environment = ::config::Environment::with_prefix(ENV_PREFIX)
        .prefix_separator(ENV_SEPARATOR)
        .separator(ENV_SEPARATOR)
        .try_parsing(true)
        .source(Some(options.env));

    let config = builder
        .add_source(environment)
        .build()?
        .try_deserialize::<AppConfig>()?;

    let mut config = config;
    if let Some(filter) = options.overrides.log_filter {
        config.logging.filter = filter;
    }

    config.validate(options.compiled)?;

    Ok(LoadedConfig {
        config,
        path: options.path,
    })
}

impl AppConfig {
    /// Validates every section and reports all problems at once.
    pub fn validate(&self, compiled: CompiledPlatforms) -> Result<(), ConfigError> {
        let mut issues = Vec::new();

        if !(1..=300).contains(&self.app.shutdown_timeout_secs) {
            issues.push("app.shutdown_timeout_secs must be between 1 and 300".to_owned());
        }

        if self.logging.filter.trim().is_empty() {
            issues.push("logging.filter must not be empty".to_owned());
        }

        check_range(
            &mut issues,
            "runtime.inbound_queue_capacity",
            self.runtime.inbound_queue_capacity as u64,
            1,
            65_536,
        );
        check_range(
            &mut issues,
            "runtime.outbound_queue_capacity",
            self.runtime.outbound_queue_capacity as u64,
            1,
            65_536,
        );
        check_range(
            &mut issues,
            "runtime.handler_timeout_secs",
            self.runtime.handler_timeout_secs,
            1,
            3_600,
        );
        check_range(
            &mut issues,
            "runtime.send_timeout_secs",
            self.runtime.send_timeout_secs,
            1,
            600,
        );

        check_range(
            &mut issues,
            "http.connect_timeout_secs",
            self.http.connect_timeout_secs,
            1,
            120,
        );
        check_range(
            &mut issues,
            "http.request_timeout_secs",
            self.http.request_timeout_secs,
            1,
            600,
        );
        check_range(
            &mut issues,
            "http.max_response_bytes",
            self.http.max_response_bytes,
            1_024,
            1_073_741_824,
        );
        if self.http.user_agent.trim().is_empty() {
            issues.push("http.user_agent must not be empty".to_owned());
        }
        if let Some(proxy) = self.http.proxy_url() {
            let supported = ["http://", "https://", "socks5://", "socks5h://"];
            if !supported.iter().any(|prefix| proxy.starts_with(prefix)) {
                issues.push(
                    "http.proxy must start with http://, https://, socks5://, or socks5h://"
                        .to_owned(),
                );
            }
        }

        if self.storage.sqlite_path.as_os_str().is_empty() {
            issues.push("storage.sqlite_path must not be empty".to_owned());
        }
        check_range(
            &mut issues,
            "storage.max_connections",
            u64::from(self.storage.max_connections),
            1,
            64,
        );
        check_range(
            &mut issues,
            "storage.busy_timeout_ms",
            self.storage.busy_timeout_ms,
            0,
            60_000,
        );

        let telegram = &self.platforms.telegram;
        if telegram.enabled {
            if !compiled.telegram {
                issues.push(
                    "platforms.telegram.enabled requires the `telegram` build feature".to_owned(),
                );
            }
            if telegram.token().is_none_or(str::is_empty) {
                issues.push(
                    "platforms.telegram.token is required when Telegram is enabled".to_owned(),
                );
            }
        }

        let discord = &self.platforms.discord;
        if discord.enabled {
            if !compiled.discord {
                issues.push(
                    "platforms.discord.enabled requires the `discord` build feature".to_owned(),
                );
            }
            if discord.token().is_none_or(str::is_empty) {
                issues
                    .push("platforms.discord.token is required when Discord is enabled".to_owned());
            }
        }

        if issues.is_empty() {
            Ok(())
        } else {
            Err(ConfigError::Invalid(ConfigIssues(issues)))
        }
    }

    /// Requires at least one platform before the application starts connecting.
    ///
    /// `check-config` deliberately does not require this, so the shipped example file can be
    /// validated with every platform disabled.
    pub fn require_platform(&self) -> Result<(), ConfigError> {
        if self.platforms.telegram.enabled || self.platforms.discord.enabled {
            Ok(())
        } else {
            Err(ConfigError::Invalid(ConfigIssues(vec![
                "at least one platform must be enabled to run".to_owned(),
            ])))
        }
    }

    /// Process-level durations derived from validated scalar configuration.
    pub fn shutdown_timeout(&self) -> Duration {
        Duration::from_secs(self.app.shutdown_timeout_secs)
    }
}

fn check_range(issues: &mut Vec<String>, field: &str, value: u64, min: u64, max: u64) {
    if !(min..=max).contains(&value) {
        issues.push(format!(
            "{field} must be between {min} and {max} (got {value})"
        ));
    }
}

/// Human-readable configuration summary. Secrets are intentionally not part of it.
pub struct ConfigSummary<'a>(pub &'a AppConfig);

impl fmt::Display for ConfigSummary<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let config = self.0;
        writeln!(f, "environment: {}", config.app.environment)?;
        writeln!(
            f,
            "logging: {} ({})",
            config.logging.format, config.logging.filter
        )?;
        writeln!(
            f,
            "runtime: inbound_queue={} outbound_queue={} handler_timeout={}s send_timeout={}s",
            config.runtime.inbound_queue_capacity,
            config.runtime.outbound_queue_capacity,
            config.runtime.handler_timeout_secs,
            config.runtime.send_timeout_secs
        )?;
        writeln!(
            f,
            "http: connect={}s request={}s max_response={}B proxy={}",
            config.http.connect_timeout_secs,
            config.http.request_timeout_secs,
            config.http.max_response_bytes,
            if config.http.proxy_url().is_some() {
                "configured"
            } else {
                "none"
            }
        )?;
        writeln!(
            f,
            "storage: path={} max_connections={}",
            config.storage.sqlite_path.display(),
            config.storage.max_connections
        )?;
        writeln!(
            f,
            "platforms: telegram={} discord={}",
            config.platforms.telegram.enabled, config.platforms.discord.enabled
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_config(contents: &str) -> tempfile::NamedTempFile {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(contents.as_bytes()).unwrap();
        file.flush().unwrap();
        file
    }

    fn load_with(
        path: PathBuf,
        env: &[(&str, &str)],
        overrides: CliOverrides,
    ) -> Result<LoadedConfig, ConfigError> {
        load(LoadOptions {
            path,
            env: env
                .iter()
                .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
                .collect(),
            compiled: CompiledPlatforms {
                telegram: true,
                discord: true,
            },
            overrides,
        })
    }

    const ENABLED_TELEGRAM: &str = r#"
[platforms.telegram]
enabled = true
token = "123:fake-token"
"#;

    #[test]
    fn missing_file_uses_defaults_and_check_config_does_not_require_a_platform() {
        let loaded = load_with(
            PathBuf::from("/definitely/missing.toml"),
            &[],
            CliOverrides::default(),
        )
        .unwrap();

        assert_eq!(loaded.config.runtime.inbound_queue_capacity, 256);
        assert!(loaded.config.require_platform().is_err());
    }

    #[test]
    fn missing_file_with_enabled_platform_uses_defaults() {
        let loaded = load_with(
            PathBuf::from("/definitely/missing.toml"),
            &[
                (
                    "LUKOSBOT__PLATFORMS__TELEGRAM__TOKEN",
                    "123:from-environment",
                ),
                ("LUKOSBOT__PLATFORMS__TELEGRAM__ENABLED", "true"),
            ],
            CliOverrides::default(),
        )
        .unwrap();

        assert_eq!(loaded.config.runtime.inbound_queue_capacity, 256);
        assert!(loaded.config.platforms.telegram.enabled);
        assert_eq!(
            loaded.config.platforms.telegram.token(),
            Some("123:from-environment")
        );
    }

    #[test]
    fn file_values_override_defaults() {
        let file = write_config(
            r#"
[app]
environment = "production"

[runtime]
inbound_queue_capacity = 10

[platforms.telegram]
enabled = true
token = "1:file"
"#,
        );

        let loaded = load_with(file.path().to_owned(), &[], CliOverrides::default()).unwrap();

        assert_eq!(loaded.config.app.environment, Environment::Production);
        assert_eq!(loaded.config.runtime.inbound_queue_capacity, 10);
    }

    #[test]
    fn environment_overrides_file() {
        let file = write_config(
            r#"
[runtime]
inbound_queue_capacity = 10
"#,
        );

        let loaded = load_with(
            file.path().to_owned(),
            &[
                ("LUKOSBOT__RUNTIME__INBOUND_QUEUE_CAPACITY", "7"),
                ("LUKOSBOT__PLATFORMS__TELEGRAM__ENABLED", "true"),
                ("LUKOSBOT__PLATFORMS__TELEGRAM__TOKEN", "1:env-token"),
            ],
            CliOverrides::default(),
        )
        .unwrap();

        assert_eq!(loaded.config.runtime.inbound_queue_capacity, 7);
    }

    #[test]
    fn cli_log_filter_overrides_environment() {
        let loaded = load_with(
            PathBuf::from("/definitely/missing.toml"),
            &[
                ("LUKOSBOT__LOGGING__FILTER", "debug"),
                ("LUKOSBOT__PLATFORMS__TELEGRAM__ENABLED", "true"),
                ("LUKOSBOT__PLATFORMS__TELEGRAM__TOKEN", "1:token"),
            ],
            CliOverrides {
                log_filter: Some("warn".to_owned()),
            },
        )
        .unwrap();

        assert_eq!(loaded.config.logging.filter, "warn");
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let file = write_config(
            r#"
[app]
environment = "development"
typo_field = 3

[platforms.telegram]
enabled = true
token = "1:token"
"#,
        );

        let error = load_with(file.path().to_owned(), &[], CliOverrides::default()).unwrap_err();
        assert!(matches!(error, ConfigError::Load(_)));
    }

    #[test]
    fn enabled_platform_requires_token() {
        let error = load_with(
            PathBuf::from("/definitely/missing.toml"),
            &[("LUKOSBOT__PLATFORMS__TELEGRAM__ENABLED", "true")],
            CliOverrides::default(),
        )
        .unwrap_err();

        assert!(
            error
                .issues()
                .iter()
                .any(|issue| issue.contains("token is required"))
        );
    }

    #[test]
    fn enabled_platform_must_be_compiled_in() {
        let file = write_config(ENABLED_TELEGRAM);
        let error = load(LoadOptions {
            path: file.path().to_owned(),
            env: HashMap::new(),
            compiled: CompiledPlatforms {
                telegram: false,
                discord: false,
            },
            overrides: CliOverrides::default(),
        })
        .unwrap_err();

        assert!(
            error
                .issues()
                .iter()
                .any(|issue| issue.contains("build feature"))
        );
    }

    #[test]
    fn numeric_token_from_environment_is_accepted() {
        let loaded = load_with(
            PathBuf::from("/definitely/missing.toml"),
            &[
                ("LUKOSBOT__PLATFORMS__TELEGRAM__ENABLED", "true"),
                ("LUKOSBOT__PLATFORMS__TELEGRAM__TOKEN", "12345"),
            ],
            CliOverrides::default(),
        )
        .unwrap();

        assert_eq!(loaded.config.platforms.telegram.token(), Some("12345"));
    }

    #[test]
    fn invalid_ranges_are_all_reported() {
        let file = write_config(
            r#"
[runtime]
inbound_queue_capacity = 0
outbound_queue_capacity = 0

[platforms.telegram]
enabled = true
token = "1:token"
"#,
        );

        let error = load_with(file.path().to_owned(), &[], CliOverrides::default()).unwrap_err();
        let issues = error.issues();

        assert!(
            issues
                .iter()
                .any(|issue| issue.contains("inbound_queue_capacity"))
        );
        assert!(
            issues
                .iter()
                .any(|issue| issue.contains("outbound_queue_capacity"))
        );
    }

    #[test]
    fn secrets_are_redacted_in_debug_output() {
        let file = write_config(
            r#"
[platforms.telegram]
enabled = true
token = "super-secret-token"
"#,
        );

        let loaded = load_with(file.path().to_owned(), &[], CliOverrides::default()).unwrap();
        let debug = format!("{:?}", loaded.config);

        assert!(!debug.contains("super-secret-token"));
        assert!(debug.contains("REDACTED"));
    }

    #[test]
    fn summary_does_not_contain_secrets() {
        let file = write_config(
            r#"
[platforms.telegram]
enabled = true
token = "super-secret-token"
"#,
        );

        let loaded = load_with(file.path().to_owned(), &[], CliOverrides::default()).unwrap();
        let summary = ConfigSummary(&loaded.config).to_string();

        assert!(!summary.contains("super-secret-token"));
        assert!(summary.contains("telegram=true"));
    }
}
