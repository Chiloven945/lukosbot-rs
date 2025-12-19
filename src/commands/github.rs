// src/commands/github.rs
use std::{collections::HashMap, sync::Arc, time::Duration};

use anyhow::{anyhow, Result};
use azalea_brigadier::prelude::*;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, USER_AGENT};
use reqwest::Client;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use tracing::warn;
use url::Url;

use crate::config::ProxyConfig;
use crate::core::command_registry::BotCommand;
use crate::core::command_source::CommandSource;
use crate::core::dispatcher::CommandDispatcher;

const USAGE: &str = r#"用法：
`/github user <username>`     # 查询用户信息
`/github repo <owner>/<repo>` # 查询仓库信息
`/github search <keyword> [--top=<num>] [--lang=<lang>] [--sort=stars|updated] [--order=desc|asc]` - 搜索仓库
示例：
`/github user GitHub`
`/github repo Chiloven945/lukosbot2`
`/github search lukosbot --top=5 --lang=java --sort=stars --order=desc`
"#;

// -------------------- GitHub API types --------------------

#[derive(Debug, Deserialize)]
struct GhError {
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GhUser {
    login: String,
    name: Option<String>,
    html_url: String,
    public_repos: i64,
    followers: i64,
    following: i64,
}

#[derive(Debug, Deserialize)]
struct GhRepo {
    full_name: String,
    html_url: String,
    language: Option<String>,
    stargazers_count: i64,
    forks_count: i64,
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GhSearchResp {
    items: Vec<GhRepo>,
}

// -------------------- GitHubApi --------------------

pub struct GitHubApi {
    client: Client,
    token: Option<String>,
}

impl GitHubApi {
    const BASE: &'static str = "https://api.github.com";
    const CONN_TIMEOUT: Duration = Duration::from_millis(6000);
    const READ_TIMEOUT: Duration = Duration::from_millis(10000);

    pub fn new(token: Option<String>, proxy: &ProxyConfig) -> Self {
        let token = token.and_then(|t| {
            let t = t.trim().to_string();
            if t.is_empty() { None } else { Some(t) }
        });

        let mut builder = Client::builder()
            .connect_timeout(Self::CONN_TIMEOUT)
            .timeout(Self::READ_TIMEOUT);

        builder = proxy
            .apply_to_reqwest_builder(builder)
            .expect("apply proxy failed");

        let client = builder.build().expect("reqwest client build failed");
        Self { client, token }
    }

    async fn get_user(&self, username: &str) -> Result<GhUser> {
        self.get_typed(&format!("/users/{username}"), &[]).await
    }

    async fn get_repo(&self, owner: &str, repo: &str) -> Result<GhRepo> {
        self.get_typed(&format!("/repos/{owner}/{repo}"), &[]).await
    }

    async fn search_repos(
        &self,
        keywords: &str,
        sort: Option<&str>,
        order: Option<&str>,
        language: Option<&str>,
        per_page: usize,
    ) -> Result<GhSearchResp> {
        let mut full_q = keywords.to_string();
        if let Some(lang) = language {
            let lang = lang.trim();
            if !lang.is_empty() {
                full_q.push_str(" language:");
                full_q.push_str(lang);
            }
        }

        let mut q: Vec<(&str, String)> = vec![("q", full_q)];
        if let Some(sort) = sort.filter(|s| !s.trim().is_empty()) {
            q.push(("sort", sort.trim().to_string()));
        }
        if let Some(order) = order.filter(|s| !s.trim().is_empty()) {
            q.push(("order", order.trim().to_string()));
        }
        if per_page > 0 {
            q.push(("per_page", per_page.min(10).to_string()));
        }

        self.get_typed("/search/repositories", &q).await
    }

    async fn get_typed<T: DeserializeOwned>(&self, path: &str, query: &[(&str, String)]) -> Result<T> {
        let mut url = Url::parse(Self::BASE)?.join(path)?;
        {
            let mut qp = url.query_pairs_mut();
            for (k, v) in query {
                qp.append_pair(k, v);
            }
        }

        let mut headers = HeaderMap::new();
        headers.insert(
            ACCEPT,
            HeaderValue::from_static("application/vnd.github.v3+json"),
        );
        headers.insert(USER_AGENT, HeaderValue::from_static("lukosbot-rs"));
        if let Some(t) = &self.token {
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {t}"))?,
            );
        }

        let resp = self.client.get(url).headers(headers).send().await?;
        let status = resp.status();
        let body = resp.text().await?;

        if !status.is_success() {
            let msg = serde_json::from_str::<GhError>(&body)
                .ok()
                .and_then(|e| e.message)
                .or_else(|| status.canonical_reason().map(|s| s.to_string()))
                .unwrap_or_else(|| "HTTP error".to_string());
            return Err(anyhow!(msg));
        }

        let v = serde_json::from_str::<T>(&body).map_err(|e| anyhow!("bad json: {e}"))?;
        Ok(v)
    }
}

// -------------------- GitHubCommand --------------------

pub struct GitHubCommand {
    api: Arc<GitHubApi>,
}

impl GitHubCommand {
    pub fn new(token: Option<String>, proxy: &ProxyConfig) -> Self {
        Self {
            api: Arc::new(GitHubApi::new(token, proxy)),
        }
    }

    async fn handle_user(api: &GitHubApi, username: &str) -> String {
        match api.get_user(username).await {
            Ok(u) => {
                let display = u
                    .name
                    .as_deref()
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or(u.login.as_str());

                format!(
                    "用户: {display} ({login})\n主页: {url}\n公开仓库: {repos} | 粉丝: {followers} | 关注: {following}\n",
                    display = display,
                    login = u.login,
                    url = u.html_url,
                    repos = u.public_repos,
                    followers = u.followers,
                    following = u.following,
                )
            }
            Err(e) => {
                warn!("github user 查询失败: {username} err={e:?}");
                format!("找不到用户或请求失败：{username}")
            }
        }
    }

