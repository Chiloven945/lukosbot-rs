use std::sync::Arc;

use async_trait::async_trait;

use lukosbot_command_engine::{
    ArgType, ArgValue, ArgsSpec, CommandContext, CommandDefinition, CommandError, CommandExecutor,
    CommandInvocation, InputSpec, PositionalSpec, UsageMetadata, Validator,
};

use super::random::{RandomSource, approximate_multinomial};

pub struct CoinCommand {
    random: Arc<dyn RandomSource>,
}

impl CoinCommand {
    pub fn new(random: Arc<dyn RandomSource>) -> Self {
        Self { random }
    }
}

#[async_trait]
impl CommandExecutor for CoinCommand {
    async fn execute(
        &self,
        context: &CommandContext<'_>,
        invocation: CommandInvocation,
    ) -> Result<(), CommandError> {
        let count = invocation
            .args
            .get::<i64>("count")
            .map_err(CommandError::internal)?;
        let text = run_coin(count, self.random.as_ref());
        context.reply_text(text).await?;
        Ok(())
    }
}

pub fn definition(random: Arc<dyn RandomSource>) -> CommandDefinition {
    let args = ArgsSpec {
        positionals: vec![
            PositionalSpec::new("count", ArgType::I64)
                .optional()
                .default(ArgValue::I64(1))
                .description("硬币数量（正整数）")
                .validator(Validator::new(|value| match value {
                    ArgValue::I64(count) if *count > 0 => Ok(()),
                    _ => Err("硬币数量必须是正整数。".to_owned()),
                })),
        ],
        options: Vec::new(),
    };

    CommandDefinition::new(
        "coin",
        "抛硬币",
        InputSpec::Args(args),
        Arc::new(CoinCommand::new(random)),
    )
    .usage(UsageMetadata {
        examples: vec!["coin 10".to_owned()],
        ..UsageMetadata::default()
    })
}

/// Generates the user-visible coin result. Kept public for deterministic unit tests.
pub fn run_coin(times: i64, random: &dyn RandomSource) -> String {
    if times <= 0 {
        return "硬币数量必须是正整数。".to_owned();
    }

    let times = times as u64;
    let counts = approximate_multinomial(
        times,
        &[0.499999999999, 0.499999999999, 0.000000000002],
        random,
    );

    if times == 1 {
        let result = if counts[0] == 1 {
            "是正面。"
        } else if counts[1] == 1 {
            "是反面。"
        } else {
            "它立起来了！"
        };
        format!("你抛了 1 个硬币。\n{result}")
    } else {
        format!(
            "你抛了 {times} 个硬币。\n其中 {} 个是正面，{} 个是反面……\n还有 {} 个立起来了！",
            counts[0], counts[1], counts[2]
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::basic::random::SeededRandom;

    #[test]
    fn single_coin_has_one_result() {
        let random = SeededRandom::new(1);
        let text = run_coin(1, &random);
        assert!(text.starts_with("你抛了 1 个硬币。"));
    }

    #[test]
    fn multi_coin_output_keeps_bucket_total() {
        let random = SeededRandom::new(2);
        let text = run_coin(10, &random);
        assert!(text.contains("你抛了 10 个硬币。"));
    }

    #[test]
    fn invalid_count_is_rejected() {
        let random = SeededRandom::new(3);
        assert_eq!(run_coin(0, &random), "硬币数量必须是正整数。");
        assert_eq!(run_coin(-1, &random), "硬币数量必须是正整数。");
    }
}
