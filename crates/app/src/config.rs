//! Typed application configuration.
//!
//! Configuration is layered as `CLI > environment > TOML file > defaults`. Environment keys use
//! the `LUKOSBOT` prefix and `__` as the nesting separator, for example
//! `LUKOSBOT__RUNTIME__INBOUND_QUEUE_CAPACITY=128`.
//!
//! Configuration is only read and validated at startup. There is no hot reload and the file is
//! never rewritten by the application.

mod integrations;
mod load;
mod platforms;
mod policy;
mod secret;
mod sections;
mod security;
mod summary;
mod validate;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_integrations;

/// Environment variable prefix used for configuration overrides.
pub const ENV_PREFIX: &str = "LUKOSBOT";
/// Separator used between nested keys in environment variables.
pub const ENV_SEPARATOR: &str = "__";

pub use integrations::{
    GitHubSettings, IntegrationsSection, MusicSettings, SoundCloudSettings, SpotifySettings,
    WhoisSettings,
};
pub use load::{
    CliOverrides, CompiledPlatforms, ConfigError, ConfigIssues, LoadOptions, LoadedConfig, load,
};
pub use platforms::{DiscordSection, PlatformsSection, TelegramSection};
pub use policy::{PolicyMatchConfig, PolicyRuleConfig, PolicySection};
pub use sections::{
    AppConfig, AppSection, BasicCommandSettings, CommandsSection, ControlCommandSettings,
    Environment, HttpSection, IntegrationCommandSettings, LogFormat, LoggingSection,
    RuntimeSection, StorageSection, TwentyFourSettings,
};
pub use security::SecuritySection;
pub use summary::ConfigSummary;
