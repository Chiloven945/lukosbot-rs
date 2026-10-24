use std::io::Cursor;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;

use lukosbot_command_engine::{
    ArgType, ArgsSpec, CommandContext, CommandDefinition, CommandError, CommandExecutor,
    CommandInvocation, CommandNode, InputSpec, PositionalSpec, SyntaxLine, SyntaxPart,
    UsageMetadata,
};
use lukosbot_integrations::{E621Md5, E621Post, E621Rating, E621Service};

use self::format::{
    allowed_ratings, extract_id_or_name, format_artist, format_post, parse_post_id, split_page,
};
use crate::integrations::error::map_integration_error;
use crate::integrations::format::truncate_reply;
use crate::rendering::Theme;

pub mod format;
pub mod grid;

const E621_IMAGE_HOSTS: &[&str] = &[
    "e621.net",
    "www.e621.net",
    "static1.e621.net",
    "static2.e621.net",
    "static3.e621.net",
];

pub struct E621Command {
    service: Arc<dyn E621Service>,
    limit: NonZeroU32,
}

impl E621Command {
    pub fn new(service: Arc<dyn E621Service>, limit: u32) -> Self {
        Self {
            service,
            limit: NonZeroU32::new(limit.clamp(1, 12)).expect("clamped E621 limit is non-zero"),
        }
    }

    pub fn into_definition(self) -> CommandDefinition {
        let command: Arc<dyn CommandExecutor> = Arc::new(self);
        let artist = CommandNode::leaf(
            "artist",
            "查询作者资料",
            InputSpec::Args(ArgsSpec {
                positionals: vec![PositionalSpec::new("id", ArgType::String)],
                options: Vec::new(),
            }),
            Arc::clone(&command),
        );
        let post = CommandNode::leaf(
            "post",
            "查询帖子详情",
            InputSpec::Args(ArgsSpec {
                positionals: vec![PositionalSpec::new("id", ArgType::String)],
                options: Vec::new(),
            }),
            Arc::clone(&command),
        );
        let search_artist = CommandNode::leaf(
            "artist",
            "搜索作者",
            InputSpec::Raw {
                name: "text".to_owned(),
                required: true,
            },
            Arc::clone(&command),
        );
        let search_post = CommandNode::leaf(
            "post",
            "搜索帖子",
            InputSpec::Raw {
                name: "text".to_owned(),
                required: true,
            },
            Arc::clone(&command),
        );
        let search_md5 = CommandNode::leaf(
            "md5",
            "按 MD5 查询帖子",
            InputSpec::Args(ArgsSpec {
                positionals: vec![PositionalSpec::new("md5", ArgType::String)],
                options: Vec::new(),
            }),
            command,
        );

        CommandDefinition::new(
            "e621",
            "查询 E621 上的作品和作者信息",
            InputSpec::NoArgs,
            Arc::new(Usage),
        )
        .child(
            CommandNode::group("get", "查询详情")
                .child(artist)
                .child(post),
        )
        .child(
            CommandNode::group("search", "搜索")
                .child(search_artist)
                .child(search_post)
                .child(search_md5),
        )
        .usage(UsageMetadata {
            syntaxes: vec![
                SyntaxLine::described(
                    "查询作者",
                    [
                        SyntaxPart::Literal("get".to_owned()),
                        SyntaxPart::Literal("artist".to_owned()),
                        SyntaxPart::Argument {
                            name: "id-or-name".to_owned(),
                            optional: false,
                        },
                    ],
                ),
                SyntaxLine::described(
                    "查询帖子",
                    [
                        SyntaxPart::Literal("get".to_owned()),
                        SyntaxPart::Literal("post".to_owned()),
                        SyntaxPart::Argument {
                            name: "id-or-link".to_owned(),
                            optional: false,
                        },
                    ],
                ),
                SyntaxLine::described(
                    "搜索作者（末尾正整数为页码）",
                    [
                        SyntaxPart::Literal("search".to_owned()),
                        SyntaxPart::Literal("artist".to_owned()),
                        SyntaxPart::Argument {
                            name: "text".to_owned(),
                            optional: false,
                        },
                    ],
                ),
                SyntaxLine::described(
                    "搜索帖子（末尾正整数为页码）",
                    [
                        SyntaxPart::Literal("search".to_owned()),
                        SyntaxPart::Literal("post".to_owned()),
                        SyntaxPart::Argument {
                            name: "text".to_owned(),
                            optional: false,
                        },
                    ],
                ),
                SyntaxLine::described(
                    "按 MD5 查询",
                    [
                        SyntaxPart::Literal("search".to_owned()),
                        SyntaxPart::Literal("md5".to_owned()),
                        SyntaxPart::Argument {
                            name: "md5".to_owned(),
                            optional: false,
                        },
                    ],
                ),
            ],
            examples: vec![
                "e621 get artist 123456".to_owned(),
                "e621 get post 123456".to_owned(),
                "e621 search post cat 2".to_owned(),
                "e621 search md5 0123456789abcdef0123456789abcdef".to_owned(),
            ],
            notes: vec!["结果会按当前聊天的 e621.rating 策略过滤。".to_owned()],
        })
    }

