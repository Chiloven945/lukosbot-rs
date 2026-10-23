use std::sync::Arc;

use async_trait::async_trait;

use lukosbot_command_engine::{
    CommandContext, CommandDefinition, CommandError, CommandExecutor, CommandInvocation, InputSpec,
    SyntaxLine, SyntaxPart, UsageMetadata,
};

use super::manager::GameSessionKey;
use super::worker::TwentyFourHandle;

pub struct TwentyFourCommand {
    handle: TwentyFourHandle,
}

impl TwentyFourCommand {
    pub fn new(handle: TwentyFourHandle) -> Self {
        Self { handle }
    }
}

#[async_trait]
impl CommandExecutor for TwentyFourCommand {
    async fn execute(
        &self,
        context: &CommandContext<'_>,
        invocation: CommandInvocation,
    ) -> Result<(), CommandError> {
        let key = GameSessionKey {
            platform: context.address().platform,
            user_id: context.sender().id.clone(),
        };
        let input = invocation.raw_tail.trim();

        let reply = if input.is_empty() {
            self.handle
                .start(key, context.address().clone())
                .await
                .map_err(CommandError::internal)?
        } else if input.eq_ignore_ascii_case("giveup") {
            self.handle
                .give_up(key)
                .await
                .map_err(CommandError::internal)?
        } else {
            self.handle
                .submit(key, input.to_owned())
                .await
                .map_err(CommandError::internal)?
        };

        context.reply_text(reply).await?;
        Ok(())
    }
}

pub fn definition(handle: TwentyFourHandle) -> CommandDefinition {
    CommandDefinition::new(
        "24",
        "玩 24 点游戏",
        InputSpec::Raw {
            name: "input".to_owned(),
            required: false,
        },
        Arc::new(TwentyFourCommand::new(handle)),
    )
    .usage(UsageMetadata {
        syntaxes: vec![
            SyntaxLine::described("开始一场新游戏", []),
            SyntaxLine::described(
                "提交答案表达式",
                [SyntaxPart::Argument {
                    name: "expression".to_owned(),
                    optional: false,
                }],
            ),
            SyntaxLine::described("放弃并查看答案", [SyntaxPart::Literal("giveup".to_owned())]),
        ],
        examples: vec![
            "24".to_owned(),
            "24 (2+1)*7+3".to_owned(),
            "24 giveup".to_owned(),
        ],
        ..UsageMetadata::default()
    })
}
