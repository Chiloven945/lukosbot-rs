use anyhow::Result;
use serenity::all::{
    async_trait, CommandDataOptionValue, Context, EventHandler, GatewayIntents,
    Interaction, Message as DiscordMessage, Ready,
};
use serenity::client::Client;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tokio::sync::{mpsc, Mutex, RwLock};
use tracing::error;

use crate::model::{Address, ChatPlatform, MessageIn};

pub type InSink = mpsc::UnboundedSender<MessageIn>;

pub struct DiscordStack {
    pub(crate) token: String,
    sink: RwLock<Option<InSink>>,
    started: AtomicBool,
    shard_shutdown: Mutex<Option<serenity::gateway::ShardManager>>,
}

impl DiscordStack {
    pub fn new(token: String) -> Arc<Self> {
        Arc::new(Self {
            token,
            sink: RwLock::new(None),
            started: AtomicBool::new(false),
            shard_shutdown: Mutex::new(None),
        })
    }

    pub async fn set_sink(&self, sink: InSink) {
        *self.sink.write().await = Some(sink);
    }

    pub async fn ensure_started(self: &Arc<Self>) -> Result<()> {
        if self.started.swap(true, Ordering::SeqCst) {
            return Ok(());
        }

        let intents = GatewayIntents::GUILD_MESSAGES
            | GatewayIntents::DIRECT_MESSAGES
            | GatewayIntents::MESSAGE_CONTENT;

        let handler = Handler {
            stack: self.clone(),
        };

        let mut client = Client::builder(&self.token, intents)
            .event_handler(handler)
            .await?;

        let mgr = client.shard_manager.clone();
        tokio::spawn(async move {
            if let Err(e) = client.start().await {
                error!("discord client start failed: {e:?}");
            }
        });

        let _ = mgr;

        Ok(())
    }
}

struct Handler {
    stack: Arc<DiscordStack>,
}

#[async_trait]
impl EventHandler for Handler {
    async fn message(&self, _ctx: Context, msg: DiscordMessage) {
        if msg.author.bot {
            return;
        }
        let text = msg.content.trim();
        if text.is_empty() {
            return;
        }

        let is_guild = msg.guild_id.is_some();
        let chat_id = if is_guild {
            msg.channel_id.get() as i64
        } else {
            msg.author.id.get() as i64
        };
        let user_id = msg.author.id.get() as i64;

        if let Some(sink) = self.stack.sink.read().await.as_ref() {
            let _ = sink.send(MessageIn::new(
                Address::new(ChatPlatform::Discord, chat_id, is_guild),
                Some(user_id),
                text.to_string(),
            ));
        }
    }

    async fn ready(&self, _ctx: Context, _ready: Ready) {
        // 你 Java 的 ensureStarted 里会注册 slash commands；这里先留钩子
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        let Some(cmd) = interaction.as_command() else {
            return;
        };
        if cmd.user.bot {
            return;
        }

        let mut text = String::new();
        text.push('/');
        text.push_str(&cmd.data.name);
        for opt in &cmd.data.options {
            text.push(' ');
            text.push_str(&fmt_cmd_value(&opt.value));
        }

        let is_guild = cmd.guild_id.is_some();
        let chat_id = if is_guild {
            cmd.channel_id.get() as i64
        } else {
            cmd.user.id.get() as i64
        };
        let user_id = cmd.user.id.get() as i64;

        if let Some(sink) = self.stack.sink.read().await.as_ref() {
            let _ = sink.send(MessageIn::new(
                Address::new(ChatPlatform::Discord, chat_id, is_guild),
                Some(user_id),
                text,
            ));
        }

        let _ = cmd
            .create_response(
                &ctx.http,
                serenity::all::CreateInteractionResponse::Message(
                    serenity::all::CreateInteractionResponseMessage::new()
                        .content("（推荐直接发送消息）")
                        .ephemeral(true),
                ),
            )
            .await;
    }
}

fn fmt_cmd_value(v: &CommandDataOptionValue) -> String {
    match v {
        CommandDataOptionValue::Autocomplete { value, .. } => value.clone(),
        CommandDataOptionValue::Boolean(b) => b.to_string(),
        CommandDataOptionValue::Integer(i) => i.to_string(),
        CommandDataOptionValue::Number(n) => n.to_string(),
        CommandDataOptionValue::String(s) => s.clone(),

        CommandDataOptionValue::SubCommand(opts)
        | CommandDataOptionValue::SubCommandGroup(opts) => {
            let mut out = String::new();
            for o in opts {
                out.push(' ');
                out.push_str(&fmt_cmd_value(&o.value));
            }
            out.trim().to_string()
        }

        CommandDataOptionValue::Attachment(id) => id.get().to_string(),
        CommandDataOptionValue::Channel(id) => id.get().to_string(),
        CommandDataOptionValue::Mentionable(id) => id.get().to_string(),
        CommandDataOptionValue::Role(id) => id.get().to_string(),
        CommandDataOptionValue::User(id) => id.get().to_string(),

        CommandDataOptionValue::Unknown(u) => format!("unknown({u})"),
        _ => "<unsupported>".to_string(),
    }
}
