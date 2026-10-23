use std::sync::Arc;

use async_trait::async_trait;

use lukosbot_command_engine::{
    ArgType, ArgValue, ArgsSpec, CommandContext, CommandDefinition, CommandError, CommandExecutor,
    CommandInvocation, InputSpec, PositionalSpec, UsageMetadata, Validator,
};

use super::random::{RandomSource, approximate_multinomial};

pub struct DiceCommand {
    random: Arc<dyn RandomSource>,
}

impl DiceCommand {
    pub fn new(random: Arc<dyn RandomSource>) -> Self {
        Self { random }
    }
}

#[async_trait]
impl CommandExecutor for DiceCommand {
    async fn execute(
        &self,
        context: &CommandContext<'_>,
        invocation: CommandInvocation,
    ) -> Result<(), CommandError> {
        let count = invocation
            .args
            .get::<i64>("count")
            .map_err(CommandError::internal)?;
        let text = run_dice(count, self.random.as_ref());
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
                .description("骰子数量（正整数）")
                .validator(Validator::new(|value| match value {
                    ArgValue::I64(count) if *count > 0 => Ok(()),
                    _ => Err("骰子数量必须是正整数。".to_owned()),
                })),
        ],
        options: Vec::new(),
    };

    CommandDefinition::new(
        "dice",
        "掷骰子，可指定骰子数量",
        InputSpec::Args(args),
        Arc::new(DiceCommand::new(random)),
    )
    .usage(UsageMetadata {
        examples: vec!["dice".to_owned(), "dice 3".to_owned()],
        ..UsageMetadata::default()
    })
}

/// Generates the user-visible dice result. Kept public for deterministic unit tests.
pub fn run_dice(count: i64, random: &dyn RandomSource) -> String {
    if count <= 0 {
        return "骰子数量必须是正整数。".to_owned();
    }

    let count = count as u64;
    let faces = approximate_multinomial(count, &[1.0; 6], random);

    if count == 1 {
        let face = faces
            .iter()
            .position(|value| *value > 0)
            .map(|index| index + 1)
            .unwrap_or(1);
        return format!("你掷了 1 个骰子。\n朝上的一面是……{face}！");
    }

    let sum: u128 = faces
        .iter()
        .enumerate()
        .map(|(index, count)| u128::from(*count) * (index as u128 + 1))
        .sum();

    format!(
        "你掷了 {count} 个骰子。\n其中，点数为 1 的有 {} 个，2 的有 {} 个，3 的有 {} 个，4 的有 {} 个，5 的有 {} 个，6 的有 {} 个。\n它们的点数合计为 {sum}！",
        faces[0], faces[1], faces[2], faces[3], faces[4], faces[5]
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::basic::random::SeededRandom;

    #[test]
    fn single_die_has_one_face() {
        let random = SeededRandom::new(4);
        let text = run_dice(1, &random);
        assert!(text.starts_with("你掷了 1 个骰子。"));
    }

    #[test]
    fn multi_dice_sum_matches_buckets() {
        let random = SeededRandom::new(5);
        let text = run_dice(20, &random);
        assert!(text.contains("你掷了 20 个骰子。"));
        assert!(text.contains("它们的点数合计为"));
    }

    #[test]
    fn invalid_count_is_rejected() {
        let random = SeededRandom::new(6);
        assert_eq!(run_dice(0, &random), "骰子数量必须是正整数。");
        assert_eq!(run_dice(-1, &random), "骰子数量必须是正整数。");
    }
}