    async fn get_artist(&self, context: &CommandContext<'_>, id: &str) -> Result<(), CommandError> {
        let artist = self
            .service
            .get_artist(&extract_id_or_name(id, "artists")?)
            .await
            .map_err(|error| map_integration_error(error, "未找到该作者。", "作者参数无效。"))?
            .ok_or_else(|| CommandError::UserInput("未找到该作者。".to_owned()))?;
        context.reply_text(format_artist(&artist)).await?;
        Ok(())
    }

    async fn get_post(&self, context: &CommandContext<'_>, id: &str) -> Result<(), CommandError> {
        let id = parse_post_id(id)?;
        let post = self
            .service
            .get_post(id)
            .await
            .map_err(|error| map_integration_error(error, "未找到该帖子。", "帖子 ID 无效。"))?
            .ok_or_else(|| CommandError::UserInput("未找到该帖子。".to_owned()))?;
        self.ensure_rating(context, &post)?;
        context.reply_text(format_post(&post)).await?;
        Ok(())
    }

    async fn search_artist(
        &self,
        context: &CommandContext<'_>,
        raw: &str,
    ) -> Result<(), CommandError> {
        let (query, page) = split_page(raw);
        let page = NonZeroU32::new(page).unwrap_or(NonZeroU32::MIN);
        if page.get() > 750 {
            return Err(CommandError::UserInput(
                "页码必须在 1 到 750 之间。".to_owned(),
            ));
        }
        let limit = self.limit;
        let artists = self
            .service
            .search_artists(&query, page, limit)
            .await
            .map_err(|error| {
                map_integration_error(error, "未找到匹配的作者。", "搜索参数无效。")
            })?;
        if artists.is_empty() {
            context.reply_text("未找到匹配的结果。").await?;
            return Ok(());
        }

        let start = (page.get() - 1) * limit.get() + 1;
        let end = start + artists.len() as u32 - 1;
        let mut text = format!("\"{query}\"的搜索结果：\n\n");
        for artist in &artists {
            text.push_str(&format!("  - {}：{}\n", artist.id, artist.name));
        }
        text.push_str(&format!("\n当前显示第 {start} 至 {end} 条结果。"));
        context.reply_text(truncate_reply(&text)).await?;
        Ok(())
    }

    async fn search_post(
        &self,
        context: &CommandContext<'_>,
        raw: &str,
    ) -> Result<(), CommandError> {
        let (query, page) = split_page(raw);
        let page = NonZeroU32::new(page).unwrap_or(NonZeroU32::MIN);
        if page.get() > 750 {
            return Err(CommandError::UserInput(
                "页码必须在 1 到 750 之间。".to_owned(),
            ));
        }
        let limit = self.limit;
        let allowed = allowed_ratings(context);
        if allowed.is_empty() {
            context.reply_text("搜索结果已被策略过滤。").await?;
            return Ok(());
        }

        let effective = if allowed.contains(&E621Rating::Explicit) {
            query.clone()
        } else if query.is_empty() {
            "-rating:e".to_owned()
        } else {
            format!("{query} -rating:e")
        };
        let posts = self
            .service
            .search_posts(&effective, page, limit)
            .await
            .map_err(|error| {
                map_integration_error(error, "未找到匹配的帖子。", "搜索参数无效。")
            })?;
        let filtered = posts
            .into_iter()
            .filter(|post| post.is_allowed(&allowed))
            .collect::<Vec<_>>();
        if filtered.is_empty() {
            context.reply_text("未找到匹配的结果。").await?;
            return Ok(());
        }

        if let Err(error) = self.send_grid(context, &query, page.get(), &filtered).await {
            tracing::warn!(%error, "e621 grid rendering failed; falling back to text");
        }

        let start = (page.get() - 1) * limit.get() + 1;
        let end = start + filtered.len() as u32 - 1;
        let mut text = format!("\"{query}\"的搜索结果：\n\n");
        for post in &filtered {
            text.push_str(&format!(
                "  - {}：{}（{}）\n",
                post.id,
                post.artist_tags.join("、"),
                post.rating.as_str().to_ascii_uppercase()
            ));
        }
        text.push_str(&format!("\n当前显示第 {start} 至 {end} 条结果。"));
        context.reply_text(truncate_reply(&text)).await?;
        Ok(())
    }

