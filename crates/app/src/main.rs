use std::future::Future;
use std::io::Write;
use std::process::ExitCode;

use anyhow::Context;
use clap::Parser;

use lukosbot_app::cli::{Cli, Command, ConflictMode};
use lukosbot_app::config::{self, CompiledPlatforms, ConfigSummary, LoadOptions};
use lukosbot_app::{bootstrap, shutdown, telemetry};
use lukosbot_core::StateStore;
use lukosbot_infra::storage::{ImportOptions, SqliteStateStore, import_jsonl};

fn main() -> ExitCode {
    run().unwrap_or_else(|error| {
        eprintln!("error: {error:#}");
        ExitCode::FAILURE
    })
}

fn run() -> anyhow::Result<ExitCode> {
    load_dotenv()?;
    let cli = Cli::parse();

    let loaded = config::load(LoadOptions::from_process(
        cli.config.clone(),
        CompiledPlatforms::current(),
        cli.overrides(),
    ))
    .with_context(|| format!("loading configuration from `{}`", cli.config.display()))?;

    telemetry::init(&loaded.config.logging, cli.log_filter.as_deref())
        .context("initialising logging")?;

    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        environment = %loaded.config.app.environment,
        telegram = loaded.config.platforms.telegram.enabled,
        discord = loaded.config.platforms.discord.enabled,
        config = %loaded.path.display(),
        "configuration loaded"
    );

    match cli.command.unwrap_or(Command::Run) {
        Command::CheckConfig => {
            let mut stdout = std::io::stdout().lock();
            write!(stdout, "{}", ConfigSummary(&loaded.config))?;
            stdout.flush()?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Migrate => block_on(migrate(loaded))?,
        Command::ImportState {
            input,
            apply,
            on_conflict,
        } => block_on(import_state(loaded, input, apply, on_conflict))?,
        Command::Run => block_on(run_bot(loaded))?,
    }
}

async fn migrate(loaded: config::LoadedConfig) -> anyhow::Result<ExitCode> {
    let settings = bootstrap::storage_settings(&loaded.config);
    let store = SqliteStateStore::connect(&settings)
        .await
        .context("applying database migrations")?;
    store.close().await.context("closing the state store")?;

    tracing::info!("database migrations applied");
    Ok(ExitCode::SUCCESS)
}

async fn import_state(
    loaded: config::LoadedConfig,
    input: std::path::PathBuf,
    apply: bool,
    on_conflict: ConflictMode,
) -> anyhow::Result<ExitCode> {
    let settings = bootstrap::storage_settings(&loaded.config);
    let store = SqliteStateStore::connect(&settings)
        .await
        .context("opening the target SQLite state store")?;

    let report = import_jsonl(
        &store,
        &input,
        ImportOptions {
            apply,
            on_conflict: on_conflict.into(),
        },
    )
    .await
    .with_context(|| format!("importing state from `{}`", input.display()))?;

    let mut stdout = std::io::stdout().lock();
    writeln!(stdout, "{report}")?;
    stdout.flush()?;

    store.close().await.context("closing the state store")?;
    Ok(ExitCode::SUCCESS)
}

async fn run_bot(loaded: config::LoadedConfig) -> anyhow::Result<ExitCode> {
    let bootstrapped = bootstrap::bootstrap(loaded)
        .await
        .context("assembling the application")?;

    // `http` stays alive for the whole run; future command services will receive it.
    let bootstrap::Bootstrapped { runtime, http } = bootstrapped;

    let result = runtime.run(shutdown::wait_for_signal()).await;
    drop(http);

    result.context("running the application")?;
    Ok(ExitCode::SUCCESS)
}

fn load_dotenv() -> anyhow::Result<()> {
    match dotenvy::dotenv() {
        Ok(_) => Ok(()),
        Err(error) if matches!(&error, dotenvy::Error::Io(io) if io.kind() == std::io::ErrorKind::NotFound) => {
            Ok(())
        }
        Err(error) => Err(error).context("loading .env"),
    }
}

fn block_on<F>(future: F) -> anyhow::Result<F::Output>
where
    F: Future,
{
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("building the Tokio runtime")?;

    Ok(runtime.block_on(future))
}
