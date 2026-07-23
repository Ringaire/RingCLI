//! 图片处理：读取 + 自动压缩（适配各 provider 的体积上限）。
//!
//! 各 provider 图片体积上限不同（智谱 5MB、OpenAI 20MB、Anthropic 5MB）。
//! 统一压到安全阈值（默认 4MB），避免被 provider 拒绝。
//! 压缩策略：原文件 < 阈值则原样返回；否则 resize 长边到 2048 + JPEG 质量递降。

use std::path::Path;

use base64::{engine::general_purpose::STANDARD, Engine as _};

/// 默认安全体积上限（4MB，满足大多数 provider 的 5MB 限制）。
pub const DEFAULT_MAX_BYTES: usize = 4 * 1024 * 1024;
/// 压缩目标长边（像素）。
const MAX_DIMENSION: u32 = 2048;

/// 判断文件是否为支持的图片格式（按扩展名）。
pub fn is_image(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()).map(str::to_lowercase).as_deref(),
        Some("png") | Some("jpg") | Some("jpeg") | Some("gif") | Some("webp")
    )
}

/// 读取图片并按需压缩。返回 (media_type, base64_data)。
///
/// - 原文件 < `max_bytes` → 原样 base64 编码（保留原格式）
/// - 超限 → 解码 → resize → JPEG 质量递降 → 直到 < `max_bytes`
///
/// 失败返回 None（调用方跳过）。
pub fn read_and_compress(path: &Path, max_bytes: usize) -> Option<(String, String)> {
    let raw = std::fs::read(path).ok()?;
    let media_type = mime_from_path(path);

    // 原文件在限内 → 直接用
    if raw.len() <= max_bytes {
        let b64 = STANDARD.encode(&raw);
        return Some((media_type.to_string(), b64));
    }

    // 超限 → 解码 + resize + JPEG 压缩
    compress_to_jpeg(&raw, max_bytes).map(|data| {
        let b64 = STANDARD.encode(&data);
        ("image/jpeg".to_string(), b64)
    })
}

/// 解码 + resize + JPEG 质量递降，压到 < max_bytes。
fn compress_to_jpeg(raw: &[u8], max_bytes: usize) -> Option<Vec<u8>> {
    use image::ImageReader;
    use std::io::Cursor;

    let img = ImageReader::new(Cursor::new(raw))
        .with_guessed_format()
        .ok()?
        .decode()
        .ok()?
        .to_rgb8();

    let (w, h) = img.dimensions();
    let scale = if w.max(h) > MAX_DIMENSION {
        MAX_DIMENSION as f64 / w.max(h) as f64
    } else {
        1.0
    };
    let resized = if scale < 1.0 {
        image::imageops::resize(
            &img,
            (w as f64 * scale).round() as u32,
            (h as f64 * scale).round() as u32,
            image::imageops::FilterType::Lanczos3,
        )
    } else {
        img
    };

    // 质量递降，直到满足体积
    use image::codecs::jpeg::JpegEncoder;
    let (rw, rh) = resized.dimensions();
    for q in [85u8, 75, 65, 55, 45, 35] {
        let mut buf = Cursor::new(Vec::new());
        let mut enc = JpegEncoder::new_with_quality(&mut buf, q);
        if enc.encode_image(&image::DynamicImage::ImageRgb8(resized.clone())).is_err() {
            continue;
        }
        let data = buf.into_inner();
        if data.len() <= max_bytes {
            let _ = (rw, rh); // 抑制未用警告
            return Some(data);
        }
    }
    None
}

fn mime_from_path(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_lowercase)
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        _ => "image/jpeg",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_image_detection() {
        assert!(is_image(Path::new("photo.png")));
        assert!(is_image(Path::new("photo.JPG")));
        assert!(is_image(Path::new("a.jpeg")));
        assert!(is_image(Path::new("x.webp")));
        assert!(!is_image(Path::new("readme.md")));
        assert!(!is_image(Path::new("code.rs")));
    }

    #[test]
    fn mime_detection() {
        assert_eq!(mime_from_path(Path::new("a.png")), "image/png");
        assert_eq!(mime_from_path(Path::new("a.JPG")), "image/jpeg");
        assert_eq!(mime_from_path(Path::new("a.gif")), "image/gif");
    }

    #[test]
    fn small_file_kept_as_is() {
        // 构造一个 1x1 PNG，远小于上限 → 应原样返回 png
        let png = [
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, // PNG signature
        ];
        let tmp = std::env::temp_dir().join("ring_img_test.png");
        std::fs::write(&tmp, png).unwrap();
        let result = read_and_compress(&tmp, DEFAULT_MAX_BYTES);
        let _ = std::fs::remove_file(&tmp);
        // 1x1 不完整 PNG 解码会失败，但 raw < max_bytes 走原样分支
        assert!(result.is_some());
        assert_eq!(result.unwrap().0, "image/png");
    }
}
