use std::collections::VecDeque;
use std::io::Cursor;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use image::codecs::jpeg::{JpegDecoder, JpegEncoder};
use image::codecs::png::{PngDecoder, PngEncoder};
use image::codecs::webp::WebPDecoder;
use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView, ImageDecoder, ImageEncoder, ImageFormat, Limits};
use sha2::{Digest, Sha256};
use tokio::sync::Semaphore;
use tokio::time::{Instant, timeout_at};

const MIN_BASE64_BYTES: usize = 256 * 1024;
const MAX_BASE64_BYTES: usize = 16 * 1024 * 1024;
const MAX_SOURCE_SIDE: u32 = 8192;
const MAX_SOURCE_PIXELS: u64 = 24 * 1024 * 1024;
const MAX_DECODE_BYTES: u64 = 128 * 1024 * 1024;
const TARGET_SIDE: u32 = 1600;
const JPEG_QUALITY: u8 = 60;
const IMAGE_TIMEOUT: Duration = Duration::from_secs(5);

const MAX_CACHE_ENTRIES: usize = 32;
const MAX_CACHE_ENTRY_BYTES: usize = 512 * 1024;
type Compression = (&'static str, String);
type CacheKey = (ImageFormat, [u8; 32]);
type CacheEntry = (CacheKey, Option<Arc<Compression>>);
// 只在内存中保存较小的输出和原图散列，供同一张历史截图再次发送时复用。
static CACHE: Mutex<VecDeque<CacheEntry>> = Mutex::new(VecDeque::new());
static ENCODERS: Semaphore = Semaphore::const_new(2);

pub(crate) async fn compress_data_url(url: &str) -> Option<String> {
    let (header, data) = url.strip_prefix("data:")?.split_once(',')?;
    let media_type = header.strip_suffix(";base64")?;
    let (compressed_media_type, compressed_data) = compress_base64_image(media_type, data).await?;
    Some(format!(
        "data:{compressed_media_type};base64,{compressed_data}"
    ))
}

pub(crate) async fn compress_base64_image(media_type: &str, data: &str) -> Option<Compression> {
    if data.len() <= MIN_BASE64_BYTES || data.len() > MAX_BASE64_BYTES {
        return None;
    }
    let format = match media_type {
        "image/jpeg" | "image/jpg" => ImageFormat::Jpeg,
        "image/png" => ImageFormat::Png,
        "image/webp" => ImageFormat::WebP,
        _ => return None,
    };
    let original_size = media_type.len() + data.len();

    // 排队期间借用原图。超时透传后，尚未结束的编码仍占用原许可。
    let deadline = Instant::now() + IMAGE_TIMEOUT;
    let permit = timeout_at(deadline, ENCODERS.acquire()).await.ok()?.ok()?;
    let data = data.to_owned();
    let task = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        compress_cached(format, &data)
    });
    let compressed = timeout_at(deadline, task).await.ok()?.ok()??;
    (compressed.0.len() + compressed.1.len() < original_size)
        .then(|| (compressed.0, compressed.1.clone()))
}

fn compress_cached(format: ImageFormat, data: &str) -> Option<Arc<Compression>> {
    let key = (format, Sha256::digest(data.as_bytes()).into());
    if let Ok(mut cache) = CACHE.lock() {
        if let Some(index) = cache.iter().position(|entry| entry.0 == key) {
            let entry = cache.remove(index)?;
            let value = entry.1.clone();
            cache.push_back(entry);
            return value;
        }
    }
    let value = compress(format, data).map(Arc::new);
    if value
        .as_deref()
        .is_none_or(|(_, encoded)| encoded.len() <= MAX_CACHE_ENTRY_BYTES)
    {
        if let Ok(mut cache) = CACHE.lock() {
            if let Some(index) = cache.iter().position(|entry| entry.0 == key) {
                cache.remove(index);
            }
            if cache.len() == MAX_CACHE_ENTRIES {
                cache.pop_front();
            }
            cache.push_back((key, value.clone()));
        }
    }
    value
}

