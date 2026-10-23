use std::sync::Arc;

use async_trait::async_trait;

use lukosbot_command_engine::{
    CommandContext, CommandDefinition, CommandError, CommandExecutor, CommandInvocation, InputSpec,
    UsageMetadata,
};

pub struct StartCommand;

#[async_trait]
impl CommandExecutor for StartCommand {
    async fn execute(
        &self,
        context: &CommandContext<'_>,
        _invocation: CommandInvocation,
    ) -> Result<(), CommandError> {
        let text = format!(
            "欢迎使用 LukosBot！这是由 @chiloven945 制作的聊天机器人，你可以在 Discord 和 Telegram 上找到它！发送 {}help 查看可用命令。",
            context.prefix()
        );
        context.reply_text(text).await?;
        Ok(())
    }
}

pub fn definition() -> CommandDefinition {
    CommandDefinition::new(
        "start",
        "开始使用 LukosBot",
        InputSpec::NoArgs,
        Arc::new(StartCommand),
    )
    .hidden()
    .usage(UsageMetadata {
        examples: vec!["start".to_owned()],
        ..UsageMetadata::default()
    })
}
