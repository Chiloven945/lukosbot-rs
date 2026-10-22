//! Structured logging initialisation.

use tracing_subscriber::EnvFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use crate::config::{LogFormat, LoggingSection};

#[derive(Debug, thiserror::Error)]
pub enum TelemetryError {
    #[error("invalid log filter `{filter}`")]
    InvalidFilter {
        filter: String,
        #[source]
        source: tracing_subscriber::filter::ParseError,
    },

    #[error("a tracing subscriber is already installed")]
    AlreadyInitialised,
}

/// Resolves the effective log filter.
///
/// Precedence is `CLI > RUST_LOG > configuration file`.
pub fn resolve_filter(config: &LoggingSection, cli_filter: Option<&str>) -> String {
    cli_filter
        .map(ToOwned::to_owned)
        .or_else(|| std::env::var("RUST_LOG").ok())
        .unwrap_or_else(|| config.filter.clone())
}

/// Installs the global tracing subscriber.
///
/// Filter precedence is `CLI > RUST_LOG > configuration file`.
pub fn init(config: &LoggingSection, cli_filter: Option<&str>) -> Result<(), TelemetryError> {
    let source = resolve_filter(config, cli_filter);

    let filter = EnvFilter::try_new(&source).map_err(|error| TelemetryError::InvalidFilter {
        filter: source.clone(),
        source: error,
    })?;

    let registry = tracing_subscriber::registry().with(filter);

    let result = match config.format {
        LogFormat::Pretty => registry
            .with(
                tracing_subscriber::fmt::layer()
                    .pretty()
                    .with_target(true)
                    .with_writer(std::io::stderr),
            )
            .try_init(),
        LogFormat::Compact => registry
            .with(
                tracing_subscriber::fmt::layer()
                    .compact()
                    .with_target(true)
                    .with_writer(std::io::stderr),
            )
            .try_init(),
        LogFormat::Json => registry
            .with(
                tracing_subscriber::fmt::layer()
                    .json()
                    .with_target(true)
                    .with_writer(std::io::stderr)
                    .with_current_span(true)
                    .with_span_list(true),
            )
            .try_init(),
    };

    result.map_err(|_| TelemetryError::AlreadyInitialised)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> LoggingSection {
        LoggingSection {
            format: LogFormat::Pretty,
            filter: "info".to_owned(),
        }
    }

    #[test]
    fn cli_filter_has_highest_priority() {
        assert_eq!(resolve_filter(&config(), Some("warn")), "warn");
    }

    #[test]
    fn configuration_filter_is_the_fallback() {
        // Tests should not depend on the ambient RUST_LOG value set by a developer or CI.
        if std::env::var("RUST_LOG").is_err() {
            assert_eq!(resolve_filter(&config(), None), "info");
        }
    }
}