fn compress(format: ImageFormat, data: &str) -> Option<Compression> {
    let bytes = STANDARD.decode(data).ok()?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SOURCE_SIDE);
    limits.max_image_height = Some(MAX_SOURCE_SIDE);
    limits.max_alloc = Some(MAX_DECODE_BYTES);

    let source = Cursor::new(bytes.as_slice());
    let image = match format {
        ImageFormat::Png => {
            let decoder = PngDecoder::with_limits(source, limits).ok()?;
            if decoder.is_apng().ok()? {
                return None;
            }
            decode(decoder)?
        }
        ImageFormat::Jpeg => {
            let mut decoder = JpegDecoder::new(source).ok()?;
            decoder.set_limits(limits).ok()?;
            decode(decoder)?
        }
        ImageFormat::WebP => {
            let mut decoder = WebPDecoder::new(Cursor::new(bounded_webp(&bytes)?)).ok()?;
            if decoder.has_animation() {
                return None;
            }
            decoder.set_limits(limits).ok()?;
            decode(decoder)?
        }
        _ => return None,
    };
    // 高位深 PNG 保留原图。常见的完全不透明 RGBA 截图可走 JPEG。
    let has_transparency = match &image {
        DynamicImage::ImageRgba8(pixels) => pixels.pixels().any(|pixel| pixel[3] != 255),
        DynamicImage::ImageLumaA8(pixels) => pixels.pixels().any(|pixel| pixel[1] != 255),
        DynamicImage::ImageRgb8(_) | DynamicImage::ImageLuma8(_) => false,
        _ => return None,
    };
    let (width, height) = image.dimensions();
    let image = if width.max(height) > TARGET_SIDE {
        image.resize(TARGET_SIDE, TARGET_SIDE, FilterType::Triangle)
    } else {
        image
    };
    let (width, height) = image.dimensions();
    let mut output = Vec::new();
    let media_type = if has_transparency {
        let pixels = image.into_rgba8();
        PngEncoder::new(&mut output)
            .write_image(&pixels, width, height, image::ExtendedColorType::Rgba8)
            .ok()?;
        "image/png"
    } else {
        let pixels = image.into_rgb8();
        JpegEncoder::new_with_quality(&mut output, JPEG_QUALITY)
            .encode(&pixels, width, height, image::ExtendedColorType::Rgb8)
            .ok()?;
        "image/jpeg"
    };
    (output.len() < bytes.len()).then(|| (media_type, STANDARD.encode(output)))
}

fn bounded_webp(bytes: &[u8]) -> Option<&[u8]> {
    let declared_size = u32::from_le_bytes(bytes.get(4..8)?.try_into().ok()?) as usize;
    let bytes = bytes.get(..8usize.checked_add(declared_size)?)?;
    if bytes.get(..4)? != b"RIFF" || bytes.get(8..12)? != b"WEBP" {
        return None;
    }
    let mut chunks = bytes.get(12..)?;
    while !chunks.is_empty() {
        let size = u32::from_le_bytes(chunks.get(4..8)?.try_into().ok()?) as usize;
        let rest = chunks.get(8..)?;
        let payload = rest.get(..size)?;
        // 扩展 WebP 有独立的画布和图像尺寸，对有损比特流的实际尺寸也要限额。
        match chunks.get(..4)? {
            b"VP8 " => {
                let width = u16::from_le_bytes(payload.get(6..8)?.try_into().ok()?) & 0x3fff;
                let height = u16::from_le_bytes(payload.get(8..10)?.try_into().ok()?) & 0x3fff;
                if !valid_dimensions(u32::from(width), u32::from(height)) {
                    return None;
                }
            }
            b"ANIM" | b"ANMF" => return None,
            _ => {}
        }
        chunks = rest.get(size.checked_add(size % 2)?..)?;
    }
    Some(bytes)
}

fn valid_dimensions(width: u32, height: u32) -> bool {
    width > 0
        && height > 0
        && width <= MAX_SOURCE_SIDE
        && height <= MAX_SOURCE_SIDE
        && u64::from(width) * u64::from(height) <= MAX_SOURCE_PIXELS
}

fn decode(mut decoder: impl ImageDecoder) -> Option<DynamicImage> {
    let (width, height) = decoder.dimensions();
    if !valid_dimensions(width, height) || decoder.total_bytes() > MAX_DECODE_BYTES {
        return None;
    }
    let orientation = decoder.orientation().ok()?;
    let mut image = DynamicImage::from_decoder(decoder).ok()?;
    image.apply_orientation(orientation);
    Some(image)
}

