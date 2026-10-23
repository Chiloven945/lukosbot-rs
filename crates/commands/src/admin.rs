use std::str::FromStr;
use std::sync::Arc;

use async_trait::async_trait;

use lukosbot_command_engine::{
    ArgType, ArgsSpec, CommandContext, CommandDefinition, CommandError, CommandExecutor,
    CommandInvocation, CommandNode, InputSpec, PositionalSpec, SyntaxLine, SyntaxPart,
    UsageMetadata,
};
use lukosbot_core::auth::{
    AdminError, AdminIdentity, AdminMutation, AuthorizationError, AuthorizationService,
    BotAdminService,
};
use lukosbot_domain::{ChatKind, PlatformKind, PlatformUserId};

/// `/admin` command implementation.
pub struct AdminCommand {
    admins: Arc<BotAdminService>,
    authorization: Arc<AuthorizationService>,
}

impl AdminCommand {
    pub fn new(admins: Arc<BotAdminService>, authorization: Arc<AuthorizationService>) -> Self {
        Self {
            admins,
            authorization,
        }
    }

    pub fn into_definition(self) -> CommandDefinition {
        let command: Arc<dyn CommandExecutor> = Arc::new(self);

        CommandDefinition::new(
            "admin",
            "管理机器人管理员并查看当前身份",
            InputSpec::NoArgs,
            Arc::clone(&command),
        )
        .child(CommandNode::leaf(
            "me",
            "查看你的当前身份",
            InputSpec::NoArgs,
            Arc::clone(&command),
        ))
        .child(CommandNode::leaf(
            "list",
            "查看机器人管理员列表",
            InputSpec::NoArgs,
            Arc::clone(&command),
        ))
        .child(CommandNode::leaf(
            "add",
            "添加机器人管理员",
            InputSpec::Args(ArgsSpec {
                positionals: vec![
                    PositionalSpec::new("platform", ArgType::String)
                        .description("平台：telegram / discord"),
                    PositionalSpec::new("userId", ArgType::I64).description("平台用户 ID"),
                ],
                options: Vec::new(),
            }),
            Arc::clone(&command),
        ))
        .child(CommandNode::leaf(
            "remove",
            "移除机器人管理员",
            InputSpec::Args(ArgsSpec {
                positionals: vec![
                    PositionalSpec::new("platform", ArgType::String)
                        .description("平台：telegram / discord"),
                    PositionalSpec::new("userId", ArgType::I64).description("平台用户 ID"),
                ],
                options: Vec::new(),
            }),
            command,
        ))
        .usage(UsageMetadata {
            syntaxes: vec![
                SyntaxLine::described("查看你的当前身份", [SyntaxPart::Literal("me".to_owned())]),
                SyntaxLine::described(
                    "查看机器人管理员列表",
                    [SyntaxPart::Literal("list".to_owned())],
                ),
                SyntaxLine::described(
                    "添加机器人管理员",
                    [
                        SyntaxPart::Literal("add".to_owned()),
                        SyntaxPart::Argument {
                            name: "platform".to_owned(),
                            optional: false,
                        },
                        SyntaxPart::Argument {
                            name: "userId".to_owned(),
                            optional: false,
                        },
                    ],
                ),
                SyntaxLine::described(
                    "移除机器人管理员",
                    [
                        SyntaxPart::Literal("remove".to_owned()),
                        SyntaxPart::Argument {
                            name: "platform".to_owned(),
                            optional: false,
                        },
                        SyntaxPart::Argument {
                            name: "userId".to_owned(),
                            optional: false,
                        },
                    ],
                ),
            ],
            examples: vec![
                "admin me".to_owned(),
                "admin list".to_owned(),
                "admin add telegram 123456789".to_owned(),
                "admin remove discord 987654321".to_owned(),
            ],
            notes: vec!["list/add/remove 仅机器人管理员可用。".to_owned()],
        })
    }

