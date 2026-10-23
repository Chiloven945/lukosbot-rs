use std::sync::Arc;

use async_trait::async_trait;

use lukosbot_command_engine::{
    ArgType, ArgsSpec, CommandContext, CommandDefinition, CommandError, CommandExecutor,
    CommandInvocation, CommandNode, InputSpec, PositionalSpec, SyntaxLine, SyntaxPart,
    UsageMetadata,
};
use lukosbot_core::StateScope;
use lukosbot_core::auth::AuthorizationService;
use lukosbot_core::state::{StateDefinition, StateService, StateServiceError};
use lukosbot_domain::{ChatAddress, PlatformUserId};

use crate::admin::map_auth_error;

/// `/pref` command implementation.
pub struct PrefCommand {
    states: Arc<StateService>,
    authorization: Arc<AuthorizationService>,
}

impl PrefCommand {
    pub fn new(states: Arc<StateService>, authorization: Arc<AuthorizationService>) -> Self {
        Self {
            states,
            authorization,
        }
    }

    pub fn into_definition(self) -> CommandDefinition {
        let command: Arc<dyn CommandExecutor> = Arc::new(self);

        CommandDefinition::new(
            "pref",
            "查看和管理偏好设置",
            InputSpec::NoArgs,
            Arc::clone(&command),
        )
        .child(CommandNode::leaf(
            "list",
            "列出可用配置项",
            InputSpec::NoArgs,
            Arc::clone(&command),
        ))
        .child(CommandNode::leaf(
            "get",
            "查看配置项",
            InputSpec::Args(ArgsSpec {
                positionals: vec![
                    PositionalSpec::new("scope", ArgType::String)
                        .optional()
                        .description("user / chat / global"),
                    PositionalSpec::new("state", ArgType::String).description("配置项名称"),
                ],
                options: Vec::new(),
            }),
            Arc::clone(&command),
        ))
        .child(CommandNode::leaf(
            "set",
            "设置配置项",
            InputSpec::Args(ArgsSpec {
                positionals: vec![
                    PositionalSpec::new("scope", ArgType::String)
                        .description("user / chat / global"),
                    PositionalSpec::new("state", ArgType::String).description("配置项名称"),
                    PositionalSpec::new("value", ArgType::String)
                        .greedy()
                        .description("配置值"),
                ],
                options: Vec::new(),
            }),
            Arc::clone(&command),
        ))
        .child(CommandNode::leaf(
            "clear",
            "清除配置项",
            InputSpec::Args(ArgsSpec {
                positionals: vec![
                    PositionalSpec::new("scope", ArgType::String)
                        .description("user / chat / global"),
                    PositionalSpec::new("state", ArgType::String).description("配置项名称"),
                ],
                options: Vec::new(),
            }),
            command,
        ))
        .usage(UsageMetadata {
            syntaxes: vec![
                SyntaxLine::described("列出可用配置项", []),
                SyntaxLine::described(
                    "查看配置项（不指定 scope 时按优先级解析）",
                    [
                        SyntaxPart::Argument {
                            name: "scope".to_owned(),
                            optional: true,
                        },
                        SyntaxPart::Argument {
                            name: "state".to_owned(),
                            optional: false,
                        },
                    ],
                ),
                SyntaxLine::described(
                    "设置配置项",
                    [
                        SyntaxPart::Literal("set".to_owned()),
                        SyntaxPart::Argument {
                            name: "scope".to_owned(),
                            optional: false,
                        },
                        SyntaxPart::Argument {
                            name: "state".to_owned(),
                            optional: false,
                        },
                        SyntaxPart::Argument {
                            name: "value".to_owned(),
                            optional: false,
                        },
                    ],
                ),
                SyntaxLine::described(
                    "清除配置项",
                    [
                        SyntaxPart::Literal("clear".to_owned()),
                        SyntaxPart::Argument {
                            name: "scope".to_owned(),
                            optional: false,
                        },
                        SyntaxPart::Argument {
                            name: "state".to_owned(),
                            optional: false,
                        },
                    ],
                ),
            ],
            examples: vec![
                "pref list".to_owned(),
                "pref get lang".to_owned(),
                "pref set user lang en-us".to_owned(),
                "pref clear global notifyMode".to_owned(),
            ],
            notes: vec![
                "scope 只能是 user、chat 或 global。".to_owned(),
                "user 范围只能修改自己的设置；chat 需要聊天管理员；global 需要机器人管理员。"
                    .to_owned(),
            ],
        })
    }

