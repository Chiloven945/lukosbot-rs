use serde::Deserialize;
use url::Url;

use lukosbot_integrations::{E621Artist, E621File, E621Post, E621Rating};

fn parse_url(value: Option<String>) -> Option<Url> {
    value.and_then(|value| Url::parse(&value).ok())
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct ArtistDto {
    #[serde(default)]
    id: u64,
    #[serde(default)]
    name: String,
    #[serde(default)]
    other_names: Vec<String>,
    #[serde(default)]
    notes: Option<String>,
    #[serde(default)]
    group_name: Option<String>,
    #[serde(default)]
    urls: Vec<ArtistUrlDto>,
    #[serde(default)]
    is_active: Option<bool>,
    #[serde(default)]
    created_at: Option<String>,
    #[serde(default)]
    updated_at: Option<String>,
    #[serde(default)]
    linked_user_id: Option<u64>,
    #[serde(default)]
    creator_id: Option<u64>,
    #[serde(default)]
    is_locked: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(super) enum ArtistUrlDto {
    String(String),
    Object {
        #[serde(default)]
        url: Option<String>,
        #[serde(default)]
        normalized_url: Option<String>,
    },
}

impl ArtistUrlDto {
    pub(super) fn into_url(self) -> Option<Url> {
        match self {
            Self::String(value) => Url::parse(&value).ok(),
            Self::Object {
                url,
                normalized_url,
            } => parse_url(normalized_url).or_else(|| parse_url(url)),
        }
    }
}

impl ArtistDto {
    pub(super) fn into_model(self) -> E621Artist {
        E621Artist {
            id: self.id,
            name: self.name,
            other_names: self.other_names,
            notes: self.notes,
            group_name: self.group_name,
            urls: self
                .urls
                .into_iter()
                .filter_map(ArtistUrlDto::into_url)
                .collect(),
            is_active: self.is_active,
            created_at: self.created_at,
            updated_at: self.updated_at,
            linked_user_id: self.linked_user_id,
            creator_id: self.creator_id,
            is_locked: self.is_locked,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(super) enum PostsResponse {
    Legacy { posts: Vec<PostDto> },
    V2(Vec<PostDto>),
}

impl PostsResponse {
    pub(super) fn into_posts(self) -> Vec<PostDto> {
        match self {
            Self::Legacy { posts } | Self::V2(posts) => posts,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(super) enum SinglePostResponse {
    Legacy { post: PostDto },
    V2(PostDto),
}

impl SinglePostResponse {
    pub(super) fn into_post(self) -> PostDto {
        match self {
            Self::Legacy { post } | Self::V2(post) => post,
        }
    }
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct PostDto {
    #[serde(default)]
    id: u64,
    #[serde(default)]
    rating: String,
    #[serde(default)]
    file: Option<FileDto>,
    #[serde(default)]
    preview: Option<UrlDto>,
    #[serde(default)]
    sample: Option<UrlDto>,
    #[serde(default)]
    score: Option<ScoreDto>,
    #[serde(default)]
    fav_count: Option<u64>,
    #[serde(default)]
    comment_count: Option<u64>,
    #[serde(default)]
    files: Option<FilesDto>,
    #[serde(default)]
    stats: Option<StatsDto>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    tags: TagsDto,
    #[serde(default)]
    created_at: Option<String>,
    #[serde(default)]
    updated_at: Option<String>,
    #[serde(default)]
    uploader_name: Option<String>,
    #[serde(default)]
    sources: Vec<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct FileDto {
    #[serde(default)]
    width: Option<u32>,
    #[serde(default)]
    height: Option<u32>,
    #[serde(default)]
    ext: Option<String>,
    #[serde(default)]
    size: Option<u64>,
    #[serde(default)]
    md5: Option<String>,
    #[serde(default)]
    url: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct FilesDto {
    #[serde(default)]
    original: Option<FileDto>,
    #[serde(default)]
    preview: Option<ImageDto>,
    #[serde(default)]
    sample: Option<ImageDto>,
    #[serde(default)]
    meta: Option<MetaDto>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct ImageDto {
    #[serde(default)]
    jpg: Option<String>,
    #[serde(default)]
    webp: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct MetaDto {
    #[serde(default)]
    md5: Option<String>,
    #[serde(default)]
    ext: Option<String>,
    #[serde(default)]
    size: Option<u64>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct UrlDto {
    #[serde(default)]
    url: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct ScoreDto {
    #[serde(default)]
    total: Option<i64>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct StatsDto {
    #[serde(default)]
    score: Option<ScoreValue>,
    #[serde(default)]
    fav_count: Option<u64>,
    #[serde(default)]
    comment_count: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(super) enum ScoreValue {
    Integer(i64),
    Object(ScoreDto),
}

impl ScoreValue {
    fn total(self) -> Option<i64> {
        match self {
            Self::Integer(total) => Some(total),
            Self::Object(score) => score.total,
        }
    }
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct TagsDto {
    #[serde(default)]
    artist: Vec<String>,
}

impl PostDto {
    pub(super) fn into_model(self) -> E621Post {
        let (file, preview_url, sample_url, score, favorites, comments) =
            if let Some(files) = self.files {
                let original = files.original.unwrap_or_default();
                let meta = files.meta.unwrap_or_default();
                let file = E621File {
                    width: original.width,
                    height: original.height,
                    extension: meta.ext,
                    size: meta.size,
                    md5: meta.md5,
                    url: parse_url(original.url),
                };
                let preview_url = files
                    .preview
                    .and_then(|preview| preview.jpg.or(preview.webp))
                    .and_then(|value| Url::parse(&value).ok());
                let sample_url = files
                    .sample
                    .and_then(|sample| sample.jpg.or(sample.webp))
                    .and_then(|value| Url::parse(&value).ok());
                let stats = self.stats.unwrap_or_default();
                (
                    file,
                    preview_url,
                    sample_url,
                    stats.score.and_then(ScoreValue::total),
                    stats.fav_count,
                    stats.comment_count,
                )
            } else {
                let file = self.file.unwrap_or_default();
                (
                    E621File {
                        width: file.width,
                        height: file.height,
                        extension: file.ext,
                        size: file.size,
                        md5: file.md5,
                        url: parse_url(file.url),
                    },
                    self.preview.and_then(|preview| parse_url(preview.url)),
                    self.sample.and_then(|sample| parse_url(sample.url)),
                    self.score.and_then(|score| score.total),
                    self.fav_count,
                    self.comment_count,
                )
            };

        E621Post {
            id: self.id,
            rating: E621Rating::parse(&self.rating),
            file,
            preview_url,
            sample_url,
            score,
            favorites,
            comments,
            description: self.description,
            artist_tags: self.tags.artist,
            created_at: self.created_at,
            updated_at: self.updated_at,
            uploader_name: self.uploader_name,
            sources: self.sources,
        }
    }
}