    async fn usage(&self, context: &CommandContext<'_>) -> Result<(), CommandError> {
        context
            .reply_text(format!(
                "机器人管理员管理\n\n用法：\n  {}admin me\n  {}admin list\n  {}admin add <platform> <userId>\n  {}admin remove <platform> <userId>\n\n发送 {}admin me 查看当前身份。",
                context.prefix(),
                context.prefix(),
                context.prefix(),
                context.prefix(),
                context.prefix()
            ))
            .await?;
        Ok(())
    }

    async fn me(&self, context: &CommandContext<'_>) -> Result<(), CommandError> {
        let auth = match self.authorization.inspect(context.message()).await {
            Ok(auth) => auth,
            Err(error) => return Err(map_auth_error(error, "查看当前身份")),
        };

        let chat_id = context.address().chat_id.as_str();
        let is_group = matches!(
            context.address().kind,
            ChatKind::Group | ChatKind::Supergroup | ChatKind::Channel
        );

        context
            .reply_text(format!(
                "平台：{}\n用户 ID：{}\n聊天 ID：{}\n群聊：{}\n机器人管理员：{}\n聊天管理员：{}",
                context.address().platform.as_str(),
                context.sender().id.as_str(),
                chat_id,
                if is_group { "是" } else { "否" },
                if auth.bot_admin { "是" } else { "否" },
                if auth.chat_admin { "是" } else { "否" }
            ))
            .await?;
        Ok(())
    }