    async fn list(&self, context: &CommandContext<'_>) -> Result<(), CommandError> {
        let registry = self.states.registry();
        let definitions: Vec<&dyn StateDefinition> = registry.all().collect();

        if definitions.is_empty() {
            context.reply_text("暂无可用配置项。").await?;
            return Ok(());
        }

        let mut lines = String::from("可用的配置项：\n");
        for definition in definitions {
            let mut scopes = definition
                .allowed_scopes()
                .iter()
                .map(|scope| scope_display(*scope))
                .collect::<Vec<_>>();
            scopes.sort_unstable();
            let scopes = scopes.join("/");
            let order = definition
                .resolve_order()
                .iter()
                .map(|scope| scope_display(*scope))
                .collect::<Vec<_>>()
                .join(" -> ");

            lines.push_str(&format!(
                "- {}：{}；可用范围：{scopes}；生效优先级：{order}",
                definition.name(),
                definition.description()
            ));

            if !definition.suggestions().is_empty() {
                lines.push_str(&format!(
                    "；可选值：{}",
                    definition.suggestions().join("、")
                ));
            }
            lines.push('\n');
        }

        context.reply_text(lines.trim_end()).await?;
        Ok(())
    }

    async fn get(
        &self,
        context: &CommandContext<'_>,
        invocation: &CommandInvocation,
    ) -> Result<(), CommandError> {
        let state_name = invocation
            .args
            .get::<String>("state")
            .map_err(CommandError::internal)?;
        let definition = self.find_definition(&state_name)?;

        let scope_raw = invocation
            .args
            .get_opt::<String>("scope")
            .map_err(CommandError::internal)?;
        let explicit_scope_name = scope_raw.clone();

        let value = if let Some(scope_raw) = scope_raw {
            let scope = self.parse_scope(context, definition, &scope_raw)?;
            self.states
                .get_at_scope(definition, &scope)
                .await
                .map_err(map_state_error)?
                .map(|value| value.value)
        } else {
            Some(
                self.states
                    .resolve(definition, context.address(), Some(&context.sender().id))
                    .await
                    .map_err(map_state_error)?
                    .value,
            )
        };

        let Some(value) = value else {
            context
                .reply_text(format!(
                    "{} 在{}范围内未单独设置。",
                    definition.name(),
                    explicit_scope_name.as_deref().unwrap_or("指定")
                ))
                .await?;
            return Ok(());
        };

        let formatted = definition
            .format_value(&value)
            .map_err(CommandError::internal)?;
        let message = match explicit_scope_name.as_deref() {
            Some(scope) => format!("{} 在{}范围内的值：{formatted}", definition.name(), scope),
            None => format!("{} 当前生效值：{formatted}", definition.name()),
        };
        context.reply_text(message).await?;
        Ok(())
    }

    async fn set(
        &self,
        context: &CommandContext<'_>,
        invocation: &CommandInvocation,
    ) -> Result<(), CommandError> {
        let scope_raw = invocation
            .args
            .get::<String>("scope")
            .map_err(CommandError::internal)?;
        let state_name = invocation
            .args
            .get::<String>("state")
            .map_err(CommandError::internal)?;
        let raw_value = invocation
            .args
            .get::<String>("value")
            .map_err(CommandError::internal)?;

        let definition = self.find_definition(&state_name)?;
        let scope = self.parse_scope(context, definition, &scope_raw)?;
        self.ensure_write_allowed(context, scope.kind(), definition.name())
            .await?;

        let value = self
            .states
            .set_at_scope(definition, &scope, &raw_value)
            .await
            .map_err(map_state_error)?;
        let formatted = definition
            .format_value(&value)
            .map_err(CommandError::internal)?;

        context
            .reply_text(format!(
                "已将 {} 在{}范围内设置为：{formatted}",
                definition.name(),
                scope_display(scope.kind())
            ))
            .await?;
        Ok(())
    }

    async fn clear(
        &self,
        context: &CommandContext<'_>,
        invocation: &CommandInvocation,
    ) -> Result<(), CommandError> {
        let scope_raw = invocation
            .args
            .get::<String>("scope")
            .map_err(CommandError::internal)?;
        let state_name = invocation
            .args
            .get::<String>("state")
            .map_err(CommandError::internal)?;

        let definition = self.find_definition(&state_name)?;
        let scope = self.parse_scope(context, definition, &scope_raw)?;
        self.ensure_write_allowed(context, scope.kind(), definition.name())
            .await?;

        self.states
            .clear_at_scope(definition, &scope)
            .await
            .map_err(map_state_error)?;

        context
            .reply_text(format!(
                "已清除 {} 在{}范围内的设置。",
                definition.name(),
                scope_display(scope.kind())
            ))
            .await?;
        Ok(())
    }

