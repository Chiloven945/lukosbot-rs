use std::sync::Arc;

use async_trait::async_trait;

use lukosbot_command_engine::{
    CommandContext, CommandDefinition, CommandError, CommandExecutor, CommandInvocation, InputSpec,
    SyntaxLine, SyntaxPart, UsageMetadata,
};
use lukosbot_integrations::WhoisService;

use super::error::map_integration_error;
use super::format::truncate_reply;

pub struct WhoisCommand {
    service: Arc<dyn WhoisService>,
}

impl WhoisCommand {
    pub fn new(service: Arc<dyn WhoisService>) -> Self {
        Self { service }
    }

    pub fn into_definition(self) -> CommandDefinition {
        CommandDefinition::new(
            "whois",
            "查询域名 Whois 信息",
            InputSpec::Raw {
                name: "domain".to_owned(),
                required: true,
            },
            Arc::new(self),
        )
        .usage(UsageMetadata {
            syntaxes: vec![SyntaxLine::described(
                "查询域名 Whois 信息",
                [SyntaxPart::Argument {
                    name: "domain".to_owned(),
                    optional: false,
                }],
            )],
            examples: vec!["whois example.com".to_owned()],
            notes: vec!["支持普通域名和国际化域名。".to_owned()],
        })
    }
}

#[async_trait]
impl CommandExecutor for WhoisCommand {
    async fn execute(
        &self,
        context: &CommandContext<'_>,
        invocation: CommandInvocation,
    ) -> Result<(), CommandError> {
        if invocation.path.as_slice() != ["whois"] {
            return Err(CommandError::internal("invalid whois command path"));
        }

        let record = self
            .service
            .query(invocation.raw_tail.trim())
            .await
            .map_err(|error| {
                map_integration_error(error, "未查询到 WHOIS 信息。", "域名格式无效。")
            })?;

        let text = record.text.trim();
        if text.is_empty() {
            return Err(CommandError::UserInput("未查询到 WHOIS 信息。".to_owned()));
        }

        let mut reply = text.to_owned();
        if record.truncated {
            reply.push_str("\n…（结果过长，已截断）");
        }
        context.reply_text(truncate_reply(&reply)).await?;
        Ok(())
    }
}

pub fn definition(service: Arc<dyn WhoisService>) -> CommandDefinition {
    WhoisCommand::new(service).into_definition()
}
