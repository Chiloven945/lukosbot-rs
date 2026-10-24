use std::fmt;
use std::path::PathBuf;

use serde::Deserialize;

use super::{IntegrationsSection, PlatformsSection, PolicySection, SecuritySection};

/// Root configuration object.
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AppConfig {
    pub app: AppSection,
    pub logging: LoggingSection,
    pub runtime: RuntimeSection,
    pub http: HttpSection,
    pub storage: StorageSection,
    pub platforms: PlatformsSection,
    pub commands: CommandsSection,
    pub security: SecuritySection,
    pub policy: PolicySection,
    pub integrations: IntegrationsSection,
    pub media: MediaSection,
    pub web: WebSection,
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AppSection {
    pub environment: Environment,
    pub shutdown_timeout_secs: u64,
}

impl Default for AppSection {
    fn default() -> Self {
        Self {
            environment: Environment::Development,
            shutdown_timeout_secs: 15,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Environment {
    #[default]
    Development,
    Production,
}

impl fmt::Display for Environment {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Development => "development",
            Self::Production => "production",
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LoggingSection {
    pub format: LogFormat,
    pub filter: String,
}

impl Default for LoggingSection {
    fn default() -> Self {
        Self {
            format: LogFormat::Pretty,
            filter: "info,sqlx=warn".to_owned(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    #[default]
    Pretty,
    Compact,
    Json,
}

impl fmt::Display for LogFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Pretty => "pretty",
            Self::Compact => "compact",
            Self::Json => "json",
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RuntimeSection {
    pub inbound_queue_capacity: usize,
    pub outbound_queue_capacity: usize,
    pub handler_timeout_secs: u64,
    pub send_timeout_secs: u64,
}

impl Default for RuntimeSection {
    fn default() -> Self {
        Self {
            inbound_queue_capacity: 256,
            outbound_queue_capacity: 128,
            handler_timeout_secs: 30,
            send_timeout_secs: 20,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HttpSection {
    pub connect_timeout_secs: u64,
    pub request_timeout_secs: u64,
    pub max_response_bytes: u64,
    pub user_agent: String,
    pub proxy: String,
}

impl HttpSection {
    /// Returns the configured proxy URL when one is set.
    pub fn proxy_url(&self) -> Option<&str> {
        let proxy = self.proxy.trim();
        (!proxy.is_empty()).then_some(proxy)
    }
}

impl Default for HttpSection {
    fn default() -> Self {
        Self {
            connect_timeout_secs: 5,
            request_timeout_secs: 20,
            max_response_bytes: 10 * 1024 * 1024,
            user_agent: "LukosBot/0.1 (+https://github.com/Chiloven945/lukosbot-rs)".to_owned(),
            proxy: String::new(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StorageSection {
    pub sqlite_path: PathBuf,
    pub busy_timeout_ms: u64,
    pub max_connections: u32,
    pub create_parent_dirs: bool,
    pub expiry_cleanup_interval_secs: u64,
}

impl Default for StorageSection {
    fn default() -> Self {
        Self {
            sqlite_path: PathBuf::from("./data/lukosbot.sqlite3"),
            busy_timeout_ms: 5_000,
            max_connections: 5,
            create_parent_dirs: true,
            expiry_cleanup_interval_secs: 3_600,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MediaSection {
    pub max_download_bytes: u64,
    pub max_upload_bytes: u64,
    pub max_thumbnail_bytes: u64,
    pub max_image_pixels: u64,
    pub max_concurrent_downloads: usize,
    pub rendering: RenderingSection,
}

impl Default for MediaSection {
    fn default() -> Self {
        Self {
            max_download_bytes: 8 * 1024 * 1024,
            max_upload_bytes: 8 * 1024 * 1024,
            max_thumbnail_bytes: 2 * 1024 * 1024,
            max_image_pixels: 40_000_000,
            max_concurrent_downloads: 4,
            rendering: RenderingSection::default(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RenderingSection {
    pub theme: ImageTheme,
    pub max_grid_posts: u8,
    pub max_render_pixels: u64,
    pub font_dirs: Vec<PathBuf>,
}

impl Default for RenderingSection {
    fn default() -> Self {
        Self {
            theme: ImageTheme::Light,
            max_grid_posts: 12,
            max_render_pixels: 20_000_000,
            font_dirs: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageTheme {
    #[default]
    Light,
    Dark,
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WebSection {
    pub request_timeout_secs: u64,
    pub max_html_bytes: u64,
    pub max_markdown_bytes: u64,
    pub max_redirects: u8,
    pub user_agent: String,
    pub browser: BrowserSection,
}

impl Default for WebSection {
    fn default() -> Self {
        Self {
            request_timeout_secs: 15,
            max_html_bytes: 10 * 1024 * 1024,
            max_markdown_bytes: 2 * 1024 * 1024,
            max_redirects: 5,
            user_agent: "Mozilla/5.0 (compatible; LukosBot/0.1)".to_owned(),
            browser: BrowserSection::default(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BrowserSection {
    pub enabled: bool,
    pub webdriver_url: String,
    pub page_load_timeout_secs: u64,
    pub job_timeout_secs: u64,
    pub queue_capacity: usize,
    pub full_page_width: u32,
    pub full_page_max_height: u32,
    pub wiki_intro_width: u32,
}

impl Default for BrowserSection {
    fn default() -> Self {
        Self {
            enabled: false,
            webdriver_url: "http://127.0.0.1:4444".to_owned(),
            page_load_timeout_secs: 20,
            job_timeout_secs: 25,
            queue_capacity: 4,
            full_page_width: 1080,
            full_page_max_height: 8_000,
            wiki_intro_width: 1380,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CommandsSection {
    pub prefix: String,
    pub basic: BasicCommandSettings,
    pub twenty_four: TwentyFourSettings,
    pub control: ControlCommandSettings,
    pub integrations: IntegrationCommandSettings,
    pub media: MediaCommandSettings,
    pub web: WebCommandSettings,
}

impl Default for CommandsSection {
    fn default() -> Self {
        Self {
            prefix: "/".to_owned(),
            basic: BasicCommandSettings::default(),
            twenty_four: TwentyFourSettings::default(),
            control: ControlCommandSettings::default(),
            integrations: IntegrationCommandSettings::default(),
            media: MediaCommandSettings::default(),
            web: WebCommandSettings::default(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MediaCommandSettings {
    pub cave: bool,
    pub e621: bool,
}

impl Default for MediaCommandSettings {
    fn default() -> Self {
        Self {
            cave: true,
            e621: true,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WebCommandSettings {
    pub wiki: bool,
    pub mcwiki: bool,
}

impl Default for WebCommandSettings {
    fn default() -> Self {
        Self {
            wiki: true,
            mcwiki: true,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct IntegrationCommandSettings {
    pub github: bool,
    pub bilibili: bool,
    pub ip: bool,
    pub music: bool,
    pub whois: bool,
}

impl Default for IntegrationCommandSettings {
    fn default() -> Self {
        Self {
            github: true,
            bilibili: true,
            ip: true,
            music: true,
            whois: true,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ControlCommandSettings {
    pub admin: bool,
    pub pref: bool,
    pub service: bool,
}

impl Default for ControlCommandSettings {
    fn default() -> Self {
        Self {
            admin: true,
            pref: true,
            service: true,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BasicCommandSettings {
    pub ping: bool,
    pub echo: bool,
    pub coin: bool,
    pub dice: bool,
    pub luck: bool,
    pub twenty_four: bool,
    pub weather: bool,
}

impl Default for BasicCommandSettings {
    fn default() -> Self {
        Self {
            ping: true,
            echo: true,
            coin: true,
            dice: true,
            luck: true,
            twenty_four: true,
            weather: true,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TwentyFourSettings {
    pub time_limit_secs: u64,
}

impl Default for TwentyFourSettings {
    fn default() -> Self {
        Self {
            time_limit_secs: 300,
        }
    }
}
