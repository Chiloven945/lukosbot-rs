use std::sync::Arc;

use async_trait::async_trait;

use lukosbot_command_engine::{
    CommandContext, CommandDefinition, CommandError, CommandExecutor, CommandInvocation, InputSpec,
    UsageMetadata,
};

pub struct EchoCommand;

#[async_trait]
impl CommandExecutor for EchoCommand {
    async fn execute(
        &self,
        context: &CommandContext<'_>,
        invocation: CommandInvocation,
    ) -> Result<(), CommandError> {
        let text = invocation.raw_tail.trim();
        if text.is_empty() {
            return Err(CommandError::UserInput(format!(
                "请输入要回显的文本。用法：{}echo <text>",
                context.prefix()
            )));
        }

        context.reply_text(text).await?;
        Ok(())
    }
}

pub fn definition() -> CommandDefinition {
    CommandDefinition::new(
        "echo",
        "原样返回文本",
        InputSpec::Raw {
            name: "text".to_owned(),
            required: false,
        },
        Arc::new(EchoCommand),
    )
    .usage(UsageMetadata {
        syntaxes: vec![lukosbot_command_engine::SyntaxLine::described(
            "回显输入的文本",
            [lukosbot_command_engine::SyntaxPart::Argument {
                name: "text".to_owned(),
                optional: false,
            }],
        )],
        examples: vec!["echo hello world".to_owned()],
        ..UsageMetadata::default()
    })
}
