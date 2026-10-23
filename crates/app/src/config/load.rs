use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;

use thiserror::Error;

use super::{AppConfig, ENV_PREFIX, ENV_SEPARATOR};

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
    Load(#[from] config::ConfigError),

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
    let mut builder = config::Config::builder();

    if options.path.exists() {
        builder = builder
            .add_source(config::File::from(options.path.clone()).format(config::FileFormat::Toml));
    }

    let environment = config::Environment::with_prefix(ENV_PREFIX)
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
