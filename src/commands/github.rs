// src/commands/github.rs
use std::{collections::HashMap, sync::Arc, time::Duration};

use crate::config::ProxyConfig;
use crate::core::command_registry::BotCommand;
use crate::core::command_source::CommandSource;
use crate::core::dispatcher::CommandDispatcher;
use anyhow::{anyhow, Result};
use azalea_brigadier::prelude::*;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, USER_AGENT};
use reqwest::Client;
use serde_yaml::Value;
use tracing::warn;
use url::Url;

const USAGE: &str = r#"用法：
`/github user <username>`     # 查询用户信息
`/github repo <owner>/<repo>` # 查询仓库信息
`/github search <keyword> [--top=<num>] [--lang=<lang>] [--sort=stars|updated] [--order=desc|asc]` - 搜索仓库
示例：
`/github user GitHub`
`/github repo Chiloven945/lukosbot2`
`/github search lukosbot --top=5 --lang=java --sort=stars --order=desc`
"#;

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
        let mut builder = Client::builder()
            .connect_timeout(Self::CONN_TIMEOUT)
            .timeout(Self::READ_TIMEOUT);

        builder = proxy
            .apply_to_reqwest_builder(builder)
            .expect("apply proxy failed");

        let client = builder.build().expect("reqwest client build failed");

        Self { client, token }
    }

    pub async fn get_user(&self, username: &str) -> Result<Value> {
        self.get(&format!("/users/{username}"), &[]).await
    }

    pub async fn get_repo(&self, owner: &str, repo: &str) -> Result<Value> {
        self.get(&format!("/repos/{owner}/{repo}"), &[]).await
    }

    pub async fn search_repos(
        &self,
        keywords: &str,
        sort: Option<&str>,
        order: Option<&str>,
        language: Option<&str>,
        per_page: usize,
    ) -> Result<Value> {
        let mut full_q = keywords.to_string();
        if let Some(lang) = language {
            if !lang.trim().is_empty() {
                full_q.push_str(" language:");
                full_q.push_str(lang.trim());
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

        self.get("/search/repositories", &q).await
    }

    async fn get(&self, path: &str, query: &[(&str, String)]) -> Result<Value> {
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

        let v: Value = serde_yaml::from_str(&body).map_err(|e| anyhow!("bad json: {e}"))?;

        if status.as_u16() >= 400 {
            let msg = v
                .get("message")
                .and_then(|x| x.as_str())
                .unwrap_or_else(|| status.canonical_reason().unwrap_or("HTTP error"));
            return Err(anyhow!(msg.to_string()));
        }

        Ok(v)
    }
}

// -------------------- GitHubCommand (ported from Java) --------------------

pub struct GitHubCommand {
    api: Arc<GitHubApi>,
}

impl GitHubCommand {
    async fn handle_user(api: &GitHubApi, username: &str) -> String {
        match api.get_user(username).await {
            Ok(obj) => {
                let login = get_s(&obj, "login").unwrap_or("");
                let name = get_s(&obj, "name").unwrap_or("");
                let url = get_s(&obj, "html_url").unwrap_or("");
                let repos = get_i(&obj, "public_repos");
                let followers = get_i(&obj, "followers");
                let following = get_i(&obj, "following");

                let display = if name.trim().is_empty() { login } else { name };
                format!(
                    "用户: {display} ({login})\n主页: {url}\n公开仓库: {repos} | 粉丝: {followers} | 关注: {following}\n"
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
            Ok(obj) => {
                let full_name = get_s(&obj, "full_name").unwrap_or("");
                let url = get_s(&obj, "html_url").unwrap_or("");
                let lang = get_s(&obj, "language").unwrap_or("未知");
                let stars = get_i(&obj, "stargazers_count");
                let forks = get_i(&obj, "forks_count");
                let desc = get_s(&obj, "description").unwrap_or("无");
                let desc = if desc.trim().is_empty() { "无" } else { desc };

                format!(
                    "仓库: {full_name}\n主页: {url}\n语言: {lang} | Star: {stars} | Fork: {forks}\n描述: {desc}\n"
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
            Ok(result) => {
                let items = result.get("items").and_then(|v| v.as_sequence());
                let Some(items) = items else {
                    return "未搜索到任何仓库。".to_string();
                };
                if items.is_empty() {
                    return "未搜索到任何仓库。".to_string();
                }

                let count = items.len().min(p.top);
                let mut sb = String::from("【仓库搜索结果】\n");
                for repo in items.iter().take(count) {
                    let full_name = get_s(repo, "full_name").unwrap_or("");
                    let stars = get_i(repo, "stargazers_count");
                    let url = get_s(repo, "html_url").unwrap_or("");
                    sb.push_str(full_name);
                    sb.push_str(" - ");
                    sb.push_str(&format!("{stars}★\n"));
                    sb.push_str(url);
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

    pub fn new(token: Option<String>, proxy: &ProxyConfig) -> Self {
        Self {
            api: Arc::new(GitHubApi::new(token, proxy)),
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
                                let text =
                                    GitHubCommand::handle_user(api.as_ref(), &username).await;
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
                                let text =
                                    GitHubCommand::handle_repo(api.as_ref(), &repo_arg).await;
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

// -------------------- helpers (ported from Java small utils) --------------------

fn get_s<'a>(obj: &'a Value, k: &str) -> Option<&'a str> {
    obj.get(k)?.as_str()
}

fn get_i(obj: &Value, k: &str) -> i64 {
    obj.get(k).and_then(|v| v.as_i64()).unwrap_or(0)
}

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

        let top = if top == 0 { 3 } else { top.min(10) };

        Self {
            keywords,
            top,
            language: opts.get("lang").cloned(),
            sort: opts.get("sort").cloned(),
            order: opts.get("order").cloned(),
        }
    }
}
