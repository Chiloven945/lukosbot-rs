use std::fmt::Write;
use std::sync::Arc;

use lukosbot_integrations::{
    BilibiliService, GitHubService, IpLookupService, MusicService, WhoisService,
};

pub mod bilibili;
pub mod error;
pub mod format;
pub mod github;
pub mod ip;
pub mod music;
pub mod whois;

/// Concrete services injected by the composition root.
#[derive(Clone, Default)]
pub struct IntegrationCommandServices {
    pub github: Option<Arc<dyn GitHubService>>,
    pub bilibili: Option<Arc<dyn BilibiliService>>,
    pub ip: Option<Arc<dyn IpLookupService>>,
    pub music: Option<Arc<dyn MusicService>>,
    pub whois: Option<Arc<dyn WhoisService>>,
}

pub(crate) fn usage_text(header: &str, lines: &[&str]) -> String {
    let mut text = String::from(header);
    for line in lines {
        let _ = write!(text, "\n  {line}");
    }
    text
}
