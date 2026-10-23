use std::sync::Arc;

use async_trait::async_trait;
use thiserror::Error;
use tracing::{debug, warn};

use lukosbot_core::handler::{HandlerContext, HandlerError, MessageHandler};
use lukosbot_core::policy::CommandPolicy;
use lukosbot_domain::{InboundMessage, OutboundMessage};

use crate::args::ParsedArgs;
use crate::context::CommandContext;
use crate::error::{CommandError, CommandParseError, ResolveError};
use crate::invocation::CommandInvocation;
use crate::lexer;
use crate::parser::parse_args;
use crate::registry::{CommandRegistry, ResolvedCommand};
use crate::spec::InputSpec;

/// A command handler configuration error.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum CommandHandlerError {
    #[error("command prefix must not be empty")]
    EmptyPrefix,

    #[error("command prefix must not be only whitespace")]
    BlankPrefix,

    #[error("command prefix must not contain control characters")]
    ControlCharacter,
}

/// The bridge between the core message pipeline and the command engine.
pub struct CommandMessageHandler {
    prefix: String,
    registry: Arc<CommandRegistry>,
    policy: Arc<dyn CommandPolicy>,
}

impl CommandMessageHandler {
    pub fn new(
        prefix: impl Into<String>,
        registry: Arc<CommandRegistry>,
        policy: Arc<dyn CommandPolicy>,
    ) -> Result<Self, CommandHandlerError> {
        let prefix = prefix.into();
        if prefix.is_empty() {
            return Err(CommandHandlerError::EmptyPrefix);
        }
        if prefix.chars().all(char::is_whitespace) {
            return Err(CommandHandlerError::BlankPrefix);
        }
        if prefix.chars().any(char::is_control) {
            return Err(CommandHandlerError::ControlCharacter);
        }

        Ok(Self {
            prefix,
            registry,
            policy,
        })
    }

    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    async fn reply_error(
        context: &HandlerContext,
        message: &InboundMessage,
        text: String,
    ) -> Result<(), HandlerError> {
        context
            .outbound()
            .send(message.address.clone(), OutboundMessage::text(text))
            .await?;
        Ok(())
    }
}

#[async_trait]
impl MessageHandler for CommandMessageHandler {
    async fn handle(
        &self,
        context: &HandlerContext,
        message: InboundMessage,
    ) -> Result<(), HandlerError> {
        let Some(primary) = message.primary_text() else {
            return Ok(());
        };

        let primary = primary.trim();
        if primary.is_empty() || !primary.starts_with(&self.prefix) {
            return Ok(());
        }

        let line = primary[self.prefix.len()..].trim();
        if line.is_empty() {
            return Ok(());
        }

        let root_token = line
            .split_whitespace()
            .next()
            .expect("a non-empty command line has a first token");
        let Some(definition) = self.registry.find(root_token) else {
            return Ok(());
        };

        let decision = self.policy.evaluate_command(&message, &definition.name);
        if !decision.allowed {
            debug!(
                command = %definition.name,
                matched_rules = ?decision.matched_rule_ids,
                "command denied by policy"
            );
            return Self::reply_error(context, &message, policy_denied_message(&definition.name))
                .await;
        }

        let resolved = match self.registry.resolve(line) {
            Ok(Some(resolved)) => resolved,
            Ok(None) => return Ok(()),
            Err(error) => {
                let (path, text) = render_resolve_error(&error, &self.prefix);
                debug!(command = %path, "command resolution failed");
                return Self::reply_error(context, &message, text).await;
            }
        };

        let invocation = match build_invocation(&resolved, line) {
            Ok(invocation) => invocation,
            Err(error) => {
                let path = resolved.path.join(" ");
                debug!(command = %path, "command arguments were invalid");
                let text = render_parse_error(&error, &self.prefix, &path);
                return Self::reply_error(context, &message, text).await;
            }
        };

        let command_path = resolved.path.join(" ");
        let Some(executor) = resolved.node.executor.as_ref() else {
            warn!(command = %command_path, "resolved command has no executor");
            return Self::reply_error(context, &message, "命令执行失败，请稍后再试。".to_owned())
                .await;
        };

        let command_context = CommandContext::new(
            &message,
            context,
            &self.registry,
            &self.prefix,
            &*self.policy,
        );

        match executor.execute(&command_context, invocation).await {
            Ok(()) => Ok(()),
            Err(CommandError::UserInput(text)) => Self::reply_error(context, &message, text).await,
            Err(CommandError::Send(error)) => Err(HandlerError::Outbound(error)),
            Err(CommandError::Internal(error)) => {
                warn!(command = %command_path, %error, "command execution failed");
                Self::reply_error(context, &message, "命令执行失败，请稍后再试。".to_owned()).await
            }
        }
    }
}

fn build_invocation(
    resolved: &ResolvedCommand<'_>,
    line: &str,
) -> Result<CommandInvocation, CommandParseError> {
    let tail = resolved.tail.trim_start();
    let args = match &resolved.node.input {
        InputSpec::NoArgs => {
            if !tail.is_empty() {
                return Err(CommandParseError::UnexpectedArguments {
                    rest: tail.to_owned(),
                });
            }
            ParsedArgs::default()
        }
        InputSpec::Raw { name, required } => {
            if *required && tail.is_empty() {
                return Err(CommandParseError::MissingArgument { name: name.clone() });
            }
            ParsedArgs::default()
        }
        InputSpec::Args(spec) => {
            let tokens = lexer::split(tail)?;
            parse_args(&tokens, spec)?
        }
    };

    Ok(CommandInvocation {
        raw_command_line: line.to_owned(),
        path: resolved
            .path
            .iter()
            .map(|part| (*part).to_owned())
            .collect(),
        raw_tail: tail.to_owned(),
        args,
    })
}

fn render_parse_error(error: &CommandParseError, prefix: &str, path: &str) -> String {
    format!("命令参数错误：{error}\n\n发送 {prefix}help {path} 查看详细用法。")
}

fn render_resolve_error(error: &ResolveError, prefix: &str) -> (String, String) {
    let path = match error {
        ResolveError::UnknownSubcommand { path, .. } | ResolveError::MissingSubcommand { path } => {
            path.clone()
        }
    };
    let text = format!("命令参数错误：{error}\n\n发送 {prefix}help {path} 查看详细用法。");
    (path, text)
}

/// The user-facing message for a policy-denied command.
pub fn policy_denied_message(command_name: &str) -> String {
    format!("当前聊天不允许使用此命令：{command_name}")
}
