use std::sync::Arc;

use async_trait::async_trait;

use lukosbot_command_engine::{
    ArgType, ArgValue, ArgsSpec, CommandContext, CommandDefinition, CommandError, CommandExecutor,
    CommandInvocation, CommandNode, InputSpec, OptionSpec, PositionalSpec, SyntaxLine, SyntaxPart,
    UsageMetadata, Validator,
};
use lukosbot_integrations::{
    GitHubOrder, GitHubRepository, GitHubSearchParams, GitHubSearchResult, GitHubService,
    GitHubSort, GitHubUser,
};

use super::error::map_integration_error;
use super::format::{strip_controls, truncate_reply};
use super::usage_text;

pub struct GitHubCommand {
    service: Arc<dyn GitHubService>,
}

impl GitHubCommand {
    pub fn new(service: Arc<dyn GitHubService>) -> Self {
        Self { service }
    }

    pub fn into_definition(self) -> CommandDefinition {
        let command: Arc<dyn CommandExecutor> = Arc::new(self);

        CommandDefinition::new(
            "github",
            "GitHub 查询工具",
            InputSpec::NoArgs,
            Arc::clone(&command),
        )
        .alias("gh")
        .child(CommandNode::leaf(
            "user",
            "查询用户信息",
            InputSpec::Args(ArgsSpec {
                positionals: vec![
                    PositionalSpec::new("username", ArgType::String).description("GitHub 用户名"),
                ],
                options: Vec::new(),
            }),
            Arc::clone(&command),
        ))
        .child(CommandNode::leaf(
            "repo",
            "查询仓库信息",
            InputSpec::Args(ArgsSpec {
                positionals: vec![
                    PositionalSpec::new("repository", ArgType::String)
                        .description("仓库名，格式 owner/repo"),
                ],
                options: Vec::new(),
            }),
            Arc::clone(&command),
        ))
        .child(CommandNode::leaf(
            "search",
            "搜索仓库",
            InputSpec::Args(ArgsSpec {
                positionals: vec![
                    PositionalSpec::new("keyword", ArgType::String)
                        .greedy()
                        .description("搜索关键字"),
                ],
                options: vec![
                    OptionSpec::new("top", ["--top"], ArgType::I64)
                        .default(ArgValue::I64(3))
                        .description("返回数量（1-10）")
                        .validator(Validator::new(|value| match value {
                            ArgValue::I64(top) if (1..=10).contains(top) => Ok(()),
                            _ => Err("返回数量必须在 1 到 10 之间。".to_owned()),
                        })),
                    OptionSpec::new("lang", ["--lang"], ArgType::String).description("语言过滤"),
                    OptionSpec::new(
                        "sort",
                        ["--sort"],
                        ArgType::Choice(vec!["stars".to_owned(), "updated".to_owned()]),
                    )
                    .description("排序字段"),
                    OptionSpec::new(
                        "order",
                        ["--order"],
                        ArgType::Choice(vec!["asc".to_owned(), "desc".to_owned()]),
                    )
                    .description("排序方向"),
                ],
            }),
            command,
        ))
        .usage(UsageMetadata {
            syntaxes: vec![
                SyntaxLine::described(
                    "查询用户",
                    [
                        SyntaxPart::Literal("user".to_owned()),
                        SyntaxPart::Argument {
                            name: "username".to_owned(),
                            optional: false,
                        },
                    ],
                ),
                SyntaxLine::described(
                    "查询仓库",
                    [
                        SyntaxPart::Literal("repo".to_owned()),
                        SyntaxPart::Argument {
                            name: "owner/repo".to_owned(),
                            optional: false,
                        },
                    ],
                ),
                SyntaxLine::described(
                    "搜索仓库",
                    [
                        SyntaxPart::Literal("search".to_owned()),
                        SyntaxPart::Argument {
                            name: "keyword".to_owned(),
                            optional: false,
                        },
                        SyntaxPart::Option {
                            names: vec!["--top".to_owned()],
                            value_name: Some("N".to_owned()),
                            optional: true,
                        },
                    ],
                ),
            ],
            examples: vec![
                "github user GitHub".to_owned(),
                "github repo Chiloven945/lukosbot-rs".to_owned(),
                "github search rust bot --top=5 --lang=rust".to_owned(),
            ],
            notes: vec!["未配置 Token 时仍可查询公开资源，但会受到匿名 API 限额限制。".to_owned()],
        })
    }

    async fn user(
        &self,
        context: &CommandContext<'_>,
        invocation: &CommandInvocation,
    ) -> Result<(), CommandError> {
        let username = invocation
            .args
            .get::<String>("username")
            .map_err(CommandError::internal)?;
        if username.trim().is_empty() {
            return Err(CommandError::UserInput("请输入 GitHub 用户名。".to_owned()));
        }

        let user = self.service.user(username.trim()).await.map_err(|error| {
            map_integration_error(error, "未找到该 GitHub 用户。", "GitHub 用户名无效。")
        })?;
        context.reply_text(format_user(&user)).await?;
        Ok(())
    }

