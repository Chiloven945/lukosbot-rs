use std::io::Cursor;

use bytes::Bytes;
use cosmic_text::{FontSystem, SwashCache};
use image::{ImageFormat, Rgba, RgbaImage, imageops};

use lukosbot_integrations::E621Post;

use crate::rendering::{RenderError, Theme, background, draw_text, foreground, muted};

const CARD_W: u32 = 236;
const CARD_H: u32 = 304;
const THUMB: u32 = 208;
const GAP: u32 = 18;
const PAD: u32 = 28;
const HEADER: u32 = 74;

pub fn render(
    query: &str,
    page: u32,
    posts: &[E621Post],
    thumbnails: &[Option<RgbaImage>],
    theme: Theme,
) -> Result<Bytes, RenderError> {
    let count = posts.len().max(1);
    let columns = (count as f32).sqrt().ceil() as u32;
    let columns = columns.clamp(1, 5);
    let rows = (posts.len().max(1) as u32).div_ceil(columns);
    let width = PAD * 2 + columns * CARD_W + columns.saturating_sub(1) * GAP;
    let height = PAD * 2 + HEADER + rows * CARD_H + rows.saturating_sub(1) * GAP;
    if width.saturating_mul(height) > 20_000_000 {
        return Err(RenderError::TooLarge);
    }

    let mut canvas = RgbaImage::from_pixel(width, height, background(theme));
    let mut font_system = FontSystem::new();
    let mut cache = SwashCache::new();
    draw_text(
        &mut canvas,
        &mut font_system,
        &mut cache,
        "E621 搜索结果",
        PAD as i32,
        PAD as i32,
        28.0,
        foreground(theme),
    );
    draw_text(
        &mut canvas,
        &mut font_system,
        &mut cache,
        &format!(
            "“{}” · 第 {page} 页 · {} 个结果",
            if query.is_empty() { "全部" } else { query },
            posts.len()
        ),
        PAD as i32,
        (PAD + 38) as i32,
        18.0,
        muted(theme),
    );

    for (index, post) in posts.iter().enumerate() {
        let row = index as u32 / columns;
        let column = index as u32 % columns;
        let x = PAD + column * (CARD_W + GAP);
        let y = PAD + HEADER + row * (CARD_H + GAP);
        imageops::overlay(
            &mut canvas,
            &RgbaImage::from_pixel(CARD_W, CARD_H, card(theme)),
            i64::from(x),
            i64::from(y),
        );

        let thumb_x = x + (CARD_W - THUMB) / 2;
        let thumb_y = y + 14;
        match thumbnails.get(index).and_then(Option::as_ref) {
            Some(thumbnail) => {
                let resized =
                    imageops::resize(thumbnail, THUMB, THUMB, imageops::FilterType::Triangle);
                imageops::overlay(
                    &mut canvas,
                    &resized,
                    i64::from(thumb_x),
                    i64::from(thumb_y),
                );
            }
            None => {
                imageops::overlay(
                    &mut canvas,
                    &RgbaImage::from_pixel(THUMB, THUMB, placeholder(theme)),
                    i64::from(thumb_x),
                    i64::from(thumb_y),
                );
                draw_text(
                    &mut canvas,
                    &mut font_system,
                    &mut cache,
                    "无预览图",
                    (thumb_x + 52) as i32,
                    (thumb_y + THUMB / 2) as i32,
                    18.0,
                    muted(theme),
                );
            }
        }

        let line_y = y + THUMB + 26;
        draw_text(
            &mut canvas,
            &mut font_system,
            &mut cache,
            &format!(
                "#{}  {}",
                post.id,
                post.rating.as_str().to_ascii_uppercase()
            ),
            (x + 14) as i32,
            line_y as i32,
            18.0,
            foreground(theme),
        );
        let artist = if post.artist_tags.is_empty() {
            "未知作者".to_owned()
        } else {
            post.artist_tags.join("、")
        };
        draw_text(
            &mut canvas,
            &mut font_system,
            &mut cache,
            &ellipsize(&artist, 22),
            (x + 14) as i32,
            (line_y + 26) as i32,
            15.0,
            muted(theme),
        );
        let dimensions = match (post.file.width, post.file.height) {
            (Some(width), Some(height)) => format!("{width}×{height}"),
            _ => "尺寸未知".to_owned(),
        };
        draw_text(
            &mut canvas,
            &mut font_system,
            &mut cache,
            &format!(
                "★ {}  ❤ {}",
                post.score.unwrap_or(0),
                post.favorites.unwrap_or(0)
            ),
            (x + 14) as i32,
            (line_y + 50) as i32,
            15.0,
            muted(theme),
        );
        draw_text(
            &mut canvas,
            &mut font_system,
            &mut cache,
            &dimensions,
            (x + 14) as i32,
            (line_y + 72) as i32,
            15.0,
            muted(theme),
        );
    }

    let mut output = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(canvas)
        .write_to(&mut output, ImageFormat::Png)
        .map_err(RenderError::Encode)?;
    Ok(Bytes::from(output.into_inner()))
}

fn card(theme: Theme) -> Rgba<u8> {
    match theme {
        Theme::Light => Rgba([255, 255, 255, 255]),
        Theme::Dark => Rgba([36, 39, 46, 255]),
    }
}

fn placeholder(theme: Theme) -> Rgba<u8> {
    match theme {
        Theme::Light => Rgba([224, 228, 234, 255]),
        Theme::Dark => Rgba([52, 56, 65, 255]),
    }
}

fn ellipsize(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        value.to_owned()
    } else {
        let mut value = value
            .chars()
            .take(max.saturating_sub(1))
            .collect::<String>();
        value.push('…');
        value
    }
}
