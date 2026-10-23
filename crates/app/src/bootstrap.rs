//! Dependency assembly: configuration -> infrastructure -> adapters -> runtime.

use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;

use lukosbot_command_engine::CommandMessageHandler;
use lukosbot_commands::{BasicCommandsConfig, RuntimeInfo, TwentyFourService, build_registry};
use lukosbot_core::{AppRuntime, RuntimeSettings, StateStore};
use lukosbot_infra::http::{HttpClient, HttpSettings};
use lukosbot_infra::storage::{SqliteSettings, SqliteStateStore};

use crate::config::{AppConfig, LoadedConfig};

/// A fully assembled application that is ready to run.
///
/// The shared HTTP client is validated at startup (including proxy configuration) and kept alive
/// for future command services. The base runtime does not consume it yet.
pub struct Bootstrapped {
    pub runtime: AppRuntime,
    pub http: Arc<HttpClient>,
}

pub async fn bootstrap(loaded: LoadedConfig) -> anyhow::Result<Bootstrapped> {
    let LoadedConfig { config, .. } = loaded;
    config.require_platform()?;

    let store = SqliteStateStore::connect(&storage_settings(&config))
        .await
        .context("opening the SQLite state store")?;
    let store: Arc<dyn StateStore> = Arc::new(store);

    let http = Arc::new(
        HttpClient::new(http_settings(&config)).context("building the shared HTTP client")?,
    );

    tracing::debug!(
        proxy = config.http.proxy_url().is_some(),
        "shared HTTP client ready"
    );

    let runtime_info = Arc::new(RuntimeInfo::new(env!("CARGO_PKG_VERSION")));
    let (twenty_four_handle, twenty_four_worker) = if config.commands.basic.twenty_four {
        let (handle, worker) = TwentyFourService::with_prefix(
            Duration::from_secs(config.commands.twenty_four.time_limit_secs),
            &config.commands.prefix,
        )
        .into_parts();
        (Some(handle), Some(worker))
    } else {
        (None, None)
    };

    let command_config = BasicCommandsConfig {
        ping: config.commands.basic.ping,
        echo: config.commands.basic.echo,
        coin: config.commands.basic.coin,
        dice: config.commands.basic.dice,
        luck: config.commands.basic.luck,
        twenty_four: config.commands.basic.twenty_four,
        weather: config.commands.basic.weather,
    };
    let registry = build_registry(&command_config, runtime_info, twenty_four_handle)
        .context("building the command registry")?;
    let handler = Arc::new(
        CommandMessageHandler::new(&config.commands.prefix, Arc::new(registry))
            .context("building the command handler")?,
    );

    let builder = AppRuntime::builder(runtime_settings(&config), handler, store);

    let builder = if let Some(worker) = twenty_four_worker {
        builder
            .add_worker(Arc::new(worker))
            .context("registering the 24-point worker")?
    } else {
        builder
    };

    #[cfg(feature = "telegram")]
    let builder = if config.platforms.telegram.enabled {
        let token = config
            .platforms
            .telegram
            .token()
            .expect("configuration validation requires a Telegram token");
        let adapter = lukosbot_platform_telegram::TelegramAdapter::with_http(
            token,
            lukosbot_platform_telegram::TelegramHttpSettings {
                connect_timeout: Duration::from_secs(config.http.connect_timeout_secs),
                request_timeout: Duration::from_secs(config.http.request_timeout_secs),
                proxy: config.http.proxy_url().map(str::to_owned),
            },
        )
        .context("building the Telegram HTTP client")?;

        builder
            .add_platform(Arc::new(adapter))
            .context("registering the Telegram adapter")?
    } else {
        builder
    };

    #[cfg(feature = "discord")]
    let builder = if config.platforms.discord.enabled {
        let token = config
            .platforms
            .discord
            .token()
            .expect("configuration validation requires a Discord token");
        builder
            .add_platform(Arc::new(lukosbot_platform_discord::DiscordAdapter::new(
                token,
            )))
            .context("registering the Discord adapter")?
    } else {
        builder
    };

    // Keep the configuration values used for construction observable in debug logs without
    // logging secrets.
    tracing::debug!(
        inbound_capacity = config.runtime.inbound_queue_capacity,
        outbound_capacity = config.runtime.outbound_queue_capacity,
        "runtime assembly complete"
    );

    Ok(Bootstrapped {
        runtime: builder.build(),
        http,
    })
}

fn runtime_settings(config: &AppConfig) -> RuntimeSettings {
    RuntimeSettings {
        inbound_capacity: config.runtime.inbound_queue_capacity,
        outbound_capacity: config.runtime.outbound_queue_capacity,
        handler_timeout: Duration::from_secs(config.runtime.handler_timeout_secs),
        send_timeout: Duration::from_secs(config.runtime.send_timeout_secs),
        shutdown_timeout: config.shutdown_timeout(),
    }
}

pub fn storage_settings(config: &AppConfig) -> SqliteSettings {
    SqliteSettings {
        path: config.storage.sqlite_path.clone(),
        busy_timeout: Duration::from_millis(config.storage.busy_timeout_ms),
        max_connections: config.storage.max_connections,
        create_parent_dirs: config.storage.create_parent_dirs,
    }
}

fn http_settings(config: &AppConfig) -> HttpSettings {
    HttpSettings {
        connect_timeout: Duration::from_secs(config.http.connect_timeout_secs),
        request_timeout: Duration::from_secs(config.http.request_timeout_secs),
        max_response_bytes: config.http.max_response_bytes,
        user_agent: config.http.user_agent.clone(),
        proxy: config.http.proxy_url().map(str::to_owned),
    }
}

#[cfg(all(test, feature = "telegram"))]
mod tests {
    use super::*;
    use crate::config::{CompiledPlatforms, LoadOptions};
    use std::collections::HashMap;

    #[cfg(feature = "telegram")]
    #[tokio::test]
    async fn bootstrap_opens_the_database_and_registers_enabled_platforms() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("application.toml");
        std::fs::write(
            &config_path,
            format!(
                r#"
                [storage]
                sqlite_path = "{}"

                [platforms.telegram]
                enabled = true
                token = "123:fake-token"
                "#,
                dir.path().join("state.sqlite3").display()
            ),
        )
        .unwrap();

        let loaded = crate::config::load(LoadOptions {
            path: config_path,
            env: HashMap::new(),
            compiled: CompiledPlatforms {
                telegram: true,
                discord: true,
            },
            overrides: Default::default(),
        })
        .unwrap();

        let bootstrapped = bootstrap(loaded).await.unwrap();
        assert!(
            bootstrapped
                .http
                .raw()
                .get("http://example.com")
                .build()
                .is_ok()
        );
    }

    #[cfg(feature = "telegram")]
    #[tokio::test]
    async fn bootstrap_fails_when_the_database_cannot_be_opened() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("application.toml");
        std::fs::write(
            &config_path,
            format!(
                r#"
                [storage]
                sqlite_path = "{}"
                create_parent_dirs = false

                [platforms.telegram]
                enabled = true
                token = "123:fake-token"
                "#,
                dir.path().display()
            ),
        )
        .unwrap();

        let loaded = crate::config::load(LoadOptions {
            path: config_path,
            env: HashMap::new(),
            compiled: CompiledPlatforms {
                telegram: true,
                discord: true,
            },
            overrides: Default::default(),
        })
        .unwrap();

        assert!(bootstrap(loaded).await.is_err());
    }
}
