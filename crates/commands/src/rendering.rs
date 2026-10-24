use std::io::Cursor;

use bytes::Bytes;
use cosmic_text::{Attrs, Buffer, FontSystem, Metrics, Shaping, SwashCache};
use image::{ImageFormat, Rgba, RgbaImage};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    Light,
    Dark,
}

#[derive(Debug, Clone)]
pub struct RenderLine {
    pub text: String,
    pub size: f32,
    pub muted: bool,
    pub gap_before: u32,
}

impl RenderLine {
    pub fn title(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            size: 30.0,
            muted: false,
            gap_before: 0,
        }
    }

    pub fn body(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            size: 20.0,
            muted: false,
            gap_before: 8,
        }
    }

    pub fn muted(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            size: 18.0,
            muted: true,
            gap_before: 6,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("failed to encode image")]
    Encode(#[from] image::ImageError),
    #[error("rendered image would exceed the size limit")]
    TooLarge,
}

pub fn render_lines(
    lines: &[RenderLine],
    theme: Theme,
    width: u32,
    max_height: u32,
) -> Result<Bytes, RenderError> {
    let width = width.max(320);
    let line_heights = lines
        .iter()
        .map(|line| line.size.round() as u32 + 8 + line.gap_before)
        .sum::<u32>();
    let height = (line_heights + 40).min(max_height.max(200));
    if height >= max_height {
        return Err(RenderError::TooLarge);
    }

    let mut canvas = RgbaImage::from_pixel(width, height, background(theme));
    let mut font_system = FontSystem::new();
    let mut cache = SwashCache::new();
    let mut y = 20_i32;

    for line in lines {
        y += line.gap_before as i32;
        draw_text(
            &mut canvas,
            &mut font_system,
            &mut cache,
            &line.text,
            24,
            y,
            line.size,
            if line.muted {
                muted(theme)
            } else {
                foreground(theme)
            },
        );
        y += line.size.round() as i32 + 8;
    }

    let mut output = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(canvas)
        .write_to(&mut output, ImageFormat::Png)
        .map_err(RenderError::Encode)?;
    Ok(Bytes::from(output.into_inner()))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_text(
    canvas: &mut RgbaImage,
    font_system: &mut FontSystem,
    cache: &mut SwashCache,
    text: &str,
    x: i32,
    y: i32,
    size: f32,
    color: Rgba<u8>,
) {
    let mut buffer = Buffer::new(font_system, Metrics::new(size, size * 1.35));
    buffer.set_size(Some(canvas.width() as f32 - x as f32 * 2.0), None);
    buffer.set_text(text, &Attrs::new(), Shaping::Advanced, None);
    buffer.shape_until_scroll(font_system, false);

    for run in buffer.layout_runs() {
        for glyph in run.glyphs {
            let physical = glyph.physical((0.0, 0.0), 1.0);
            if let Some(image) = cache.get_image(font_system, physical.cache_key) {
                let gx = x + physical.x + image.placement.left;
                let gy = y + run.line_y as i32 + physical.y - image.placement.top;
                for image_y in 0..image.placement.height as i32 {
                    for image_x in 0..image.placement.width as i32 {
                        let alpha =
                            image.data[(image_y * image.placement.width as i32 + image_x) as usize];
                        if alpha == 0 {
                            continue;
                        }
                        let px = gx + image_x;
                        let py = gy + image_y;
                        if px < 0
                            || py < 0
                            || px >= canvas.width() as i32
                            || py >= canvas.height() as i32
                        {
                            continue;
                        }
                        let pixel = canvas.get_pixel_mut(px as u32, py as u32);
                        let alpha = alpha as u16;
                        for channel in 0..3 {
                            pixel[channel] = ((u16::from(color[channel]) * alpha
                                + u16::from(pixel[channel]) * (255 - alpha))
                                / 255) as u8;
                        }
                        pixel[3] = 255;
                    }
                }
            }
        }
    }
}

pub(crate) fn background(theme: Theme) -> Rgba<u8> {
    match theme {
        Theme::Light => Rgba([248, 249, 251, 255]),
        Theme::Dark => Rgba([24, 26, 31, 255]),
    }
}

pub(crate) fn foreground(theme: Theme) -> Rgba<u8> {
    match theme {
        Theme::Light => Rgba([30, 34, 42, 255]),
        Theme::Dark => Rgba([235, 238, 245, 255]),
    }
}

pub(crate) fn muted(theme: Theme) -> Rgba<u8> {
    match theme {
        Theme::Light => Rgba([90, 98, 112, 255]),
        Theme::Dark => Rgba([166, 174, 188, 255]),
    }
}
