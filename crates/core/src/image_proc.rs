// SPDX-License-Identifier: GPL-3.0-only
// Adapts MPC Autofill's desktop-tool/src/processing.py and constants.py.
// Upstream authors: chilli-axe and contributors. Modified for Rust from 2026-09-20.
// See NOTICE.md and Git history for attribution and dated changes.
//! Optional image post-processing: downscale a high-DPI card image before upload
//! to cut upload time. The UI preview is handled separately (remote Drive
//! thumbnails), so this module only produces the upload image.

use std::io::Cursor;

use image::imageops::FilterType;
use image::ImageFormat;

use crate::Result;

/// Re-encode `bytes` (a Drive thumbnail) into a small JPEG bounded by `max_edge`,
/// so the UI grid serves tiny, uniform files.
pub fn to_thumbnail(bytes: &[u8], max_edge: u32) -> Result<Vec<u8>> {
    let image = image::load_from_memory(bytes)?;
    let thumb = image.thumbnail(max_edge, max_edge * 2);
    let mut out = Cursor::new(Vec::new());
    thumb.write_to(&mut out, ImageFormat::Jpeg)?;
    Ok(out.into_inner())
}

/// MPC renders cards at 300 DPI for a 1110px image height; this ratio recovers an
/// image's effective DPI from its pixel height.
const DPI_HEIGHT_RATIO: f64 = 300.0 / 1110.0;

#[derive(Debug, Clone, Copy)]
pub struct PostProcess {
    pub max_dpi: u32,
    pub filter: FilterType,
}

impl Default for PostProcess {
    fn default() -> Self {
        // CatmullRom is close to Lanczos in quality for downscaling but faster.
        PostProcess { max_dpi: 800, filter: FilterType::CatmullRom }
    }
}

/// Downscale `bytes` so its effective DPI is at most `cfg.max_dpi`. Images already
/// at or below the target are returned untouched, and their pixels are never even
/// decoded — the DPI is read from the header dimensions first.
pub fn downscale(bytes: Vec<u8>, cfg: &PostProcess) -> Result<Vec<u8>> {
    let format = image::guess_format(&bytes)?;

    let (_, height) =
        image::ImageReader::with_format(Cursor::new(bytes.as_slice()), format).into_dimensions()?;
    let dpi = 10.0 * (f64::from(height) * DPI_HEIGHT_RATIO / 10.0).round();
    if dpi <= f64::from(cfg.max_dpi) {
        return Ok(bytes);
    }

    let image = image::load_from_memory_with_format(&bytes, format)?;
    let scale = f64::from(cfg.max_dpi) / dpi;
    let width = (f64::from(image.width()) * scale).round() as u32;
    let height = (f64::from(image.height()) * scale).round() as u32;
    let resized = image.resize_exact(width, height, cfg.filter);

    let mut out = Cursor::new(Vec::new());
    resized.write_to(&mut out, encode_format(format))?;
    Ok(out.into_inner())
}

/// Formats we can't re-encode fall back to PNG (lossless), so downscaling never
/// silently degrades an image via an unsupported encoder.
fn encode_format(source: ImageFormat) -> ImageFormat {
    match source {
        ImageFormat::Jpeg => ImageFormat::Jpeg,
        _ => ImageFormat::Png,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, RgbImage};

    fn png_of_height(height: u32) -> Vec<u8> {
        let image = DynamicImage::ImageRgb8(RgbImage::new(height * 3 / 4, height));
        let mut out = Cursor::new(Vec::new());
        image.write_to(&mut out, ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn leaves_low_dpi_images_byte_for_byte() {
        let bytes = png_of_height(800);
        let out = downscale(bytes.clone(), &PostProcess::default()).unwrap();
        assert_eq!(out, bytes);
    }

    #[test]
    fn downscales_high_dpi_images() {
        let bytes = png_of_height(4400);
        let cfg = PostProcess { max_dpi: 400, filter: FilterType::CatmullRom };
        let out = downscale(bytes, &cfg).unwrap();
        let shrunk = image::load_from_memory(&out).unwrap();
        assert!(shrunk.height() < 4400);
    }
}
