//! Process-level command line interface.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::config::CliOverrides;

/// LukosBot process entry point.
#[derive(Debug, Parser)]
#[command(
    name = "lukosbot",
    version,
    about = "A multiplatform chatbot built on a typed, testable runtime"
)]
pub struct Cli {
    /// Path to the TOML configuration file.
    #[arg(
        long,
        global = true,
        env = "LUKOSBOT_CONFIG",
        default_value = "config/application.toml",
        value_name = "PATH"
    )]
    pub config: PathBuf,

    /// Override the log filter (highest priority).
    #[arg(long, global = true, env = "LUKOSBOT_LOG", value_name = "FILTER")]
    pub log_filter: Option<String>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Start the bot and connect to the enabled platforms.
    Run,
    /// Load and validate configuration without connecting to any platform.
    CheckConfig,
    /// Apply database schema migrations and exit.
    Migrate,
}

impl Cli {
    pub fn overrides(&self) -> CliOverrides {
        CliOverrides {
            log_filter: self.log_filter.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_run_with_default_config_path() {
        let cli = Cli::try_parse_from(["lukosbot", "run"]).unwrap();
        assert!(matches!(cli.command, Command::Run));
        assert_eq!(cli.config, PathBuf::from("config/application.toml"));
    }

    #[test]
    fn parses_global_options_after_subcommand() {
        let cli = Cli::try_parse_from([
            "lukosbot",
            "check-config",
            "--config",
            "custom.toml",
            "--log-filter",
            "debug",
        ])
        .unwrap();

        assert!(matches!(cli.command, Command::CheckConfig));
        assert_eq!(cli.config, PathBuf::from("custom.toml"));
        assert_eq!(cli.log_filter.as_deref(), Some("debug"));
    }

    #[test]
    fn rejects_unknown_subcommands() {
        assert!(Cli::try_parse_from(["lukosbot", "unknown"]).is_err());
    }
}
