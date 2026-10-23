use std::sync::Arc;

use async_trait::async_trait;

use lukosbot_command_engine::{
    ArgType, ArgValue, ArgsSpec, CommandContext, CommandDefinition, CommandError, CommandExecutor,
    CommandInvocation, InputSpec, OptionSpec, PositionalSpec, SyntaxLine, SyntaxPart,
    UsageMetadata,
};
use lukosbot_integrations::{BilibiliService, BilibiliVideo, parse_bilibili_target};

use super::error::map_integration_error;
use super::format::{metric, strip_controls, truncate_reply, utc_time};

pub struct BilibiliCommand {
    service: Arc<dyn BilibiliService>,
}

impl BilibiliCommand {
    pub fn new(service: Arc<dyn BilibiliService>) -> Self {
        Self { service }
    }

    pub fn into_definition(self) -> CommandDefinition {
        let args = ArgsSpec {
            positionals: vec![
                PositionalSpec::new("target", ArgType::String)
                    .description("视频编号（AV / BV）或链接"),
            ],
            options: vec![
                OptionSpec::new("detailed", ["-i"], ArgType::Bool)
                    .default(ArgValue::Bool(false))
                    .description("展示更完整的视频信息"),
            ],
        };

        CommandDefinition::new(
            "bilibili",
            "查询 Bilibili 视频信息",
            InputSpec::Args(args),
            Arc::new(self),
        )
        .alias("bili")
        .usage(UsageMetadata {
            syntaxes: vec![SyntaxLine::described(
                "查询视频",
                [
                    SyntaxPart::Argument {
                        name: "target".to_owned(),
                        optional: false,
                    },
                    SyntaxPart::Option {
                        names: vec!["-i".to_owned()],
                        value_name: None,
                        optional: true,
                    },
                ],
            )],
            examples: vec![
                "bilibili BV1GJ411x7h7".to_owned(),
                "bilibili av170001".to_owned(),
                "bilibili https://b23.tv/BV1GJ411x7h7 -i".to_owned(),
            ],
            notes: vec!["-i 展示更完整的信息。".to_owned()],
        })
    }
}

#[async_trait]
impl CommandExecutor for BilibiliCommand {
    async fn execute(
        &self,
        context: &CommandContext<'_>,
        invocation: CommandInvocation,
    ) -> Result<(), CommandError> {
        if invocation.path.as_slice() != ["bilibili"] {
            return Err(CommandError::internal("invalid bilibili command path"));
        }

        let target = invocation
            .args
            .get::<String>("target")
            .map_err(CommandError::internal)?;
        let detailed = invocation
            .args
            .get::<bool>("detailed")
            .map_err(CommandError::internal)?;

        let target = parse_bilibili_target(target.trim())
            .map_err(|error| map_error(error, "未找到该视频。", "视频编号或链接无效。"))?;
        let video = self
            .service
            .query(&target)
            .await
            .map_err(|error| map_error(error, "未找到该视频。", "视频编号或链接无效。"))?;

        let mut text = if detailed {
            format_detailed(&video)
        } else {
            format_simple(&video)
        };
        if let Some(cover) = &video.cover_url {
            text.push('\n');
            text.push_str(cover.as_str());
        }

        context.reply_text(truncate_reply(&text)).await?;
        Ok(())
    }
}

fn map_error(
    error: lukosbot_integrations::IntegrationError,
    not_found: &str,
    invalid: &str,
) -> CommandError {
    map_integration_error(error, not_found, invalid)
}

fn format_simple(video: &BilibiliVideo) -> String {
    format!(
        "{}\n标题：{}\n类型：{}\nUP 主：{}\n日期：{}\n\n添加 `-i` 可查看更多信息。",
        video.url,
        video_title(video),
        category(video),
        owner_name(video),
        published(video)
    )
}

fn format_detailed(video: &BilibiliVideo) -> String {
    let mut text = String::new();
    text.push_str(video.url.as_str());
    text.push('\n');
    text.push_str("标题：");
    text.push_str(&video_title(video));
    if video.pages > 1 {
        text.push_str(&format!("（{}P）", video.pages));
    }
    text.push_str(" | 类型：");
    text.push_str(&category(video));
    text.push('\n');

    text.push_str("UP 主：");
    text.push_str(&owner_name(video));
    text.push_str(" | 粉丝：");
    text.push_str(
        &video
            .owner
            .followers
            .map(metric)
            .unwrap_or_else(|| "未知".to_owned()),
    );
    text.push('\n');

    if let Some(description) = video
        .description
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        text.push_str("简介：");
        text.push_str(&truncate_value(description, 160));
        text.push('\n');
    }

    text.push_str(&format!(
        "观看：{} | 弹幕：{} | 评论：{}\n",
        metric(video.stats.views),
        metric(video.stats.danmaku),
        metric(video.stats.replies)
    ));
    text.push_str(&format!(
        "喜欢：{} | 投币：{} | 收藏：{} | 分享：{}\n",
        metric(video.stats.likes),
        metric(video.stats.coins),
        metric(video.stats.favorites),
        metric(video.stats.shares)
    ));
    text.push_str("日期：");
    text.push_str(&published(video));
    text
}

fn video_title(video: &BilibiliVideo) -> String {
    let title = strip_controls(video.title.trim());
    if title.is_empty() {
        "未知".to_owned()
    } else {
        title
    }
}

fn category(video: &BilibiliVideo) -> String {
    video
        .category
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(strip_controls)
        .unwrap_or_else(|| "未知".to_owned())
}

fn owner_name(video: &BilibiliVideo) -> String {
    video
        .owner
        .name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(strip_controls)
        .unwrap_or_else(|| "未知".to_owned())
}

fn published(video: &BilibiliVideo) -> String {
    video
        .published_at
        .map(utc_time)
        .unwrap_or_else(|| "-".to_owned())
}

fn truncate_value(value: &str, max: usize) -> String {
    let value = strip_controls(value);
    if value.chars().count() <= max {
        value
    } else {
        value.chars().take(max).collect::<String>() + "…"
    }
}

pub fn definition(service: Arc<dyn BilibiliService>) -> CommandDefinition {
    BilibiliCommand::new(service).into_definition()
}
