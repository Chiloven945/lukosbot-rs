use std::sync::Arc;

use async_trait::async_trait;

use lukosbot_command_engine::{
    ArgType, ArgsSpec, CommandContext, CommandDefinition, CommandError, CommandExecutor,
    CommandInvocation, InputSpec, PositionalSpec, SyntaxLine, SyntaxPart, UsageMetadata,
    help::{render_command, render_list_where},
    policy_denied_message,
};
use lukosbot_domain::{MediaRef, MediaSource, OutboundMessage, OutboundPart};

use crate::rendering::{RenderLine, Theme, render_lines};

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
                .reply_text(render_list_where(
                    context.registry(),
                    context.prefix(),
                    |definition| context.is_command_allowed(&definition.name),
                ))
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

        if !context.is_command_allowed(&definition.name) {
            return Err(CommandError::UserInput(policy_denied_message(
                &definition.name,
            )));
        }

        let mode = mode.unwrap_or_default();
        let mode = mode.trim().to_ascii_lowercase();
        if !mode.is_empty()
            && !matches!(
                mode.as_str(),
                "text" | "txt" | "raw" | "文字" | "img" | "image" | "pic" | "png" | "图片" | "图"
            )
        {
            return Err(CommandError::UserInput(format!(
                "未知的输出方式：{mode}，可选值：text, img"
            )));
        }

        let text = render_command(definition, context.prefix());
        let wants_image = matches!(
            mode.as_str(),
            "img" | "image" | "pic" | "png" | "图片" | "图"
        );
        let auto_image =
            mode.is_empty() && (text.chars().count() > 1_400 || text.lines().count() > 32);
        if wants_image || auto_image {
            return match render_help_image(definition) {
                Ok(image) => {
                    context
                        .reply_message(OutboundMessage {
                            parts: vec![OutboundPart::Image {
                                media: MediaRef {
                                    source: MediaSource::Bytes(image),
                                    file_name: Some(format!("help-{}.png", definition.name)),
                                    mime_type: Some("image/png".to_owned()),
                                    size_bytes: None,
                                },
                                caption: None,
                            }],
                        })
                        .await?;
                    Ok(())
                }
                Err(error) => {
                    tracing::warn!(%error, "help image rendering failed");
                    let fallback = if wants_image {
                        format!("图片帮助生成失败，已回退文本。\n\n{text}")
                    } else {
                        text
                    };
                    context.reply_text(fallback).await?;
                    Ok(())
                }
            };
        }

        context.reply_text(text).await?;
        Ok(())
    }
}

fn render_help_image(definition: &CommandDefinition) -> Result<bytes::Bytes, CommandError> {
    let mut lines = vec![
        RenderLine::title(format!("命令：/{}", definition.name)),
        RenderLine::muted(definition.description.clone()),
    ];
    if !definition.aliases.is_empty() {
        lines.push(RenderLine::muted(format!(
            "别名：{}",
            definition.aliases.join(", ")
        )));
    }

    for syntax in &definition.usage.syntaxes {
        lines.push(RenderLine::body(format_syntax(syntax)));
    }
    for example in &definition.usage.examples {
        lines.push(RenderLine::muted(format!("示例：{example}")));
    }
    for note in &definition.usage.notes {
        lines.push(RenderLine::muted(format!("说明：{note}")));
    }

    render_lines(&lines, Theme::Light, 1_000, 8_000)
        .map_err(|error| CommandError::internal(error.to_string()))
}

fn format_syntax(syntax: &SyntaxLine) -> String {
    let mut text = String::new();
    if !syntax.description.is_empty() {
        text.push_str(&syntax.description);
        text.push_str("\n  ");
    }
    for part in &syntax.parts {
        append_part(&mut text, part);
    }
    text
}

fn append_part(output: &mut String, part: &SyntaxPart) {
    match part {
        SyntaxPart::Literal(value) => output.push_str(value),
        SyntaxPart::Argument { name, optional } => {
            if *optional {
                output.push('[');
                output.push_str(name);
                output.push(']');
            } else {
                output.push('<');
                output.push_str(name);
                output.push('>');
            }
        }
        SyntaxPart::Option {
            names,
            value_name,
            optional,
        } => {
            if *optional {
                output.push('[');
            }
            output.push_str(&names.join("/"));
            if let Some(value_name) = value_name {
                output.push(' ');
                output.push_str(value_name);
            }
            if *optional {
                output.push(']');
            }
        }
        SyntaxPart::OneOf(parts) => {
            output.push('(');
            for (index, part) in parts.iter().enumerate() {
                if index > 0 {
                    output.push('|');
                }
                append_part(output, part);
            }
            output.push(')');
        }
    }
    output.push(' ');
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
        notes: vec!["输出方式：text 为文本帮助，img 为图片帮助。".to_owned()],
    })
}
