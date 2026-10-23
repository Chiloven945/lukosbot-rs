use std::sync::Arc;

use async_trait::async_trait;

use lukosbot_command_engine::{
    ArgType, ArgsSpec, CommandContext, CommandDefinition, CommandError, CommandExecutor,
    CommandInvocation, InputSpec, PositionalSpec, SyntaxLine, SyntaxPart, UsageMetadata,
    help::{render_command, render_list},
};

pub struct HelpCommand;

#[async_trait]
impl CommandExecutor for HelpCommand {
    async fn execute(
        &self,
        context: &CommandContext<'_>,
        invocation: CommandInvocation,
    ) -> Result<(), CommandError> {
        let command = invocation
            .args
            .get_opt::<String>("command")
            .map_err(CommandError::internal)?;
        let mode = invocation
            .args
            .get_opt::<String>("mode")
            .map_err(CommandError::internal)?;

        let Some(command) = command.filter(|value| !value.trim().is_empty()) else {
            context
                .reply_text(render_list(context.registry(), context.prefix()))
                .await?;
            return Ok(());
        };

        let Some(definition) = context
            .registry()
            .find(command.trim())
            .filter(|definition| definition.visible)
        else {
            return Err(CommandError::UserInput(format!(
                "未知的命令：{}\n发送 {}help 查看可用命令列表。",
                command.trim(),
                context.prefix()
            )));
        };

        let mode = mode.unwrap_or_default();
        let mode = mode.trim().to_ascii_lowercase();
        if matches!(
            mode.as_str(),
            "img" | "image" | "pic" | "png" | "图片" | "图"
        ) {
            return Err(CommandError::UserInput(
                "图片版帮助尚未实现，请使用文本模式。".to_owned(),
            ));
        }
        if !mode.is_empty() && !matches!(mode.as_str(), "text" | "txt" | "raw" | "文字") {
            return Err(CommandError::UserInput(format!(
                "未知的输出方式：{mode}，可选值：text, img"
            )));
        }

        context
            .reply_text(render_command(definition, context.prefix()))
            .await?;
        Ok(())
    }
}

pub fn definition() -> CommandDefinition {
    let args = ArgsSpec {
        positionals: vec![
            PositionalSpec::new("command", ArgType::String)
                .optional()
                .description("命令名"),
            PositionalSpec::new("mode", ArgType::String)
                .optional()
                .description("img 或 text"),
        ],
        options: Vec::new(),
    };

    CommandDefinition::new(
        "help",
        "列出可用命令或其详细用法",
        InputSpec::Args(args),
        Arc::new(HelpCommand),
    )
    .alias("h")
    .usage(UsageMetadata {
        syntaxes: vec![
            SyntaxLine::described("列出所有可用命令", []),
            SyntaxLine::described(
                "查看命令用法（可选强制输出方式）",
                [
                    SyntaxPart::Argument {
                        name: "command".to_owned(),
                        optional: false,
                    },
                    SyntaxPart::OneOf(vec![
                        SyntaxPart::Literal("img".to_owned()),
                        SyntaxPart::Literal("text".to_owned()),
                    ]),
                ],
            ),
        ],
        examples: vec![
            "help".to_owned(),
            "help wiki".to_owned(),
            "help wiki img".to_owned(),
        ],
        notes: vec!["输出方式：text 为文本帮助，img 为图片帮助（本阶段尚未实现）。".to_owned()],
    })
}