    async fn repository(
        &self,
        context: &CommandContext<'_>,
        invocation: &CommandInvocation,
    ) -> Result<(), CommandError> {
        let raw = invocation
            .args
            .get::<String>("repository")
            .map_err(CommandError::internal)?;
        let parts: Vec<&str> = raw.split('/').collect();
        if parts.len() != 2 || parts.iter().any(|part| part.trim().is_empty()) {
            return Err(CommandError::UserInput(
                "仓库格式应为 owner/repo。".to_owned(),
            ));
        }

        let repository = self
            .service
            .repository(parts[0].trim(), parts[1].trim())
            .await
            .map_err(|error| {
                map_integration_error(error, "未找到该 GitHub 仓库。", "GitHub 仓库名无效。")
            })?;
        context.reply_text(format_repository(&repository)).await?;
        Ok(())
    }

    async fn search(
        &self,
        context: &CommandContext<'_>,
        invocation: &CommandInvocation,
    ) -> Result<(), CommandError> {
        let query = invocation
            .args
            .get::<String>("keyword")
            .map_err(CommandError::internal)?;
        if query.trim().is_empty() {
            return Err(CommandError::UserInput("请输入搜索关键字。".to_owned()));
        }

        let top = invocation
            .args
            .get::<i64>("top")
            .map_err(CommandError::internal)?;
        let language = invocation
            .args
            .get_opt::<String>("lang")
            .map_err(CommandError::internal)?;
        let sort = invocation
            .args
            .get_opt::<String>("sort")
            .map_err(CommandError::internal)?;
        let order = invocation
            .args
            .get_opt::<String>("order")
            .map_err(CommandError::internal)?;

        let params = GitHubSearchParams {
            query: query.trim().to_owned(),
            top: u8::try_from(top).unwrap_or(1),
            language: language.filter(|value| !value.trim().is_empty()),
            sort: sort.as_deref().and_then(parse_sort),
            order: order.as_deref().and_then(parse_order),
        };

        let result = self.service.search(&params).await.map_err(|error| {
            map_integration_error(error, "未找到匹配的 GitHub 仓库。", "GitHub 搜索参数无效。")
        })?;
        context.reply_text(format_search(&result)).await?;
        Ok(())
    }
}

#[async_trait]
impl CommandExecutor for GitHubCommand {
    async fn execute(
        &self,
        context: &CommandContext<'_>,
        invocation: CommandInvocation,
    ) -> Result<(), CommandError> {
        match invocation.path.as_slice() {
            [root] if root == "github" => {
                context
                    .reply_text(usage_text(
                        "用法：",
                        &[
                            "github user <username>",
                            "github repo <owner/repo>",
                            "github search <keyword...> [--top N] [--lang LANG] [--sort stars|updated] [--order asc|desc]",
                        ],
                    ))
                    .await?;
                Ok(())
            }
            [root, sub] if root == "github" => match sub.as_str() {
                "user" => self.user(context, &invocation).await,
                "repo" => self.repository(context, &invocation).await,
                "search" => self.search(context, &invocation).await,
                _ => Err(CommandError::internal("unknown github subcommand")),
            },
            _ => Err(CommandError::internal("invalid github command path")),
        }
    }
}

fn parse_sort(value: &str) -> Option<GitHubSort> {
    match value {
        "stars" => Some(GitHubSort::Stars),
        "updated" => Some(GitHubSort::Updated),
        _ => None,
    }
}

fn parse_order(value: &str) -> Option<GitHubOrder> {
    match value {
        "asc" => Some(GitHubOrder::Asc),
        "desc" => Some(GitHubOrder::Desc),
        _ => None,
    }
}

fn format_user(user: &GitHubUser) -> String {
    let display_name = user
        .name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(&user.login);
    let url = user
        .html_url
        .as_ref()
        .map(|url| url.as_str())
        .unwrap_or("无");
    truncate_reply(&format!(
        "用户：{}（{}）\n主页：{}\n公开仓库：{} | 粉丝：{} | 关注：{}",
        strip_controls(display_name),
        strip_controls(&user.login),
        url,
        user.public_repos,
        user.followers,
        user.following
    ))
}

fn format_repository(repository: &GitHubRepository) -> String {
    let name = strip_controls(&repository.full_name);
    let url = repository
        .html_url
        .as_ref()
        .map(|url| url.as_str())
        .unwrap_or("无");
    let language = repository
        .language
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("未知");
    let description = repository
        .description
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| truncate_value(value, 160))
        .unwrap_or_else(|| "无".to_owned());

    truncate_reply(&format!(
        "仓库：{name}\n主页：{url}\n语言：{language} | 收藏：{} | 分叉：{}\n描述：{description}",
        repository.stargazers_count, repository.forks_count
    ))
}

fn format_search(result: &GitHubSearchResult) -> String {
    if result.items.is_empty() {
        return "未搜索到任何仓库。".to_owned();
    }

    let mut text = format!("仓库搜索结果（共 {} 个）：\n", result.total_count);
    for repository in &result.items {
        text.push_str(&format!(
            "\n{} - {}★\n",
            strip_controls(&repository.full_name),
            repository.stargazers_count
        ));
        if let Some(url) = &repository.html_url {
            text.push_str(url.as_str());
            text.push('\n');
        }
    }
    truncate_reply(text.trim_end())
}

fn truncate_value(value: &str, max: usize) -> String {
    let value = strip_controls(value);
    if value.chars().count() <= max {
        value
    } else {
        value.chars().take(max).collect::<String>() + "…"
    }
}

pub fn definition(service: Arc<dyn GitHubService>) -> CommandDefinition {
    GitHubCommand::new(service).into_definition()
}
