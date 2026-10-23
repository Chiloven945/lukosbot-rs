use async_trait::async_trait;
use chrono::{DateTime, Utc};
use url::Url;

use crate::error::IntegrationError;

/// Hosts whose URLs may be accepted by `/bilibili`.
///
/// Matching is exact; a suffix check would accept attacker-controlled hosts such as
/// `bilibili.com.example.org`.
pub const BILIBILI_HOSTS: &[&str] = &[
    "bilibili.com",
    "www.bilibili.com",
    "m.bilibili.com",
    "b23.tv",
    "www.b23.tv",
];

const SHORT_LINK_HOSTS: &[&str] = &["b23.tv", "www.b23.tv"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VideoId {
    Bv(String),
    Av(u64),
}

impl VideoId {
    pub fn normalized(&self) -> String {
        match self {
            Self::Bv(value) => value.clone(),
            Self::Av(value) => format!("av{value}"),
        }
    }

    pub fn canonical_url(&self) -> Result<Url, url::ParseError> {
        Url::parse(&format!(
            "https://www.bilibili.com/video/{}",
            self.normalized()
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BilibiliTarget {
    Video(VideoId),
    ShortLink(Url),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BilibiliOwner {
    pub mid: Option<u64>,
    pub name: Option<String>,
    /// Best-effort follower count. `None` means the relation endpoint was unavailable.
    pub followers: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BilibiliStats {
    pub views: u64,
    pub danmaku: u64,
    pub replies: u64,
    pub likes: u64,
    pub coins: u64,
    pub favorites: u64,
    pub shares: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BilibiliVideo {
    pub id: VideoId,
    pub url: Url,
    pub title: String,
    pub category: Option<String>,
    pub description: Option<String>,
    pub cover_url: Option<Url>,
    pub published_at: Option<DateTime<Utc>>,
    pub owner: BilibiliOwner,
    pub stats: BilibiliStats,
    pub pages: u32,
}

#[async_trait]
pub trait BilibiliService: Send + Sync {
    async fn query(&self, target: &BilibiliTarget) -> Result<BilibiliVideo, IntegrationError>;
}

/// Parses a pure BV/av token or a Bilibili/B23 URL.
///
/// Network resolution of short links never happens here; the caller receives
/// [`BilibiliTarget::ShortLink`] and the service performs the bounded, validated redirect chain.
pub fn parse_bilibili_target(input: &str) -> Result<BilibiliTarget, IntegrationError> {
    let raw = input.trim();
    if raw.is_empty() {
        return Err(IntegrationError::InvalidInput {
            message: "Bilibili target must not be empty".to_owned(),
        });
    }

    if let Some(id) = parse_video_token(raw) {
        return Ok(BilibiliTarget::Video(id));
    }
    let lower = raw.to_ascii_lowercase();
    if !(lower.starts_with("https://") || lower.starts_with("http://")) {
        return Err(IntegrationError::InvalidInput {
            message: "expected a BV/av id or a Bilibili URL".to_owned(),
        });
    }

    let url = Url::parse(raw).map_err(|_| IntegrationError::InvalidInput {
        message: "the Bilibili URL could not be parsed".to_owned(),
    })?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(IntegrationError::InvalidInput {
            message: "the Bilibili URL uses an unsupported scheme".to_owned(),
        });
    }

    let host = url
        .host_str()
        .ok_or_else(|| IntegrationError::InvalidInput {
            message: "the Bilibili URL has no host".to_owned(),
        })?;
    if !BILIBILI_HOSTS
        .iter()
        .any(|allowed| host.eq_ignore_ascii_case(allowed))
    {
        return Err(IntegrationError::InvalidInput {
            message: "the URL host is not a supported Bilibili host".to_owned(),
        });
    }

    if let Some(id) = extract_video_from_url(&url) {
        return Ok(BilibiliTarget::Video(id));
    }

    if SHORT_LINK_HOSTS
        .iter()
        .any(|allowed| host.eq_ignore_ascii_case(allowed))
    {
        return Ok(BilibiliTarget::ShortLink(url));
    }

    Err(IntegrationError::InvalidInput {
        message: "the Bilibili URL does not contain a video id".to_owned(),
    })
}

fn extract_video_from_url(url: &Url) -> Option<VideoId> {
    for segment in url.path_segments().into_iter().flatten() {
        if let Some(id) = parse_video_token(segment) {
            return Some(id);
        }
    }
    for (_, value) in url.query_pairs() {
        if let Some(id) = parse_video_token(&value) {
            return Some(id);
        }
    }
    None
}

/// Extracts the first valid BV/av token from arbitrary text, such as a short-link response body.
pub fn extract_video_id(text: &str) -> Option<VideoId> {
    if let Some(id) = parse_video_token(text) {
        return Some(id);
    }
    for token in text.split(|character: char| !character.is_ascii_alphanumeric()) {
        if let Some(id) = parse_video_token(token) {
            return Some(id);
        }
    }
    None
}

fn parse_video_token(token: &str) -> Option<VideoId> {
    let token = token.trim();
    let lower = token.to_ascii_lowercase();

    if let Some(rest) = lower.strip_prefix("bv")
        && rest.len() == 10
        && rest.bytes().all(|byte| byte.is_ascii_alphanumeric())
    {
        return Some(VideoId::Bv(format!("BV{}", &token[2..])));
    }

    if let Some(rest) = lower.strip_prefix("av")
        && !rest.is_empty()
        && rest.bytes().all(|byte| byte.is_ascii_digit())
    {
        let aid = rest.parse::<u64>().ok()?;
        if aid > 0 {
            return Some(VideoId::Av(aid));
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bv_and_av_tokens() {
        assert_eq!(
            parse_bilibili_target("BV1GJ411x7h7").unwrap(),
            BilibiliTarget::Video(VideoId::Bv("BV1GJ411x7h7".to_owned()))
        );
        assert_eq!(
            parse_bilibili_target("av170001").unwrap(),
            BilibiliTarget::Video(VideoId::Av(170_001))
        );
        assert_eq!(
            parse_bilibili_target("AV170001").unwrap(),
            BilibiliTarget::Video(VideoId::Av(170_001))
        );
    }

    #[test]
    fn extracts_ids_from_urls() {
        let target =
            parse_bilibili_target("https://www.bilibili.com/video/BV1GJ411x7h7?p=2").unwrap();
        assert_eq!(
            target,
            BilibiliTarget::Video(VideoId::Bv("BV1GJ411x7h7".to_owned()))
        );
    }

    #[test]
    fn only_short_link_hosts_become_short_links() {
        let target = parse_bilibili_target("https://b23.tv/abc").unwrap();
        assert!(matches!(target, BilibiliTarget::ShortLink(_)));

        assert!(parse_bilibili_target("https://www.bilibili.com/video/abc").is_err());
    }

    #[test]
    fn rejects_lookalike_hosts_and_bad_ids() {
        assert!(
            parse_bilibili_target("https://bilibili.com.example.org/video/BV1GJ411x7h7").is_err()
        );
        assert!(parse_bilibili_target("BVshort").is_err());
        assert!(parse_bilibili_target("av").is_err());
        assert!(parse_bilibili_target("https://example.org/BV1GJ411x7h7").is_err());
    }
}
