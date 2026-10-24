use bytes::Bytes;
use url::Url;

use crate::platform::PlatformKind;

/// A reference to media that is not necessarily downloaded yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaRef {
    pub source: MediaSource,
    pub file_name: Option<String>,
    pub mime_type: Option<String>,
    pub size_bytes: Option<u64>,
}

/// Media that has been downloaded, generated, or otherwise materialized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaContent {
    pub bytes: Bytes,
    pub file_name: Option<String>,
    pub mime_type: Option<String>,
}

/// Where media can be obtained from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaSource {
    /// A directly reachable URL.
    Url(Url),
    /// An opaque platform file identifier that must be resolved by the adapter.
    PlatformFile {
        platform: PlatformKind,
        file_id: String,
    },
    /// Content that is already in memory.
    Bytes(Bytes),
}
