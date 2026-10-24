use std::num::NonZeroU32;

use async_trait::async_trait;
use url::Url;

use crate::IntegrationError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum E621Rating {
    Safe,
    Questionable,
    Explicit,
    Unknown,
}

impl E621Rating {
    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "s" | "safe" => Self::Safe,
            "q" | "questionable" => Self::Questionable,
            "e" | "explicit" => Self::Explicit,
            _ => Self::Unknown,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Safe => "s",
            Self::Questionable => "q",
            Self::Explicit => "e",
            Self::Unknown => "?",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct E621Md5(String);

impl E621Md5 {
    pub fn parse(value: &str) -> Result<Self, IntegrationError> {
        let value = value.trim().to_ascii_lowercase();
        if value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            Ok(Self(value))
        } else {
            Err(IntegrationError::InvalidInput {
                message: "MD5 must contain exactly 32 hexadecimal characters".to_owned(),
            })
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct E621Artist {
    pub id: u64,
    pub name: String,
    pub other_names: Vec<String>,
    pub notes: Option<String>,
    pub group_name: Option<String>,
    pub urls: Vec<Url>,
    pub is_active: Option<bool>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub linked_user_id: Option<u64>,
    pub creator_id: Option<u64>,
    pub is_locked: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct E621File {
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub extension: Option<String>,
    pub size: Option<u64>,
    pub md5: Option<String>,
    pub url: Option<Url>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct E621Post {
    pub id: u64,
    pub rating: E621Rating,
    pub file: E621File,
    pub preview_url: Option<Url>,
    pub sample_url: Option<Url>,
    pub score: Option<i64>,
    pub favorites: Option<u64>,
    pub comments: Option<u64>,
    pub description: Option<String>,
    pub artist_tags: Vec<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub uploader_name: Option<String>,
    pub sources: Vec<String>,
}

impl E621Post {
    pub fn thumbnail_url(&self) -> Option<&Url> {
        self.preview_url
            .as_ref()
            .or(self.sample_url.as_ref())
            .or(self.file.url.as_ref())
    }

    pub fn is_allowed(&self, allowed: &[E621Rating]) -> bool {
        !matches!(self.rating, E621Rating::Unknown) && allowed.contains(&self.rating)
    }
}

#[async_trait]
pub trait E621Service: Send + Sync {
    async fn get_artist(&self, id_or_name: &str) -> Result<Option<E621Artist>, IntegrationError>;

    async fn search_artists(
        &self,
        query: &str,
        page: NonZeroU32,
        limit: NonZeroU32,
    ) -> Result<Vec<E621Artist>, IntegrationError>;

    async fn get_post(&self, id: NonZeroU32) -> Result<Option<E621Post>, IntegrationError>;

    async fn search_posts(
        &self,
        tags: &str,
        page: NonZeroU32,
        limit: NonZeroU32,
    ) -> Result<Vec<E621Post>, IntegrationError>;

    async fn find_post_by_md5(&self, md5: &E621Md5) -> Result<Option<E621Post>, IntegrationError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ratings_and_md5() {
        assert_eq!(E621Rating::parse("S"), E621Rating::Safe);
        assert_eq!(E621Rating::parse("questionable"), E621Rating::Questionable);
        assert_eq!(E621Rating::parse("weird"), E621Rating::Unknown);
        assert!(E621Md5::parse("0123456789abcdef0123456789ABCDEF").is_ok());
        assert!(E621Md5::parse("abc").is_err());
    }
}
