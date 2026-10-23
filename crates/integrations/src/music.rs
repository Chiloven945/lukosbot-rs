use std::time::Duration;

use async_trait::async_trait;
use url::Url;

use crate::error::IntegrationError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MusicPlatform {
    Spotify,
    SoundCloud,
}

impl MusicPlatform {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Spotify => "spotify",
            Self::SoundCloud => "soundcloud",
        }
    }

    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Spotify => "Spotify",
            Self::SoundCloud => "SoundCloud",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackInfo {
    pub platform: MusicPlatform,
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub cover_url: Option<Url>,
    pub track_url: Option<Url>,
    pub duration: Option<Duration>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MusicLink {
    pub platform: MusicPlatform,
    pub url: Url,
}

/// Link parsing failures are kept separate because the command has distinct user messages for
/// "unknown platform" and "known platform but no track path".
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MusicLinkError {
    #[error("unsupported music platform")]
    UnsupportedPlatform,

    #[error("invalid music URL")]
    InvalidUrl,

    #[error("the URL does not contain a track")]
    InvalidTrackPath,
}

#[async_trait]
pub trait MusicService: Send + Sync {
    fn available_platforms(&self) -> Vec<MusicPlatform>;

    async fn search(
        &self,
        platform: MusicPlatform,
        query: &str,
    ) -> Result<TrackInfo, IntegrationError>;

    async fn resolve_link(&self, link: &MusicLink) -> Result<TrackInfo, IntegrationError>;
}

/// Parses a Spotify or SoundCloud URL without trusting string containment.
pub fn parse_music_link(input: &str) -> Result<MusicLink, MusicLinkError> {
    let url = Url::parse(input.trim()).map_err(|_| MusicLinkError::InvalidUrl)?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return Err(MusicLinkError::InvalidUrl);
    }

    let host = url.host_str().ok_or(MusicLinkError::InvalidUrl)?;
    if host.eq_ignore_ascii_case("open.spotify.com") {
        let mut segments = url.path_segments().into_iter().flatten();
        let first = segments.next();
        let id = segments.next();
        if first != Some("track") || segments.next().is_some() {
            return Err(MusicLinkError::InvalidTrackPath);
        }
        let id = id.ok_or(MusicLinkError::InvalidTrackPath)?;
        if id.is_empty() || !id.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
            return Err(MusicLinkError::InvalidTrackPath);
        }
        return Ok(MusicLink {
            platform: MusicPlatform::Spotify,
            url,
        });
    }

    const SOUNDCLOUD_HOSTS: &[&str] = &[
        "soundcloud.com",
        "www.soundcloud.com",
        "m.soundcloud.com",
        "on.soundcloud.com",
    ];
    if SOUNDCLOUD_HOSTS
        .iter()
        .any(|allowed| host.eq_ignore_ascii_case(allowed))
    {
        if url.path().trim_matches('/').is_empty() {
            return Err(MusicLinkError::InvalidTrackPath);
        }
        return Ok(MusicLink {
            platform: MusicPlatform::SoundCloud,
            url,
        });
    }

    Err(MusicLinkError::UnsupportedPlatform)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_spotify_track_links_only() {
        let link =
            parse_music_link("https://open.spotify.com/track/4uLU6hMCjMI75M1A2tKUQC").unwrap();
        assert_eq!(link.platform, MusicPlatform::Spotify);

        assert_eq!(
            parse_music_link("https://open.spotify.com/album/abc").unwrap_err(),
            MusicLinkError::InvalidTrackPath
        );
        assert_eq!(
            parse_music_link("https://open.spotify.com.evil/track/abc").unwrap_err(),
            MusicLinkError::UnsupportedPlatform
        );
    }

    #[test]
    fn rejects_http_and_credentials() {
        assert_eq!(
            parse_music_link("http://open.spotify.com/track/abc").unwrap_err(),
            MusicLinkError::InvalidUrl
        );
        assert_eq!(
            parse_music_link("https://user@open.spotify.com/track/abc").unwrap_err(),
            MusicLinkError::InvalidUrl
        );
    }

    #[test]
    fn accepts_soundcloud_links() {
        let link = parse_music_link("https://soundcloud.com/artist/track").unwrap();
        assert_eq!(link.platform, MusicPlatform::SoundCloud);
        assert_eq!(
            parse_music_link("https://soundcloud.com/").unwrap_err(),
            MusicLinkError::InvalidTrackPath
        );
    }
}