    async fn list(&self, context: &CommandContext<'_>) -> Result<(), CommandError> {
        if let Err(error) = self
            .authorization
            .require_bot_admin(context.message())
            .await
        {
            return Err(map_auth_error(error, "查看机器人管理员列表"));
        }

        let admins = self
            .admins
            .list_effective_admins()
            .await
            .map_err(map_admin_error)?;

        let mut lines = String::from("当前有效的机器人管理员：\n");
        let mut platforms: Vec<_> = admins.into_iter().collect();
        platforms.sort_by_key(|(platform, _)| platform.as_str());

        for (platform, ids) in platforms {
            let values = if ids.is_empty() {
                "无".to_owned()
            } else {
                ids.iter()
                    .map(PlatformUserId::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            lines.push_str(&format!("- {}: {values}\n", platform.as_str()));
        }

        context.reply_text(lines.trim_end()).await?;
        Ok(())
    }

    async fn add(
        &self,
        context: &CommandContext<'_>,
        invocation: &CommandInvocation,
    ) -> Result<(), CommandError> {
        if let Err(error) = self
            .authorization
            .require_bot_admin(context.message())
            .await
        {
            return Err(map_auth_error(error, "添加机器人管理员"));
        }

        let (platform, user_id) = parse_admin_args(invocation)?;
        let identity = AdminIdentity::new(platform, user_id.clone());
        let outcome = self
            .admins
            .add_dynamic_admin(identity)
            .await
            .map_err(map_admin_error)?;

        let message = match outcome {
            AdminMutation::Added => format!(
                "已添加机器人管理员：{}:{}",
                platform.as_str(),
                user_id.as_str()
            ),
            AdminMutation::AlreadyPresent => format!(
                "该用户已经是动态机器人管理员：{}:{}",
                platform.as_str(),
                user_id.as_str()
            ),
            AdminMutation::Removed | AdminMutation::NotPresent => {
                return Err(CommandError::internal(
                    "unexpected mutation result while adding an administrator",
                ));
            }
        };

        context.reply_text(message).await?;
        Ok(())
    }

    async fn remove(
        &self,
        context: &CommandContext<'_>,
        invocation: &CommandInvocation,
    ) -> Result<(), CommandError> {
        if let Err(error) = self
            .authorization
            .require_bot_admin(context.message())
            .await
        {
            return Err(map_auth_error(error, "移除机器人管理员"));
        }

        let (platform, user_id) = parse_admin_args(invocation)?;
        let identity = AdminIdentity::new(platform, user_id.clone());
        let outcome = self
            .admins
            .remove_dynamic_admin(identity)
            .await
            .map_err(map_admin_error)?;

        let message = match outcome {
            AdminMutation::Removed => format!(
                "已移除动态机器人管理员：{}:{}",
                platform.as_str(),
                user_id.as_str()
            ),
            AdminMutation::NotPresent => format!(
                "该用户不在动态管理员列表中：{}:{}（bootstrap 管理员不会受影响）。",
                platform.as_str(),
                user_id.as_str()
            ),
            AdminMutation::Added | AdminMutation::AlreadyPresent => {
                return Err(CommandError::internal(
                    "unexpected mutation result while removing an administrator",
                ));
            }
        };

        context.reply_text(message).await?;
        Ok(())
    }
}

#[async_trait]
impl CommandExecutor for AdminCommand {
    async fn execute(
        &self,
        context: &CommandContext<'_>,
        invocation: CommandInvocation,
    ) -> Result<(), CommandError> {
        match invocation.path.as_slice() {
            [root] if root == "admin" => self.usage(context).await,
            [root, sub] if root == "admin" => match sub.as_str() {
                "me" => self.me(context).await,
                "list" => self.list(context).await,
                "add" => self.add(context, &invocation).await,
                "remove" => self.remove(context, &invocation).await,
                _ => Err(CommandError::internal("unknown admin subcommand")),
            },
            _ => Err(CommandError::internal("invalid admin command path")),
        }
    }
}

fn parse_admin_args(
    invocation: &CommandInvocation,
) -> Result<(PlatformKind, PlatformUserId), CommandError> {
    let platform_raw = invocation
        .args
        .get::<String>("platform")
        .map_err(CommandError::internal)?;
    let user_id = invocation
        .args
        .get::<i64>("userId")
        .map_err(CommandError::internal)?;

    let platform = PlatformKind::from_str(platform_raw.trim().to_ascii_lowercase().as_str())
        .map_err(|_| CommandError::UserInput(format!("不支持的平台：{platform_raw}")))?;
    let user_id = PlatformUserId::new(user_id.to_string())
        .map_err(|error| CommandError::UserInput(format!("无效的用户 ID：{error}")))?;

    Ok((platform, user_id))
}

pub(crate) fn map_auth_error(error: AuthorizationError, action: &str) -> CommandError {
    match error {
        AuthorizationError::Unauthorized => CommandError::UserInput(format!(
            "权限不足：只有机器人管理员可以{action}。发送 `/admin me` 可查看当前身份。"
        )),
        AuthorizationError::ChatAdminTimeout | AuthorizationError::ChatAdminUnavailable(_) => {
            tracing::warn!(%error, "chat administrator lookup failed");
            CommandError::UserInput("暂时无法验证权限，请稍后重试。".to_owned())
        }
        AuthorizationError::Admin(admin_error) => map_admin_error(admin_error),
        error => {
            tracing::warn!(%error, "authorization check failed");
            CommandError::UserInput("暂时无法验证权限，请稍后重试。".to_owned())
        }
    }
}

fn map_admin_error(error: AdminError) -> CommandError {
    match error {
        AdminError::LastAdminProtection => CommandError::UserInput(
            "无法移除最后一个机器人管理员；请先添加其他管理员或修改配置。".to_owned(),
        ),
        AdminError::Storage(_) | AdminError::CorruptAdminList => {
            tracing::warn!(%error, "bot administrator storage operation failed");
            CommandError::UserInput("暂时无法读取或保存管理员列表，请稍后重试。".to_owned())
        }
        error => {
            tracing::warn!(%error, "bot administrator operation failed");
            CommandError::UserInput("管理员操作暂时不可用，请稍后重试。".to_owned())
        }
    }
}

pub fn definition(
    admins: Arc<BotAdminService>,
    authorization: Arc<AuthorizationService>,
) -> CommandDefinition {
    AdminCommand::new(admins, authorization).into_definition()
}