    async fn handle_repo(api: &GitHubApi, repo_arg: &str) -> String {
        let (owner, repo) = match repo_arg.split_once('/') {
            Some((a, b)) if !a.trim().is_empty() && !b.trim().is_empty() => (a.trim(), b.trim()),
            _ => return "仓库格式应为 owner/repo".to_string(),
        };

        match api.get_repo(owner, repo).await {
            Ok(r) => {
                let lang = r.language.as_deref().unwrap_or("未知");
                let desc = r
                    .description
                    .as_deref()
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or("无");

                format!(
                    "仓库: {full_name}\n主页: {url}\n语言: {lang} | Star: {stars} | Fork: {forks}\n描述: {desc}\n",
                    full_name = r.full_name,
                    url = r.html_url,
                    lang = lang,
                    stars = r.stargazers_count,
                    forks = r.forks_count,
                    desc = desc,
                )
            }
            Err(e) => {
                warn!("github repo 查询失败: {repo_arg} err={e:?}");
                format!("找不到仓库或请求失败：{repo_arg}")
            }
        }
    }

    async fn handle_search(api: &GitHubApi, q: &str) -> String {
        let p = Params::parse(q);

        match api
            .search_repos(
                &p.keywords,
                p.sort.as_deref(),
                p.order.as_deref(),
                p.language.as_deref(),
                p.top,
            )
            .await
        {
            Ok(resp) => {
                if resp.items.is_empty() {
                    return "未搜索到任何仓库。".to_string();
                }

                let count = resp.items.len().min(p.top);
                let mut sb = String::from("【仓库搜索结果】\n");
                for repo in resp.items.iter().take(count) {
                    sb.push_str(&repo.full_name);
                    sb.push_str(" - ");
                    sb.push_str(&format!("{}★\n", repo.stargazers_count));
                    sb.push_str(&repo.html_url);
                    sb.push_str("\n\n");
                }
                sb
            }
            Err(e) => {
                warn!("github search 失败: {q} err={e:?}");
                format!("搜索失败：{e}")
            }
        }
    }
}

impl BotCommand for GitHubCommand {
    fn name(&self) -> &'static str {
        "github"
    }

    fn description(&self) -> &'static str {
        "GitHub 查询工具"
    }

    fn usage(&self) -> &'static str {
        USAGE
    }

    fn register(&self, d: &mut CommandDispatcher<CommandSource>) {
        let usage = self.usage().to_string();

        let api_user = self.api.clone();
        let api_repo = self.api.clone();
        let api_search = self.api.clone();

        d.register(
            literal(self.name())
                .then(
                    literal("user").then(argument("username", greedy_string()).executes(
                        move |ctx: &CommandContext<CommandSource>| {
                            let username = get_string(ctx, "username").unwrap_or_default();
                            let src = ctx.source.clone();
                            let api = api_user.clone();

                            tokio::spawn(async move {
                                let text = GitHubCommand::handle_user(api.as_ref(), &username).await;
                                src.reply(text);
                            });

                            1
                        },
                    )),
                )
                .then(
                    literal("repo").then(argument("repo", greedy_string()).executes(
                        move |ctx: &CommandContext<CommandSource>| {
                            let repo_arg = get_string(ctx, "repo").unwrap_or_default();
                            let src = ctx.source.clone();
                            let api = api_repo.clone();

                            tokio::spawn(async move {
                                let text = GitHubCommand::handle_repo(api.as_ref(), &repo_arg).await;
                                src.reply(text);
                            });

                            1
                        },
                    )),
                )
                .then(
                    literal("search").then(argument("query", greedy_string()).executes(
                        move |ctx: &CommandContext<CommandSource>| {
                            let query = get_string(ctx, "query").unwrap_or_default();
                            let src = ctx.source.clone();
                            let api = api_search.clone();

                            tokio::spawn(async move {
                                let text = GitHubCommand::handle_search(api.as_ref(), &query).await;
                                src.reply(text);
                            });

                            1
                        },
                    )),
                )
                .executes(move |ctx: &CommandContext<CommandSource>| {
                    ctx.source.reply(usage.clone());
                    1
                }),
        );
    }
}

// -------------------- params parser --------------------

#[derive(Debug, Clone)]
struct Params {
    keywords: String,
    top: usize,
    language: Option<String>,
    sort: Option<String>,
    order: Option<String>,
}

impl Params {
    fn parse(input: &str) -> Self {
        let mut toks: Vec<&str> = input.trim().split_whitespace().collect();

        let mut opts: HashMap<String, String> = HashMap::new();
        for &t in &toks {
            if let Some(rest) = t.strip_prefix("--") {
                if let Some((k, v)) = rest.split_once('=') {
                    if !k.trim().is_empty() {
                        opts.insert(k.trim().to_string(), v.trim().to_string());
                    }
                }
            }
        }

        let keywords = toks
            .drain(..)
            .filter(|t| {
                if !t.starts_with("--") {
                    return true;
                }
                !t.contains('=')
            })
            .collect::<Vec<_>>()
            .join(" ");

        let mut top = 3usize;
        if let Some(v) = opts.get("top") {
            if let Ok(n) = v.parse::<usize>() {
                top = n;
            }
        }

        let keywords = if keywords.trim().is_empty() {
            "java".to_string()
        } else {
            keywords
        };

        let top = match top {
            0 => 3,
            n => n.min(10),
        };

        Self {
            keywords,
            top,
            language: opts.get("lang").cloned(),
            sort: opts.get("sort").cloned(),
            order: opts.get("order").cloned(),
        }
    }
}
