//! Typed contracts for the bot's external API integrations.
//!
//! This crate deliberately contains no HTTP, TCP, platform SDK, database, or configuration code.
//! The command layer depends on the normalised models and service traits; `lukosbot-infra`
//! implements the traits; `lukosbot-app` chooses the implementation.

#![forbid(unsafe_code)]

pub mod bilibili;
pub mod error;
pub mod github;
pub mod ip;
pub mod music;
pub mod whois;

pub use bilibili::{
    BILIBILI_HOSTS, BilibiliOwner, BilibiliService, BilibiliStats, BilibiliTarget, BilibiliVideo,
    VideoId, extract_video_id, parse_bilibili_target,
};
pub use error::{BoxError, IntegrationError};
pub use github::{
    GitHubOrder, GitHubRepository, GitHubRepositoryBrief, GitHubSearchParams, GitHubSearchResult,
    GitHubService, GitHubSort, GitHubUser,
};
pub use ip::{
    IpData, IpLookupService, IpProviderId, IpQueryResult, IpRisk, resolve_provider_selection,
};
pub use music::{
    MusicLink, MusicLinkError, MusicPlatform, MusicService, TrackInfo, parse_music_link,
};
pub use whois::{WhoisRecord, WhoisService};
