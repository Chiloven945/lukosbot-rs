use std::sync::Arc;

use async_trait::async_trait;

use lukosbot_command_engine::{
    CommandContext, CommandDefinition, CommandError, CommandExecutor, CommandInvocation,
    CommandNode, InputSpec, SyntaxLine, SyntaxPart, UsageMetadata,
};
use lukosbot_integrations::{
    IntegrationError, MusicLinkError, MusicPlatform, MusicService, TrackInfo, parse_music_link,
};

use super::error::map_integration_error;
use super::format::{duration_mmss, strip_controls, truncate_reply};
use super::usage_text;

pub struct MusicCommand {
    service: Arc<dyn MusicService>,
}

impl MusicCommand {
    pub fn new(service: Arc<dyn MusicService>) -> Self {
        Self { service }
    }

    pub fn into_definition(self) -> CommandDefinition {
        let command: Arc<dyn CommandExecutor> = Arc::new(self);

        CommandDefinition::new(
            "music",
            "从流媒体平台查询音乐信息",
            InputSpec::Raw {
                name: "query".to_owned(),
                required: false,
            },
            Arc::clone(&command),
        )
        .child(
            CommandNode::leaf(
                "spotify",
                "使用 Spotify 搜索歌曲",
                InputSpec::Raw {
                    name: "query".to_owned(),
                    required: false,
                },
                Arc::clone(&command),
            )
            .alias("s"),
        )
        .child(
            CommandNode::leaf(
                "soundcloud",
                "使用 SoundCloud 搜索歌曲",
                InputSpec::Raw {
                    name: "query".to_owned(),
                    required: false,
                },
                Arc::clone(&command),
            )
            .alias("sc"),
        )
        .child(CommandNode::leaf(
            "link",
            "通过链接解析歌曲",
            InputSpec::Raw {
                name: "link".to_owned(),
                required: true,
            },
            command,
        ))
        .usage(UsageMetadata {
            syntaxes: vec![
                SyntaxLine::described(
                    "搜索歌曲（默认优先 Spotify）",
                    [SyntaxPart::Argument {
                        name: "query".to_owned(),
                        optional: false,
                    }],
                ),
                SyntaxLine::described(
                    "指定平台搜索",
                    [
                        SyntaxPart::Literal("spotify|soundcloud".to_owned()),
                        SyntaxPart::Argument {
                            name: "query".to_owned(),
                            optional: false,
                        },
                    ],
                ),
                SyntaxLine::described(
                    "通过链接解析",
                    [
                        SyntaxPart::Literal("link".to_owned()),
                        SyntaxPart::Argument {
                            name: "url".to_owned(),
                            optional: false,
                        },
                    ],
                ),
            ],
            examples: vec![
                "music Never Gonna Give You Up".to_owned(),
                "music s Never Gonna Give You Up".to_owned(),
                "music link https://open.spotify.com/track/xxxxxxxx".to_owned(),
            ],
            notes: vec!["未指定平台时优先使用 Spotify，不可用时回退到 SoundCloud。".to_owned()],
        })
    }

    async fn search(
        &self,
        context: &CommandContext<'_>,
        query: &str,
        requested: Option<MusicPlatform>,
    ) -> Result<(), CommandError> {
        let query = query.trim();
        if query.is_empty() {
            return show_usage(context).await;
        }

        let available = self.service.available_platforms();
        let platform = match requested {
            Some(platform) if available.contains(&platform) => platform,
            Some(_) => {
                return Err(CommandError::UserInput(
                    "音乐平台暂未配置或不可用。".to_owned(),
                ));
            }
            None => match available.first() {
                Some(platform) => *platform,
                None => {
                    return Err(CommandError::UserInput(
                        "音乐平台暂未配置或不可用。".to_owned(),
                    ));
                }
            },
        };

        let track = self
            .service
            .search(platform, query)
            .await
            .map_err(|error| {
                map_integration_error(error, "没有找到匹配的歌曲。", "音乐搜索参数无效。")
            })?;
        context.reply_text(format_track(&track)).await?;
        Ok(())
    }

    async fn link(&self, context: &CommandContext<'_>, raw: &str) -> Result<(), CommandError> {
        let link = parse_music_link(raw.trim()).map_err(map_link_error)?;
        let track = self
            .service
            .resolve_link(&link)
            .await
            .map_err(|error| match error {
                IntegrationError::NotConfigured { .. } => {
                    CommandError::UserInput("音乐平台暂未配置或不可用。".to_owned())
                }
                error => map_integration_error(
                    error,
                    "无法从链接解析到歌曲信息。",
                    "无法从链接解析到歌曲信息。",
                ),
            })?;
        context.reply_text(format_track(&track)).await?;
        Ok(())
    }
}

#[async_trait]
impl CommandExecutor for MusicCommand {
    async fn execute(
        &self,
        context: &CommandContext<'_>,
        invocation: CommandInvocation,
    ) -> Result<(), CommandError> {
        match invocation.path.as_slice() {
            [root] if root == "music" => self.search(context, &invocation.raw_tail, None).await,
            [root, sub] if root == "music" => match sub.as_str() {
                "spotify" => {
                    self.search(context, &invocation.raw_tail, Some(MusicPlatform::Spotify))
                        .await
                }
                "soundcloud" => {
                    self.search(
                        context,
                        &invocation.raw_tail,
                        Some(MusicPlatform::SoundCloud),
                    )
                    .await
                }
                "link" => self.link(context, &invocation.raw_tail).await,
                _ => Err(CommandError::internal("unknown music subcommand")),
            },
            _ => Err(CommandError::internal("invalid music command path")),
        }
    }
}

fn map_link_error(error: MusicLinkError) -> CommandError {
    match error {
        MusicLinkError::UnsupportedPlatform => {
            CommandError::UserInput("无法识别链接所属平台。".to_owned())
        }
        MusicLinkError::InvalidUrl | MusicLinkError::InvalidTrackPath => {
            CommandError::UserInput("无法从链接解析到歌曲信息。".to_owned())
        }
    }
}

fn format_track(track: &TrackInfo) -> String {
    let mut lines = vec![
        format!("平台：{}", track.platform.display_name()),
        format!("标题：{}", value_or_unknown(&track.title)),
        format!("艺术家：{}", value_or_unknown(&track.artist)),
    ];

    if let Some(album) = non_blank(&track.album) {
        lines.push(format!("专辑：{album}"));
    }
    if let Some(duration) = track.duration.filter(|duration| !duration.is_zero()) {
        lines.push(format!("时长：{}", duration_mmss(duration)));
    }
    if let Some(url) = &track.track_url {
        lines.push(format!("链接：{url}"));
    }
    if let Some(url) = &track.cover_url {
        lines.push(format!("封面：{url}"));
    }

    truncate_reply(&lines.join("\n"))
}

fn value_or_unknown(value: &str) -> String {
    let value = strip_controls(value.trim());
    if value.is_empty() {
        "未知".to_owned()
    } else {
        value
    }
}

fn non_blank(value: &Option<String>) -> Option<String> {
    value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(strip_controls)
}

async fn show_usage(context: &CommandContext<'_>) -> Result<(), CommandError> {
    context
        .reply_text(usage_text(
            "用法：",
            &[
                "music <query...>",
                "music spotify|s <query...>",
                "music soundcloud|sc <query...>",
                "music link <url>",
            ],
        ))
        .await?;
    Ok(())
}

pub fn definition(service: Arc<dyn MusicService>) -> CommandDefinition {
    MusicCommand::new(service).into_definition()
}
