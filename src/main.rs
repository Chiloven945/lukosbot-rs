mod config;
mod lifecycle;
mod model;

mod commands;
mod core;
mod platform;

use anyhow::{Context, Result};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use crate::core::{CommandRegistry, MessageDispatcher, MessageSenderHub, PipelineProcessor};
use crate::lifecycle::{BaseCloseable, PlatformGuard};
use crate::platform::{discord::DiscordReceiver, telegram::TelegramReceiver};

#[tokio::main]
async fn main() -> Result<()> {
    // ---- logging init ----
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    info!("lukosbot starting...");
    let boot_t0 = Instant::now();

    // ---- config ----
    let t0 = Instant::now();
    let props = Arc::new(config::load_or_init()?);
    info!(
        "config loaded in {:?} (prefix='{}', telegram_enabled={}, discord_enabled={})",
        t0.elapsed(),
        props.prefix,
        props.telegram.enabled,
        props.discord.enabled,
    );

    // ---- core: hub / registry / pipeline / dispatcher ----
    let t0 = Instant::now();
    let hub = MessageSenderHub::new();
    debug!("MessageSenderHub created");

    let registry = CommandRegistry::build(props.clone());
    info!(
        "CommandRegistry built in {:?} (commands: {})",
        t0.elapsed(),
        registry.list_commands()
    );

    let pipeline = PipelineProcessor::new(props.clone(), registry.clone());
    debug!("PipelineProcessor created");

    let (in_tx, in_rx) = mpsc::unbounded_channel();
    debug!("inbound channel created");

    let dispatcher = Arc::new(MessageDispatcher::new(
        pipeline,
        hub.clone(),
        props.prefix.clone(),
    ));
    info!("MessageDispatcher created");

    // ---- platforms ----
    let mut closeable = BaseCloseable::new();
    let mut enabled_any = false;

    if props.telegram.enabled {
        let t0 = Instant::now();
        enabled_any = true;

        info!("starting TelegramReceiver...");
        let tg = TelegramReceiver::new(props.telegram.bot_token.clone());

        tg.bind(in_tx.clone()).await;
        debug!("TelegramReceiver bind done");

        tg.start().await.context("TelegramReceiver.start failed")?;
        info!("TelegramReceiver started in {:?}", t0.elapsed());

        hub.register(crate::model::ChatPlatform::Telegram, tg.sender().await?);
        debug!("Telegram sender registered into hub");

        closeable.add(Box::new(tg));
        info!("Telegram ready");
    } else {
        info!("Telegram disabled by config");
    }

    if props.discord.enabled {
        let t0 = Instant::now();
        enabled_any = true;

        info!("starting DiscordReceiver...");
        let dc = DiscordReceiver::new(props.discord.token.clone(), props.proxy.clone());

        dc.bind(in_tx.clone()).await;
        debug!("DiscordReceiver bind done");

        dc.start().await.context("DiscordReceiver.start failed")?;
        info!("DiscordReceiver started in {:?}", t0.elapsed());

        hub.register(crate::model::ChatPlatform::Discord, dc.sender().await?);
        debug!("Discord sender registered into hub");

        closeable.add(Box::new(dc));
        info!("Discord ready");
    } else {
        info!("Discord disabled by config");
    }

    PlatformGuard::ensure(enabled_any).context("no platform enabled")?;
    info!("platform guard ok (enabled_any={})", enabled_any);

    // ---- dispatcher task ----
    info!("spawning dispatcher loop...");
    let dispatcher_task = {
        let dispatcher = dispatcher.clone();
        tokio::spawn(async move {
            info!("dispatcher loop started");
            dispatcher.run(in_rx).await;
            info!("dispatcher loop exited");
        })
    };

    info!("boot completed in {:?}", boot_t0.elapsed());

    // ---- shutdown ----
    tokio::signal::ctrl_c().await?;
    warn!("Ctrl+C received, shutting down...");

    info!("closing platforms (BaseCloseable)...");
    closeable.close();
    info!("platforms closed");

    info!("stopping dispatcher...");
    dispatcher.stop();
    info!("dispatcher stop signaled");

    match dispatcher_task.await {
        Ok(_) => info!("dispatcher task joined"),
        Err(e) => error!("dispatcher task join error: {e:?}"),
    }

    info!("shutdown complete");
    Ok(())
}
