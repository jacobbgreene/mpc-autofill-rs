//! Bounded validation and optional post-processing of untrusted artwork.

use crate::{Error, Result};
use image::imageops::FilterType;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader};
use std::io::{Cursor, Seek, SeekFrom, Write};
use std::sync::Mutex;

pub const MAX_IMAGE_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_THUMBNAIL_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_IMAGE_PIXELS: u64 = 64_000_000;
pub const MAX_DECODE_ALLOCATION: u64 = 256 * 1024 * 1024;
const MAX_DIMENSION: u32 = 16_384;

// Network concurrency must never multiply simultaneous image allocations.
static PROCESSING: Mutex<()> = Mutex::new(());

fn decoded(bytes: &[u8]) -> Result<(DynamicImage, ImageFormat)> {
    if bytes.is_empty() || bytes.len() > MAX_IMAGE_BYTES {
        return Err(Error::Invalid(
            "artwork must be an image no larger than 64 MiB".into(),
        ));
    }
    let format = image::guess_format(bytes)?;
    if !matches!(
        format,
        ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP
    ) {
        return Err(Error::Invalid(
            "artwork must be a PNG, JPEG, or WebP image".into(),
        ));
    }
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    limits.max_alloc = Some(MAX_DECODE_ALLOCATION);
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    reader.limits(limits);
    let decoder = reader.into_decoder()?;
    let (width, height) = decoder.dimensions();
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > MAX_IMAGE_PIXELS {
        return Err(Error::Invalid(
            "artwork exceeds the 64 megapixel image limit".into(),
        ));
    }
    if decoder.total_bytes() > MAX_DECODE_ALLOCATION {
        return Err(Error::Invalid(
            "artwork exceeds the 256 MiB decoded image limit".into(),
        ));
    }
    // Headers alone cannot prove that a truncated image is usable artwork.
    Ok((DynamicImage::from_decoder(decoder)?, format))
}

pub fn validate(bytes: &[u8]) -> Result<&'static str> {
    let _guard = PROCESSING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let (_, format) = decoded(bytes)?;
    Ok(extension(format))
}

pub fn extension(format: ImageFormat) -> &'static str {
    match format {
        ImageFormat::Jpeg => "jpg",
        ImageFormat::WebP => "webp",
        _ => "png",
    }
}

pub fn to_thumbnail(bytes: &[u8], max_edge: u32) -> Result<Vec<u8>> {
    thumbnail(bytes, max_edge, MAX_THUMBNAIL_BYTES)
}

/// Used only after a local image has been explicitly approved and snapshotted.
pub fn thumbnail_from_original(bytes: &[u8], max_edge: u32) -> Result<Vec<u8>> {
    thumbnail(bytes, max_edge, MAX_IMAGE_BYTES)
}

fn thumbnail(bytes: &[u8], max_edge: u32, byte_limit: usize) -> Result<Vec<u8>> {
    let _guard = PROCESSING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if bytes.len() > byte_limit || !(1..=2048).contains(&max_edge) {
        return Err(Error::Invalid("preview exceeds its size limit".into()));
    }
    let (image, _) = decoded(bytes)?;
    let thumb = image.thumbnail(max_edge, max_edge * 2).to_rgb8();
    encode(
        &DynamicImage::ImageRgb8(thumb),
        ImageFormat::Jpeg,
        MAX_THUMBNAIL_BYTES,
    )
}

pub fn validate_thumbnail(bytes: &[u8], max_edge: u32) -> Result<()> {
    let _guard = PROCESSING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if bytes.len() > MAX_THUMBNAIL_BYTES || !(1..=2048).contains(&max_edge) {
        return Err(Error::Invalid("preview exceeds its size limit".into()));
    }
    let (image, format) = decoded(bytes)?;
    if format != ImageFormat::Jpeg || image.width() > max_edge || image.height() > max_edge * 2 {
        return Err(Error::Invalid(
            "cached preview has unexpected dimensions or format".into(),
        ));
    }
    Ok(())
}

const DPI_HEIGHT_RATIO: f64 = 300.0 / 1110.0;

#[derive(Debug, Clone, Copy)]
pub struct PostProcess {
    pub max_dpi: u32,
    pub filter: FilterType,
}

impl Default for PostProcess {
    fn default() -> Self {
        Self {
            max_dpi: 800,
            filter: FilterType::CatmullRom,
        }
    }
}

