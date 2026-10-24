use std::sync::Arc;

use async_trait::async_trait;

use lukosbot_command_engine::{
    ArgType, ArgValue, ArgsSpec, CommandContext, CommandDefinition, CommandError, CommandExecutor,
    CommandInvocation, CommandNode, InputSpec, PositionalSpec, SyntaxLine, SyntaxPart,
    UsageMetadata,
};
use lukosbot_core::auth::AuthorizationService;
use lukosbot_core::ports::{MediaLoadError, MediaLoadPolicy};
use lukosbot_domain::{ChatAddress, InboundMessage, PlatformKind};

use crate::admin::map_auth_error;
use crate::media::cave::service::{CaveError, CaveService};

const DISCORD_MEDIA_HOSTS: &[&str] = &["cdn.discordapp.com", "media.discordapp.net"];

pub struct CaveCommand {
    service: Arc<CaveService>,
    authorization: Arc<AuthorizationService>,
    max_image_bytes: u64,
}

impl CaveCommand {
    pub fn new(
        service: Arc<CaveService>,
        authorization: Arc<AuthorizationService>,
        max_image_bytes: u64,
    ) -> Self {
        Self {
            service,
            authorization,
            max_image_bytes,
        }
    }

    pub fn into_definition(self) -> CommandDefinition {
        let command: Arc<dyn CommandExecutor> = Arc::new(self);

        CommandDefinition::new(
            "cave",
            "回声洞：保存并随机发送文本或图片",
            InputSpec::Args(ArgsSpec {
                positionals: vec![
                    PositionalSpec::new("number", ArgType::I64)
                        .optional()
                        .default(ArgValue::I64(0))
                        .description("条目编号；省略时随机回放"),
                ],
                options: Vec::new(),
            }),
            Arc::clone(&command),
        )
        .alias("c")
        .child(CommandNode::leaf(
            "add",
            "添加条目",
            InputSpec::Raw {
                name: "message".to_owned(),
                required: false,
            },
            Arc::clone(&command),
        ))
        .child(CommandNode::leaf(
            "delete",
            "删除指定编号条目",
            InputSpec::Args(ArgsSpec {
                positionals: vec![
                    PositionalSpec::new("number", ArgType::I64)
                        .description("正整数编号")
                        .validator(lukosbot_command_engine::Validator::new(
                            |value| match value {
                                ArgValue::I64(value) if *value > 0 => Ok(()),
                                _ => Err("编号必须是正整数。".to_owned()),
                            },
                        )),
                ],
                options: Vec::new(),
            }),
            command,
        ))
        .usage(UsageMetadata {
            syntaxes: vec![
                SyntaxLine::described("随机回放一个条目", []),
                SyntaxLine::described(
                    "回放指定编号",
                    [SyntaxPart::Argument {
                        name: "number".to_owned(),
                        optional: false,
                    }],
                ),
                SyntaxLine::described(
                    "添加当前消息或引用消息",
                    [SyntaxPart::Literal("add".to_owned())],
                ),
                SyntaxLine::described(
                    "删除指定编号",
                    [
                        SyntaxPart::Literal("delete".to_owned()),
                        SyntaxPart::Argument {
                            name: "number".to_owned(),
                            optional: false,
                        },
                    ],
                ),
            ],
            examples: vec![
                "cave".to_owned(),
                "cave 12".to_owned(),
                "cave add".to_owned(),
                "cave delete 12".to_owned(),
            ],
            notes: vec!["add 和 delete 仅机器人管理员可用。".to_owned()],
        })
    }

    async fn random(&self, context: &CommandContext<'_>) -> Result<(), CommandError> {
        let Some(entry) = self.service.random().await.map_err(map_cave_error)? else {
            context.reply_text("还没有任何回声洞条目。").await?;
            return Ok(());
        };
        self.reply_entry(context, &entry).await
    }

    async fn by_number(&self, context: &CommandContext<'_>, no: u32) -> Result<(), CommandError> {
        let Some(entry) = self.service.get(no).await.map_err(map_cave_error)? else {
            context.reply_text(format!("编号 #{no} 不存在。")).await?;
            return Ok(());
        };
        self.reply_entry(context, &entry).await
    }

    async fn reply_entry(
        &self,
        context: &CommandContext<'_>,
        entry: &crate::media::cave::model::CaveEntry,
    ) -> Result<(), CommandError> {
        let outbound = self
            .service
            .to_outbound(context.address(), entry)
            .map_err(map_cave_error)?;
        context.reply_message(outbound).await?;
        Ok(())
    }

