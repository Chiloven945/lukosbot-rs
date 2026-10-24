use std::num::NonZeroU32;

use url::Url;

use lukosbot_command_engine::{CommandContext, CommandError};
use lukosbot_integrations::{E621Artist, E621Post, E621Rating};

use crate::integrations::format::{strip_controls, truncate_reply};

pub(super) const DEFAULT_RATINGS: &[&str] = &["s", "q", "e"];

pub(super) fn allowed_ratings(context: &CommandContext<'_>) -> Vec<E621Rating> {
    let defaults = DEFAULT_RATINGS
        .iter()
        .map(|rating| (*rating).to_owned())
        .collect::<Vec<_>>();
    context
        .allowed_values("e621.rating", Some(&defaults))
        .into_iter()
        .map(|rating| E621Rating::parse(&rating))
        .filter(|rating| !matches!(rating, E621Rating::Unknown))
        .collect()
}

pub(super) fn split_page(raw: &str) -> (String, u32) {
    let raw = raw.trim();
    let Some((head, tail)) = raw.rsplit_once(char::is_whitespace) else {
        return (raw.to_owned(), 1);
    };
    let Ok(page) = tail.parse::<u32>() else {
        return (raw.to_owned(), 1);
    };
    if page == 0 || head.trim().is_empty() {
        (raw.to_owned(), 1)
    } else {
        (head.trim().to_owned(), page)
    }
}

pub(super) fn parse_post_id(input: &str) -> Result<NonZeroU32, CommandError> {
    let value = extract_id_or_name(input, "posts")?;
    value
        .parse::<u32>()
        .ok()
        .and_then(NonZeroU32::new)
        .ok_or_else(|| CommandError::UserInput("帖子 ID 必须是正整数。".to_owned()))
}

pub(super) fn extract_id_or_name(input: &str, kind: &str) -> Result<String, CommandError> {
    let input = input.trim();
    if !input.starts_with("http://") && !input.starts_with("https://") {
        return Ok(input.to_owned());
    }
    let url = Url::parse(input)
        .map_err(|_| CommandError::UserInput("无法从该链接中识别 ID。".to_owned()))?;
    if !matches!(url.host_str(), Some("e621.net" | "www.e621.net")) {
        return Err(CommandError::UserInput(
            "仅支持 e621.net 链接或 ID。".to_owned(),
        ));
    }
    let id = url
        .path_segments()
        .and_then(|mut segments| {
            let first = segments.next()?;
            if first != kind {
                return None;
            }
            segments.next()?.parse::<u64>().ok()
        })
        .ok_or_else(|| CommandError::UserInput("无法从该链接中识别 ID。".to_owned()))?;
    Ok(id.to_string())
}

pub(super) fn format_artist(artist: &E621Artist) -> String {
    let mut text = format!("作者：{}（#{}）\n", artist.name, artist.id);
    if let Some(active) = artist.is_active {
        text.push_str(&format!("状态：{}\n", if active { "活跃" } else { "停用" }));
    }
    if let Some(group) = &artist.group_name
        && !group.is_empty()
    {
        text.push_str(&format!("团体：{group}\n"));
    }
    if !artist.other_names.is_empty() {
        text.push_str(&format!("其他名称：{}\n", artist.other_names.join("、")));
    }
    for url in &artist.urls {
        text.push_str(&format!("链接：{url}\n"));
    }
    if let Some(notes) = &artist.notes {
        text.push_str(&format!("说明：{}\n", truncate(strip_controls(notes), 240)));
    }
    truncate_reply(text.trim_end())
}

pub(super) fn format_post(post: &E621Post) -> String {
    let mut text = format!("帖子 {}\n", post.id);
    if let Some(created) = &post.created_at {
        text.push_str(&format!("发布于：{created}\n"));
    }
    if let Some(updated) = &post.updated_at {
        text.push_str(&format!("更新于：{updated}\n"));
    }
    text.push_str(&format!(
        "分级：{}\n",
        post.rating.as_str().to_ascii_uppercase()
    ));
    if let Some(description) = &post.description {
        text.push_str(&format!(
            "\n{}\n",
            truncate(strip_controls(description), 400)
        ));
    }
    if let Some(md5) = &post.file.md5 {
        let ext = post.file.extension.as_deref().unwrap_or("bin");
        text.push_str(&format!("文件：{md5}.{ext}\n"));
    }
    if let (Some(width), Some(height)) = (post.file.width, post.file.height) {
        text.push_str(&format!("分辨率：{width} × {height}\n"));
    }
    if let Some(size) = post.file.size {
        text.push_str(&format!("大小：{size} B\n"));
    }
    if let Some(url) = &post.file.url {
        text.push_str(&format!("链接：{url}\n"));
    }
    if let Some(uploader) = &post.uploader_name
        && !uploader.is_empty()
    {
        text.push_str(&format!("上传者：{uploader}\n"));
    }
    if !post.sources.is_empty() {
        text.push_str(&format!(
            "来源：{}\n",
            truncate(strip_controls(&post.sources.join("、")), 200)
        ));
    }
    if !post.artist_tags.is_empty() {
        text.push_str(&format!("作者：{}\n", post.artist_tags.join("、")));
    }
    text.push_str(&format!(
        "评分：{} | ❤️{} | 💬{}\n",
        post.score.unwrap_or(0),
        post.favorites.unwrap_or(0),
        post.comments.unwrap_or(0)
    ));
    truncate_reply(text.trim_end())
}

pub(super) fn truncate(value: String, limit: usize) -> String {
    if value.chars().count() <= limit {
        value
    } else {
        let mut value = value.chars().take(limit).collect::<String>();
        value.push('…');
        value
    }
}
