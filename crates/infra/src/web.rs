//! Static wiki retrieval and browser-backed screenshots.

pub mod screenshot;
pub mod wiki;

pub use screenshot::{ScreenshotHandle, ScreenshotSettings, ScreenshotWorker};

use std::time::Duration;

#[derive(Debug, Clone)]
pub struct WebSettings {
    pub request_timeout: Duration,
    pub max_html_bytes: u64,
    pub max_markdown_bytes: u64,
    pub max_redirects: usize,
    pub user_agent: String,
}

impl Default for WebSettings {
    fn default() -> Self {
        Self {
            request_timeout: Duration::from_secs(15),
            max_html_bytes: 10 * 1024 * 1024,
            max_markdown_bytes: 2 * 1024 * 1024,
            max_redirects: 5,
            user_agent: "Mozilla/5.0 (compatible; LukosBot/0.1)".to_owned(),
        }
    }
}