#[cfg(test)]
fn sample_pixels(width: u32, height: u32, transparent: bool) -> image::RgbaImage {
    let mut random = 123_456_789u32;
    image::RgbaImage::from_fn(width, height, |x, _| {
        random ^= random << 13;
        random ^= random >> 17;
        random ^= random << 5;
        let [red, green, blue, _] = random.to_le_bytes();
        let alpha = if transparent && x < width / 2 {
            40
        } else {
            255
        };
        image::Rgba([red, green, blue, alpha])
    })
}

#[cfg(test)]
fn sample_png(width: u32, height: u32, transparent: bool) -> String {
    let pixels = sample_pixels(width, height, transparent);
    let mut bytes = Vec::new();
    PngEncoder::new(&mut bytes)
        .write_image(&pixels, width, height, image::ExtendedColorType::Rgba8)
        .unwrap();
    STANDARD.encode(bytes)
}

#[cfg(test)]
pub(crate) fn sample_large_image_data_url() -> String {
    let data = sample_png(2000, 160, false);
    assert!(data.len() > MIN_BASE64_BYTES);
    format!("data:image/png;base64,{data}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn compresses_large_images_and_leaves_other_urls_untouched() {
        let large_url = sample_large_image_data_url();
        let output = compress_data_url(&large_url).await.unwrap();
        assert!(output.len() < large_url.len() / 2);
        let encoded = output.strip_prefix("data:image/jpeg;base64,").unwrap();
        let result = image::load_from_memory(&STANDARD.decode(encoded).unwrap()).unwrap();
        assert_eq!(result.dimensions(), (1600, 128));

        for url in [
            "https://example.invalid/image.png",
            "data:image/png;base64,AAAA",
            "data:application/pdf;base64,AAAA",
        ] {
            assert!(compress_data_url(url).await.is_none());
        }
        let invalid = format!("data:image/png;base64,{}", "!".repeat(MIN_BASE64_BYTES + 4));
        assert!(compress_data_url(&invalid).await.is_none());
    }

    #[test]
    fn retains_png_transparency() {
        let data = sample_png(2000, 160, true);
        let (mime, encoded) = compress(ImageFormat::Png, &data).unwrap();
        assert_eq!(mime, "image/png");
        let result = image::load_from_memory(&STANDARD.decode(encoded).unwrap())
            .unwrap()
            .into_rgba8();
        assert_eq!(result.dimensions(), (1600, 128));
        assert_eq!(result.get_pixel(0, 0)[3], 40);
        assert_eq!(result.get_pixel(1599, 0)[3], 255);
    }

    #[test]
    fn compresses_still_webp_without_losing_transparency() {
        let pixels = sample_pixels(2000, 160, true);
        let mut image_bytes = Vec::new();
        image::codecs::webp::WebPEncoder::new_lossless(&mut image_bytes)
            .write_image(&pixels, 2000, 160, image::ExtendedColorType::Rgba8)
            .unwrap();
        let data = STANDARD.encode(image_bytes);
        assert!(data.len() > MIN_BASE64_BYTES);
        let (mime, encoded) = compress(ImageFormat::WebP, &data).unwrap();
        assert_eq!(mime, "image/png");
        assert!(encoded.len() < data.len());
        let result = image::load_from_memory(&STANDARD.decode(encoded).unwrap())
            .unwrap()
            .into_rgba8();
        assert_eq!(result.dimensions(), (1600, 128));
        assert_eq!(result.get_pixel(0, 0)[3], 40);
        assert_eq!(result.get_pixel(1599, 0)[3], 255);
    }

    #[test]
    fn large_payload_with_small_dimensions_is_never_upscaled() {
        let data = sample_png(420, 220, false);
        assert!(data.len() > MIN_BASE64_BYTES);
        let (_, encoded) = compress(ImageFormat::Png, &data).unwrap();
        let result = image::load_from_memory(&STANDARD.decode(encoded).unwrap()).unwrap();
        assert_eq!(result.dimensions(), (420, 220));
    }
}
