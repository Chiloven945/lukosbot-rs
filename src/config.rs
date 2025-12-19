use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

use config::{Config, Environment, File};
use regex::Regex;
use url::Url;

const CONFIG_FILE: &str = "config/application.toml";
const TEMPLATE_TOML: &str = include_str!("../resources/application.example.toml");

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct AppProperties {
    pub prefix: String,
    pub language: String,

    pub telegram: Telegram,
    pub discord: Discord,
    pub onebot: Onebot,

    pub commands: CommandsConfig,

    pub proxy: ProxyConfig,
}

impl Default for AppProperties {
    fn default() -> Self {
        Self {
            prefix: "/".to_string(),
            language: "zh-cn".to_string(),
            telegram: Telegram::default(),
            discord: Discord::default(),
            onebot: Onebot::default(),
            commands: CommandsConfig::default(),
            proxy: ProxyConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "kebab-case")]
pub struct Telegram {
    pub enabled: bool,
    pub bot_token: String,
    pub bot_username: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "kebab-case")]
pub struct Discord {
    pub enabled: bool,
    pub token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "kebab-case")]
pub struct Onebot {
    pub enabled: bool,
    pub ws_url: String,
    pub access_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "kebab-case")]
pub struct CommandsConfig {
    pub switch: SwitchConfig,
    pub github: GitHubConfig,
    pub music: MusicConfig,
    pub translate: TranslateConfig,
    pub twenty_four: TwentyFourConfig,
    pub control: ControlConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "kebab-case")]
pub struct SwitchConfig {
    pub weather: bool,
    pub translate: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "kebab-case")]
pub struct GitHubConfig {
    pub enabled: bool,
    pub token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "kebab-case")]
pub struct MusicConfig {
    pub spotify: SpotifyConfig,
    pub soundcloud: SoundcloudConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "kebab-case")]
pub struct SpotifyConfig {
    pub enabled: bool,
    pub client_id: String,
    pub client_secret: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "kebab-case")]
pub struct SoundcloudConfig {
    pub enabled: bool,
    pub client_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "kebab-case")]
pub struct TranslateConfig {
    pub default_lang: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "kebab-case")]
pub struct TwentyFourConfig {
    pub time_limit: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "kebab-case")]
pub struct ControlConfig {
    pub weather: bool,
    pub translate: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct ProxyConfig {
    pub enabled: bool,
    #[serde(rename = "type")]
    pub proxy_type: ProxyType,

    pub host: String,
    pub port: u16,

    pub username: String,
    pub password: String,

    pub non_proxy_hosts_list: Vec<String>,
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            proxy_type: ProxyType::None,
            host: String::new(),
            port: 0,
            username: String::new(),
            password: String::new(),
            non_proxy_hosts_list: vec![],
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ProxyType {
    #[serde(alias = "NONE", alias = "none")]
    None,
    #[serde(alias = "HTTPS", alias = "HTTP", alias = "https", alias = "http")]
    Https,
    #[serde(alias = "SOCKS5", alias = "socks5")]
    Socks5,
}

impl ProxyConfig {
    fn valid_endpoint(&self) -> bool {
        !self.host.trim().is_empty() && self.port > 0 && !matches!(self.proxy_type, ProxyType::None)
    }

    fn build_proxy_url(&self) -> Result<Url> {
        if !self.valid_endpoint() {
            return Err(anyhow!("proxy enabled but host/port/type invalid"));
        }

        // reqwest: HTTP proxy 用 http://host:port；SOCKS5 推荐 socks5h://host:port（DNS 也走代理）
        let scheme = match self.proxy_type {
            ProxyType::Https => "http",
            ProxyType::Socks5 => "socks5h",
            ProxyType::None => unreachable!(),
        };

        let mut proxy = Url::parse(&format!("{scheme}://{}:{}/", self.host.trim(), self.port))?;

        let u = self.username.trim();
        if !u.is_empty() {
            // user/pass 放到 URL 里即可（reqwest 会用它做代理认证）
            let _ = proxy.set_username(u);
            let p = self.password.as_str();
            let _ = proxy.set_password(Some(p));
        }

        Ok(proxy)
    }

    fn compile_bypass_regexes(&self) -> Vec<Regex> {
        self.non_proxy_hosts_list
            .iter()
            .filter_map(|pat| {
                let pat = pat.trim();
                if pat.is_empty() {
                    return None;
                }
                // "*.local" / "127.*" 这种简单 glob -> regex
                let re = format!("^{}$", regex::escape(pat).replace("\\*", ".*"));
                Regex::new(&re).ok()
            })
            .collect()
    }

    fn is_bypassed_host(&self, host: &str, bypass: &[Regex]) -> bool {
        let h = host.trim();
        if h.is_empty() {
            return false;
        }
        bypass.iter().any(|r| r.is_match(h))
    }

    /// ✅ 注入到 reqwest::ClientBuilder（对齐 Java 的 applyTo(OkHttpClient.Builder) 思路）&#8203;:contentReference[oaicite:2]{index=2}
    pub fn apply_to_reqwest_builder(
        &self,
        builder: reqwest::ClientBuilder,
    ) -> Result<reqwest::ClientBuilder> {
        if !self.enabled || matches!(self.proxy_type, ProxyType::None) {
            return Ok(builder);
        }

        // host/port 不对就跳过（对齐你 Java 的健壮性处理）&#8203;:contentReference[oaicite:3]{index=3}
        if !self.valid_endpoint() {
            return Ok(builder);
        }

        let proxy_url = self.build_proxy_url()?;
        let bypass = self.compile_bypass_regexes();

        // custom：按 URL host 决定是否走代理
        let p = proxy_url.clone();
        let proxy = reqwest::Proxy::custom(move |url| {
            let host = url.host_str().unwrap_or("");
            if bypass.iter().any(|r| r.is_match(host)) {
                None
            } else {
                Some(p.clone())
            }
        });

        Ok(builder.proxy(proxy))
    }
}

fn normalize_lf(s: &str) -> String {
    s.replace("\r\n", "\n").trim().to_string()
}

/// 读配置：
/// 1) 文件不存在 -> 写模板
/// 2) 仅基于“文件内容”反序列化一次（serde default 自动补全）-> pretty 序列化 -> 如有变化则写回（实现缺失项补全）
/// 3) 再用 config crate 读取（file + env override）得到最终运行时配置（不把 env 写回文件）
pub fn load_or_init() -> Result<AppProperties> {
    if !Path::new(CONFIG_FILE).exists() {
        fs::create_dir_all("config").context("create config dir")?;
        fs::write(CONFIG_FILE, TEMPLATE_TOML).context("write default application.toml")?;
    }

    // -------- 2) 缺失项补全并写回（只基于文件，不吃 env）--------
    let file_txt = fs::read_to_string(CONFIG_FILE).context("read application.toml")?;

    // serde(default) + Default => 自动补全缺失字段
    let file_props: AppProperties =
        toml::from_str(&file_txt).context("parse application.toml (toml)")?;

    // pretty 输出完整配置
    let canonical = toml::to_string_pretty(&file_props).context("dump canonical toml")?;

    if normalize_lf(&file_txt) != normalize_lf(&canonical) {
        fs::write(CONFIG_FILE, canonical).context("rewrite application.toml")?;
    }

    // -------- 3) 最终运行时配置（file + env override）--------
    let cfg = Config::builder()
        .add_source(File::from(Path::new(CONFIG_FILE)))
        .add_source(Environment::with_prefix("LUKOS").separator("__"))
        .build()
        .context("build config")?;

    let props: AppProperties = cfg.try_deserialize().context("deserialize config")?;
    Ok(props)
}