    fn find_definition(&self, name: &str) -> Result<&dyn StateDefinition, CommandError> {
        self.states
            .registry()
            .find(name.trim())
            .ok_or_else(|| CommandError::UserInput(format!("未找到配置项：{name}")))
    }

    fn parse_scope(
        &self,
        context: &CommandContext<'_>,
        definition: &dyn StateDefinition,
        raw: &str,
    ) -> Result<StateScope, CommandError> {
        let kind = parse_scope_kind(raw)?;
        if !definition.allowed_scopes().contains(&kind) {
            return Err(CommandError::UserInput(format!(
                "配置项不支持“{}”范围。",
                scope_display(kind)
            )));
        }

        Ok(scope_for(
            kind,
            context.address(),
            context.sender().id.clone(),
        ))
    }

    async fn ensure_write_allowed(
        &self,
        context: &CommandContext<'_>,
        kind: lukosbot_core::StateScopeKind,
        state_name: &str,
    ) -> Result<(), CommandError> {
        match kind {
            lukosbot_core::StateScopeKind::User => Ok(()),
            lukosbot_core::StateScopeKind::Chat => self
                .authorization
                .require_chat_manager(context.message())
                .await
                .map(|_| ())
                .map_err(|error| {
                    map_auth_error(error, &format!("修改当前聊天配置（{state_name}）"))
                }),
            lukosbot_core::StateScopeKind::Global => self
                .authorization
                .require_bot_admin(context.message())
                .await
                .map(|_| ())
                .map_err(|error| map_auth_error(error, &format!("修改全局配置（{state_name}）"))),
        }
    }
}

#[async_trait]
impl CommandExecutor for PrefCommand {
    async fn execute(
        &self,
        context: &CommandContext<'_>,
        invocation: CommandInvocation,
    ) -> Result<(), CommandError> {
        match invocation.path.as_slice() {
            [root] if root == "pref" => self.list(context).await,
            [root, sub] if root == "pref" => match sub.as_str() {
                "list" => self.list(context).await,
                "get" => self.get(context, &invocation).await,
                "set" => self.set(context, &invocation).await,
                "clear" => self.clear(context, &invocation).await,
                _ => Err(CommandError::internal("unknown pref subcommand")),
            },
            _ => Err(CommandError::internal("invalid pref command path")),
        }
    }
}

fn parse_scope_kind(raw: &str) -> Result<lukosbot_core::StateScopeKind, CommandError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "user" => Ok(lukosbot_core::StateScopeKind::User),
        "chat" => Ok(lukosbot_core::StateScopeKind::Chat),
        "global" => Ok(lukosbot_core::StateScopeKind::Global),
        _ => Err(CommandError::UserInput(format!("未知配置范围：{raw}"))),
    }
}

fn scope_for(
    kind: lukosbot_core::StateScopeKind,
    address: &ChatAddress,
    user_id: PlatformUserId,
) -> StateScope {
    match kind {
        lukosbot_core::StateScopeKind::User => StateScope::user(address.platform, user_id),
        lukosbot_core::StateScopeKind::Chat => StateScope::chat(address),
        lukosbot_core::StateScopeKind::Global => StateScope::Global,
    }
}

fn scope_display(kind: lukosbot_core::StateScopeKind) -> &'static str {
    match kind {
        lukosbot_core::StateScopeKind::User => "本人",
        lukosbot_core::StateScopeKind::Chat => "当前聊天",
        lukosbot_core::StateScopeKind::Global => "全局",
    }
}

fn map_state_error(error: StateServiceError) -> CommandError {
    match error {
        StateServiceError::UnsupportedScope { scope, .. } => {
            CommandError::UserInput(format!("配置项不支持“{}”范围。", scope_display(scope)))
        }
        StateServiceError::Parse { source, .. } | StateServiceError::Validation { source, .. } => {
            CommandError::UserInput(source.to_string())
        }
        StateServiceError::InvalidStoredValue { .. } => {
            CommandError::UserInput("配置项当前值无效，请管理员检查。".to_owned())
        }
        StateServiceError::Storage(_) | StateServiceError::Definition { .. } => {
            tracing::warn!(%error, "state service operation failed");
            CommandError::UserInput("配置服务暂时不可用，请稍后重试。".to_owned())
        }
        error => {
            tracing::warn!(%error, "state service operation failed");
            CommandError::UserInput("配置服务暂时不可用，请稍后重试。".to_owned())
        }
    }
}

pub fn definition(
    states: Arc<StateService>,
    authorization: Arc<AuthorizationService>,
) -> CommandDefinition {
    PrefCommand::new(states, authorization).into_definition()
}