/// Validation always runs, including with scaling disabled or a low-DPI image.
pub fn process(bytes: Vec<u8>, cfg: Option<PostProcess>) -> Result<Vec<u8>> {
    let _guard = PROCESSING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let (image, format) = decoded(&bytes)?;
    let Some(cfg) = cfg else { return Ok(bytes) };
    if !(1..=2400).contains(&cfg.max_dpi) {
        return Err(Error::Invalid(
            "maximum image DPI must be between 1 and 2400".into(),
        ));
    }
    let dpi = 10.0 * (f64::from(image.height()) * DPI_HEIGHT_RATIO / 10.0).round();
    if dpi <= f64::from(cfg.max_dpi) {
        return Ok(bytes);
    }
    let scale = f64::from(cfg.max_dpi) / dpi;
    let width = ((f64::from(image.width()) * scale).round() as u32).max(1);
    let height = ((f64::from(image.height()) * scale).round() as u32).max(1);
    let resized = image.resize_exact(width, height, cfg.filter);
    let output = if format == ImageFormat::Jpeg {
        ImageFormat::Jpeg
    } else {
        ImageFormat::Png
    };
    encode(&resized, output, MAX_IMAGE_BYTES)
}

pub fn downscale(bytes: Vec<u8>, cfg: &PostProcess) -> Result<Vec<u8>> {
    process(bytes, Some(*cfg))
}

fn encode(image: &DynamicImage, format: ImageFormat, limit: usize) -> Result<Vec<u8>> {
    let mut output = LimitedOutput {
        cursor: Cursor::new(Vec::new()),
        limit,
    };
    image.write_to(&mut output, format)?;
    Ok(output.cursor.into_inner())
}

struct LimitedOutput {
    cursor: Cursor<Vec<u8>>,
    limit: usize,
}

impl Write for LimitedOutput {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.cursor.position().saturating_add(bytes.len() as u64) > self.limit as u64 {
            return Err(std::io::Error::other(
                "processed artwork exceeds its byte limit",
            ));
        }
        self.cursor.write(bytes)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Seek for LimitedOutput {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        let previous = self.cursor.position();
        let next = self.cursor.seek(position)?;
        if next > self.limit as u64 {
            self.cursor.set_position(previous);
            return Err(std::io::Error::other(
                "processed artwork exceeds its byte limit",
            ));
        }
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::RgbImage;

    fn png_of_height(height: u32) -> Vec<u8> {
        let image = DynamicImage::ImageRgb8(RgbImage::new(height * 3 / 4, height));
        let mut out = Cursor::new(Vec::new());
        image.write_to(&mut out, ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn leaves_low_dpi_images_byte_for_byte() {
        let bytes = png_of_height(80);
        assert_eq!(
            downscale(bytes.clone(), &PostProcess::default()).unwrap(),
            bytes
        );
    }

    #[test]
    fn downscales_high_dpi_images() {
        let cfg = PostProcess {
            max_dpi: 100,
            filter: FilterType::CatmullRom,
        };
        let out = downscale(png_of_height(800), &cfg).unwrap();
        assert!(image::load_from_memory(&out).unwrap().height() < 800);
    }

    #[test]
    fn rejects_truncated_low_dpi_image_even_with_processing_disabled() {
        let mut bytes = png_of_height(80);
        bytes.truncate(bytes.len() / 2);
        assert!(process(bytes, None).is_err());
    }

    #[test]
    fn rejects_oversized_dimension_header_without_decoding_pixels() {
        let mut bytes = png_of_height(4);
        bytes[16..20].copy_from_slice(&20_000u32.to_be_bytes());
        repair_header_crc(&mut bytes);
        assert!(validate(&bytes).is_err());
    }

    fn repair_header_crc(bytes: &mut [u8]) {
        let mut crc = 0xffff_ffffu32;
        for byte in &bytes[12..29] {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                crc = (crc >> 1) ^ (0xedb8_8320 & 0u32.wrapping_sub(crc & 1));
            }
        }
        bytes[29..33].copy_from_slice(&(!crc).to_be_bytes());
    }

    #[test]
    fn pixel_product_is_checked_before_decoding() {
        let mut bytes = png_of_height(4);
        bytes[16..20].copy_from_slice(&10_000u32.to_be_bytes());
        bytes[20..24].copy_from_slice(&7_000u32.to_be_bytes());
        repair_header_crc(&mut bytes);
        assert!(validate(&bytes)
            .unwrap_err()
            .to_string()
            .contains("64 megapixel"));
    }

    #[test]
    fn encoder_cannot_grow_beyond_its_limit() {
        let mut output = LimitedOutput {
            cursor: Cursor::new(Vec::new()),
            limit: 4,
        };
        output.write_all(b"1234").unwrap();
        assert!(output.write_all(b"5").is_err());
        assert!(output.seek(SeekFrom::Start(100)).is_err());
        assert_eq!(output.cursor.into_inner(), b"1234");
    }
}