    async fn add(
        &self,
        context: &CommandContext<'_>,
        invocation: &CommandInvocation,
    ) -> Result<(), CommandError> {
        self.authorization
            .require_bot_admin(context.message())
            .await
            .map_err(|error| map_auth_error(error, "添加回声洞条目"))?;

        let current_text = current_text(
            context.message(),
            invocation.raw_tail.trim(),
            context.prefix(),
        );
        let policy = media_policy(context.address(), self.max_image_bytes);
        let entry = self
            .service
            .add(
                context.message(),
                current_text.as_deref(),
                context.media().as_ref(),
                policy,
            )
            .await
            .map_err(map_cave_error)?;

        context
            .reply_text(format!("已添加回声洞条目 #{}。", entry.no))
            .await?;
        Ok(())
    }

    async fn delete(
        &self,
        context: &CommandContext<'_>,
        invocation: &CommandInvocation,
    ) -> Result<(), CommandError> {
        self.authorization
            .require_bot_admin(context.message())
            .await
            .map_err(|error| map_auth_error(error, "删除回声洞条目"))?;
        let no = invocation
            .args
            .get::<i64>("number")
            .map_err(CommandError::internal)?;
        let no = u32::try_from(no)
            .map_err(|_| CommandError::UserInput("编号必须是正整数。".to_owned()))?;
        if self.service.delete(no).await.map_err(map_cave_error)? {
            context
                .reply_text(format!("已删除回声洞条目 #{no}。"))
                .await?;
        } else {
            context.reply_text(format!("编号 #{no} 不存在。")).await?;
        }
        Ok(())
    }
}

#[async_trait]
impl CommandExecutor for CaveCommand {
    async fn execute(
        &self,
        context: &CommandContext<'_>,
        invocation: CommandInvocation,
    ) -> Result<(), CommandError> {
        match invocation.path.as_slice() {
            [root] if root == "cave" => {
                let no = invocation
                    .args
                    .get::<i64>("number")
                    .map_err(CommandError::internal)?;
                if no <= 0 {
                    self.random(context).await
                } else {
                    self.by_number(context, u32::try_from(no).unwrap_or(u32::MAX))
                        .await
                }
            }
            [root, sub] if root == "cave" => match sub.as_str() {
                "add" => self.add(context, &invocation).await,
                "delete" => self.delete(context, &invocation).await,
                _ => Err(CommandError::internal("unknown cave subcommand")),
            },
            _ => Err(CommandError::internal("invalid cave command path")),
        }
    }
}

fn media_policy(address: &ChatAddress, max_bytes: u64) -> MediaLoadPolicy {
    if address.platform == PlatformKind::Discord {
        MediaLoadPolicy::new(DISCORD_MEDIA_HOSTS, max_bytes)
    } else {
        MediaLoadPolicy::platform(max_bytes)
    }
}

fn current_text(message: &InboundMessage, raw_tail: &str, prefix: &str) -> Option<String> {
    if !raw_tail.is_empty() {
        return Some(raw_tail.to_owned());
    }

    let text = message
        .text_parts()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    let stripped = strip_cave_prefix(&text, prefix);
    (!stripped.is_empty()).then_some(stripped)
}

fn strip_cave_prefix(text: &str, prefix: &str) -> String {
    let mut lines = text.lines();
    let Some(first) = lines.next() else {
        return String::new();
    };
    let lower = first.to_ascii_lowercase();
    let cave = format!("{}cave add", prefix).to_ascii_lowercase();
    let c = format!("{}c add", prefix).to_ascii_lowercase();
    let stripped = if lower.starts_with(&cave) {
        first[cave.len()..].trim_start()
    } else if lower.starts_with(&c) {
        first[c.len()..].trim_start()
    } else {
        first
    };
    std::iter::once(stripped)
        .chain(lines)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned()
}

fn map_cave_error(error: CaveError) -> CommandError {
    match error {
        CaveError::Empty => CommandError::UserInput("未检测到可保存的文本或图片。".to_owned()),
        CaveError::InvalidImage => {
            CommandError::UserInput("回声洞图片数据损坏，无法回放。".to_owned())
        }
        CaveError::Media(MediaLoadError::TooLarge { .. }) => {
            CommandError::UserInput("图片过大，未保存。".to_owned())
        }
        error => {
            tracing::warn!(%error, "cave operation failed");
            CommandError::UserInput("回声洞服务暂时不可用，请稍后重试。".to_owned())
        }
    }
}