    async fn send_grid(
        &self,
        context: &CommandContext<'_>,
        query: &str,
        page: u32,
        posts: &[E621Post],
    ) -> Result<(), CommandError> {
        let loader = Arc::clone(context.media());
        let policy = lukosbot_core::ports::MediaLoadPolicy::new(E621_IMAGE_HOSTS, 2 * 1024 * 1024);
        let mut tasks = tokio::task::JoinSet::new();
        for (index, post) in posts.iter().enumerate() {
            let Some(url) = post.thumbnail_url().cloned() else {
                continue;
            };
            let loader = Arc::clone(&loader);
            tasks.spawn(async move {
                let media = lukosbot_domain::MediaRef {
                    source: lukosbot_domain::MediaSource::Url(url.clone()),
                    file_name: url
                        .path_segments()
                        .and_then(Iterator::last)
                        .map(str::to_owned),
                    mime_type: None,
                    size_bytes: None,
                };
                let result =
                    tokio::time::timeout(Duration::from_secs(4), loader.load(&media, policy))
                        .await
                        .ok()
                        .and_then(Result::ok)
                        .and_then(|content| {
                            let dimensions =
                                image::ImageReader::new(Cursor::new(content.bytes.as_ref()))
                                    .with_guessed_format()
                                    .ok()?
                                    .into_dimensions()
                                    .ok()?;
                            if u64::from(dimensions.0) * u64::from(dimensions.1) > 40_000_000 {
                                return None;
                            }
                            image::load_from_memory(&content.bytes).ok()
                        })
                        .map(|image| image.to_rgba8());
                (index, result)
            });
        }

        let mut thumbnails = vec![None; posts.len()];
        while let Some(result) = tasks.join_next().await {
            if let Ok((index, Some(image))) = result
                && let Some(slot) = thumbnails.get_mut(index)
            {
                *slot = Some(image);
            }
        }

        let png = grid::render(query, page, posts, &thumbnails, Theme::Light)
            .map_err(|error| CommandError::internal(error.to_string()))?;
        context
            .reply_message(lukosbot_domain::OutboundMessage {
                parts: vec![lukosbot_domain::OutboundPart::Image {
                    media: lukosbot_domain::MediaRef {
                        source: lukosbot_domain::MediaSource::Bytes(png),
                        file_name: Some(format!("e621-posts-{page}.png")),
                        mime_type: Some("image/png".to_owned()),
                        size_bytes: None,
                    },
                    caption: None,
                }],
            })
            .await?;
        Ok(())
    }

    async fn search_md5(
        &self,
        context: &CommandContext<'_>,
        raw: &str,
    ) -> Result<(), CommandError> {
        let md5 = E621Md5::parse(raw).map_err(|error| {
            map_integration_error(
                error,
                "未找到该 MD5 对应的帖子。",
                "MD5 必须是 32 位十六进制。",
            )
        })?;
        let post = self
            .service
            .find_post_by_md5(&md5)
            .await
            .map_err(|error| {
                map_integration_error(error, "未找到该 MD5 对应的帖子。", "MD5 无效。")
            })?
            .ok_or_else(|| CommandError::UserInput("未找到该 MD5 对应的帖子。".to_owned()))?;
        self.ensure_rating(context, &post)?;
        context.reply_text(format_post(&post)).await?;
        Ok(())
    }

    fn ensure_rating(
        &self,
        context: &CommandContext<'_>,
        post: &E621Post,
    ) -> Result<(), CommandError> {
        if post.is_allowed(&allowed_ratings(context)) {
            Ok(())
        } else {
            Err(CommandError::UserInput(format!(
                "该内容分级为 {}，当前聊天因策略限制不可查看。",
                post.rating.as_str().to_ascii_uppercase()
            )))
        }
    }
}

#[async_trait]
impl CommandExecutor for E621Command {
    async fn execute(
        &self,
        context: &CommandContext<'_>,
        invocation: CommandInvocation,
    ) -> Result<(), CommandError> {
        let path = invocation
            .path
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        match path.as_slice() {
            [root] if *root == "e621" => {
                context.reply_text("用法：e621 get|search ...").await?;
                Ok(())
            }
            [root, "get", "artist"] if *root == "e621" => {
                let id = invocation
                    .args
                    .get::<String>("id")
                    .map_err(CommandError::internal)?;
                self.get_artist(context, &id).await
            }
            [root, "get", "post"] if *root == "e621" => {
                let id = invocation
                    .args
                    .get::<String>("id")
                    .map_err(CommandError::internal)?;
                self.get_post(context, &id).await
            }
            [root, "search", "artist"] if *root == "e621" => {
                self.search_artist(context, invocation.raw_tail.as_str())
                    .await
            }
            [root, "search", "post"] if *root == "e621" => {
                self.search_post(context, invocation.raw_tail.as_str())
                    .await
            }
            [root, "search", "md5"] if *root == "e621" => {
                let md5 = invocation
                    .args
                    .get::<String>("md5")
                    .map_err(CommandError::internal)?;
                self.search_md5(context, &md5).await
            }
            _ => Err(CommandError::internal("invalid e621 command path")),
        }
    }
}

struct Usage;

#[async_trait]
impl CommandExecutor for Usage {
    async fn execute(
        &self,
        context: &CommandContext<'_>,
        _invocation: CommandInvocation,
    ) -> Result<(), CommandError> {
        context.reply_text("用法：e621 get|search ...").await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
