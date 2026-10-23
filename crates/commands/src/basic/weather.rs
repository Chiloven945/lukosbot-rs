use std::sync::Arc;

use async_trait::async_trait;

use lukosbot_command_engine::{
    CommandContext, CommandDefinition, CommandError, CommandExecutor, CommandInvocation, InputSpec,
    UsageMetadata,
};

pub struct WeatherCommand;

#[async_trait]
impl CommandExecutor for WeatherCommand {
    async fn execute(
        &self,
        context: &CommandContext<'_>,
        _invocation: CommandInvocation,
    ) -> Result<(), CommandError> {
        context
            .reply_text("天气查询功能尚未接入，请关注后续更新。")
            .await?;
        Ok(())
    }
}

pub fn definition() -> CommandDefinition {
    CommandDefinition::new(
        "weather",
        "查询天气（暂未开放）",
        InputSpec::NoArgs,
        Arc::new(WeatherCommand),
    )
    .usage(UsageMetadata {
        examples: vec!["weather".to_owned()],
        notes: vec!["天气查询功能尚未接入，暂时无法使用。".to_owned()],
        ..UsageMetadata::default()
    })
}
