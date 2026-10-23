//! Process-level command line interface.

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

use lukosbot_infra::storage::ImportConflictMode;

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

    /// Subcommand to execute. Omitting it is equivalent to `run`.
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Start the bot and connect to the enabled platforms.
    Run,
    /// Load and validate configuration without connecting to any platform.
    CheckConfig,
    /// Apply database schema migrations and exit.
    Migrate,
    /// Import a JVM bot_state JSONL export. Dry-run unless --apply is given.
    ImportState {
        /// Path to the JSONL export.
        #[arg(long, value_name = "PATH")]
        input: PathBuf,

        /// Write records to SQLite. Without this flag the command is a dry-run.
        #[arg(long)]
        apply: bool,

        /// How to handle records whose key already exists in the target database.
        #[arg(long, value_enum, default_value = "fail")]
        on_conflict: ConflictMode,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ConflictMode {
    Fail,
    Skip,
    Replace,
}

impl From<ConflictMode> for ImportConflictMode {
    fn from(value: ConflictMode) -> Self {
        match value {
            ConflictMode::Fail => Self::Fail,
            ConflictMode::Skip => Self::Skip,
            ConflictMode::Replace => Self::Replace,
        }
    }
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
        assert!(matches!(cli.command, Some(Command::Run)));
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

        assert!(matches!(cli.command, Some(Command::CheckConfig)));
        assert_eq!(cli.config, PathBuf::from("custom.toml"));
        assert_eq!(cli.log_filter.as_deref(), Some("debug"));
    }

    #[test]
    fn missing_subcommand_defaults_to_run() {
        let cli = Cli::try_parse_from(["lukosbot"]).unwrap();
        assert!(cli.command.is_none());
    }

    #[test]
    fn rejects_unknown_subcommands() {
        assert!(Cli::try_parse_from(["lukosbot", "unknown"]).is_err());
    }

    #[test]
    fn parses_dry_run_import_by_default() {
        let cli =
            Cli::try_parse_from(["lukosbot", "import-state", "--input", "state.jsonl"]).unwrap();
        let Some(Command::ImportState {
            input,
            apply,
            on_conflict,
        }) = cli.command
        else {
            panic!("expected import-state");
        };
        assert_eq!(input, PathBuf::from("state.jsonl"));
        assert!(!apply);
        assert_eq!(on_conflict, ConflictMode::Fail);
    }

    #[test]
    fn parses_apply_import_with_conflict_mode() {
        let cli = Cli::try_parse_from([
            "lukosbot",
            "import-state",
            "--input",
            "state.jsonl",
            "--apply",
            "--on-conflict",
            "replace",
        ])
        .unwrap();
        let Some(Command::ImportState {
            apply, on_conflict, ..
        }) = cli.command
        else {
            panic!("expected import-state");
        };
        assert!(apply);
        assert_eq!(on_conflict, ConflictMode::Replace);
    }
}
