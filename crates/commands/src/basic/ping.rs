use std::sync::Arc;

use async_trait::async_trait;

use lukosbot_command_engine::{
    CommandContext, CommandDefinition, CommandError, CommandExecutor, InputSpec, UsageMetadata,
};

use crate::runtime_info::RuntimeInfo;

pub struct PingCommand {
    info: Arc<RuntimeInfo>,
}

impl PingCommand {
    pub fn new(info: Arc<RuntimeInfo>) -> Self {
        Self { info }
    }
}

#[async_trait]
impl CommandExecutor for PingCommand {
    async fn execute(
        &self,
        context: &CommandContext<'_>,
        _invocation: lukosbot_command_engine::CommandInvocation,
    ) -> Result<(), CommandError> {
        context.reply_text(self.info.status_text()).await?;
        Ok(())
    }
}

pub fn definition(info: Arc<RuntimeInfo>) -> CommandDefinition {
    CommandDefinition::new(
        "ping",
        "返回机器人的运行状态与版本信息",
        InputSpec::NoArgs,
        Arc::new(PingCommand::new(info)),
    )
    .usage(UsageMetadata {
        examples: vec!["ping".to_owned()],
        ..UsageMetadata::default()
    })
}
