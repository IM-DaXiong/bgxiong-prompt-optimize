//! Vision policy and original-bytes encoding.
//! Caps come from prompt-optimize-vision-limits.json via build.rs; do not hard-code 16.

use base64::Engine;
use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, GenericImageView, RgbImage};

use crate::error::OptimizeError;
use crate::error::error_codes;
use crate::image_job::{SubmittedImage, TextVisionCaps};

include!(concat!(
    env!("OUT_DIR"),
    "/prompt_optimize_vision_limits.rs"
));

/// Shortest edge below this is rejected before HTTP on the shrink path only.
pub const MIN_LLM_VISION_EDGE_PX: u32 = 16;

/// Official DeepSeek Vision inline / URL single-image hard cap (32 MiB).
/// Shared LLM encode must Err — no truncate, no Files fallback.
pub const DEEPSEEK_VISION_INLINE_MAX_BYTES: usize = 32 * 1024 * 1024;

pub const VISION_SKIP_NONE: &str = "";
pub const VISION_SKIP_NO_CANDIDATES: &str = "no_candidates";
pub const VISION_SKIP_NO_PATH: &str = "no_resolvable_path";
pub const VISION_SKIP_NO_VISION: &str = "model_no_vision";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VisionAttachDecision {
    TextOnly,
    Attach { take: u32, truncated: bool },
}

#[derive(Debug, Clone, Copy)]
pub struct VisionInputCaps {
    pub supports_vision: bool,
    pub max_images: u32,
}

/// Temporary alias kept for in-crate call sites during the signature sync.
pub type VisionDecideCaps = VisionInputCaps;

impl From<&TextVisionCaps> for VisionInputCaps {
    fn from(caps: &TextVisionCaps) -> Self {
        Self {
            supports_vision: caps.supports_vision,
            max_images: if caps.supports_vision {
                MAX_IPC_RELS
            } else {
                0
            },
        }
    }
}

/// Decide how many candidate images to attach after the vision gate has run.
pub fn decide(caps: &VisionInputCaps, candidate_count: u32) -> VisionAttachDecision {
    if candidate_count == 0 || !caps.supports_vision || caps.max_images == 0 {
        return VisionAttachDecision::TextOnly;
    }
    let take = candidate_count.min(caps.max_images);
    VisionAttachDecision::Attach {
        take,
        truncated: candidate_count > caps.max_images,
    }
}

pub fn reject_if_over_submitted_cap(count: u32) -> Result<(), OptimizeError> {
    if count > MAX_IPC_RELS {
        return Err(OptimizeError::new(
            error_codes::IMAGE_PROMPT_REFERENCE_LIMIT,
            "参考图超过应用请求数量上限，未发送本次请求。",
        ));
    }
    Ok(())
}

pub fn cap_optional_channel(mut images: Vec<SubmittedImage>) -> (Vec<SubmittedImage>, bool) {
    if images.len() as u32 > MAX_IPC_RELS {
        images.truncate(MAX_IPC_RELS as usize);
        return (images, true);
    }
    (images, false)
}

#[derive(Debug, Clone)]
pub struct EncodedLlmImage {
    pub data_url: String,
    pub payload_bytes: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct ShrinkBudget {
    pub max_edge_candidates: &'static [u32],
    pub jpeg_qualities: &'static [u8],
    pub max_encoded_bytes: usize,
}

pub const LLM_REFERENCE_BUDGET: ShrinkBudget = ShrinkBudget {
    max_edge_candidates: &[2048, 1536, 1280, 1024],
    jpeg_qualities: &[85, 75, 65],
    max_encoded_bytes: 2 * 1024 * 1024,
};

#[derive(Debug, Clone)]
pub struct ShrunkImage {
    pub bytes: Vec<u8>,
    pub mime: &'static str,
    pub quality: u8,
    pub width: u32,
    pub height: u32,
}

fn sniff_ext_to_mime(ext: &str) -> Result<&'static str, String> {
    match ext {
        "png" => Ok("image/png"),
        "jpg" => Ok("image/jpeg"),
        "gif" => Ok("image/gif"),
        "webp" => Ok("image/webp"),
        other => Err(format!("unsupported image ext for llm: {other}")),
    }
}

pub fn sniff_image_ext(bytes: &[u8]) -> Result<&'static str, String> {
    if bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A])
        || bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47])
    {
        return Ok("png");
    }
    if bytes.len() >= 3 && bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF {
        return Ok("jpg");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Ok("gif");
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Ok("webp");
    }
    match image::guess_format(bytes) {
        Ok(image::ImageFormat::Png) => Ok("png"),
        Ok(image::ImageFormat::Jpeg) => Ok("jpg"),
        Ok(image::ImageFormat::Gif) => Ok("gif"),
        Ok(image::ImageFormat::WebP) => Ok("webp"),
        Ok(other) => Err(format!("unsupported image format: {other:?}")),
        Err(_) => Err("unrecognized image magic (cannot sniff extension)".into()),
    }
}

pub fn to_data_url(mime: &str, bytes: &[u8]) -> String {
    let mut out = String::with_capacity(mime.len() + 16 + bytes.len().div_ceil(3) * 4);
    out.push_str("data:");
    out.push_str(mime);
    out.push_str(";base64,");
    base64::engine::general_purpose::STANDARD.encode_string(bytes, &mut out);
    out
}

pub fn probe_dimensions(bytes: &[u8]) -> Result<(u32, u32), String> {
    image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| format!("probe format failed: {e}"))?
        .into_dimensions()
        .map_err(|e| format!("probe dimensions failed: {e}"))
}

pub fn within_budget(encoded_len: usize, width: u32, height: u32, budget: &ShrinkBudget) -> bool {
    let max_edge = budget
        .max_edge_candidates
        .first()
        .copied()
        .unwrap_or(u32::MAX);
    encoded_len <= budget.max_encoded_bytes && width.max(height) <= max_edge
}

fn flatten_to_rgb_on_white(img: &DynamicImage) -> RgbImage {
    if !img.color().has_alpha() {
        return img.to_rgb8();
    }
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let mut out = RgbImage::new(w, h);
    for (x, y, px) in rgba.enumerate_pixels() {
        let a = px[3] as u32;
        let blend = |c: u8| -> u8 { ((c as u32 * a + 255 * (255 - a)) / 255) as u8 };
        out.put_pixel(x, y, image::Rgb([blend(px[0]), blend(px[1]), blend(px[2])]));
    }
    out
}

fn encode_jpeg(img: &RgbImage, quality: u8) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    let mut enc = JpegEncoder::new_with_quality(&mut buf, quality);
    enc.encode_image(img)
        .map_err(|e| format!("encode jpeg failed: {e}"))?;
    Ok(buf)
}

pub fn shrink_image_to_budget(src: &[u8], budget: &ShrinkBudget) -> Result<ShrunkImage, String> {
    let base = image::load_from_memory(src).map_err(|e| format!("decode image failed: {e}"))?;
    let (sw, sh) = base.dimensions();
    let longest = sw.max(sh);

    for &max_edge in budget.max_edge_candidates {
        let resized = if longest <= max_edge {
            None
        } else {
            let scale = max_edge as f64 / longest as f64;
            let nw = ((sw as f64 * scale).round() as u32).max(1);
            let nh = ((sh as f64 * scale).round() as u32).max(1);
            Some(base.resize(nw, nh, image::imageops::FilterType::Triangle))
        };
        let target = resized.as_ref().unwrap_or(&base);
        let rgb = flatten_to_rgb_on_white(target);
        let (tw, th) = (rgb.width(), rgb.height());
        for &quality in budget.jpeg_qualities {
            let buf = encode_jpeg(&rgb, quality)?;
            if buf.len() <= budget.max_encoded_bytes {
                return Ok(ShrunkImage {
                    bytes: buf,
                    mime: "image/jpeg",
                    quality,
                    width: tw,
                    height: th,
                });
            }
        }
    }

    Err(format!(
        "image too large after compression (> {} bytes)",
        budget.max_encoded_bytes
    ))
}

fn encode_error(message: impl Into<String>) -> OptimizeError {
    OptimizeError::new("IMAGE_PROMPT_VISION_ENCODE", message)
}

/// Workbench-authored input: validate the file, preserve its bytes and dimensions.
/// No model capacity, minimum edge, resizing or re-encoding policy applies here.
pub fn encode_original_image_bytes(bytes: &[u8]) -> Result<EncodedLlmImage, OptimizeError> {
    if bytes.is_empty() {
        return Err(encode_error("参考图文件为空，未发送本次请求。"));
    }
    let ext = sniff_image_ext(bytes).map_err(encode_error)?;
    let mime = sniff_ext_to_mime(ext).map_err(encode_error)?;
    image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| encode_error(format!("参考图格式读取失败：{e}")))?
        .decode()
        .map_err(|e| encode_error(format!("参考图解码失败：{e}")))?;
    Ok(EncodedLlmImage {
        data_url: to_data_url(mime, bytes),
        payload_bytes: bytes.len() as u64,
    })
}

/// Sniff magic → budget check → data URL. Unknown format is Err (no default png).
pub fn encode_image_bytes_for_llm(bytes: &[u8]) -> Result<EncodedLlmImage, OptimizeError> {
    if bytes.is_empty() {
        return Err(encode_error("empty image bytes"));
    }
    if bytes.len() > DEEPSEEK_VISION_INLINE_MAX_BYTES {
        return Err(encode_error(format!(
            "单张图片 {} 字节超过 32MB 上限，无法发给识图模型。请换更小的图或先压缩。",
            bytes.len()
        )));
    }
    let ext = sniff_image_ext(bytes).map_err(encode_error)?;
    let mime = sniff_ext_to_mime(ext).map_err(encode_error)?;
    let (w, h) = probe_dimensions(bytes).map_err(encode_error)?;
    if w < MIN_LLM_VISION_EDGE_PX || h < MIN_LLM_VISION_EDGE_PX {
        return Err(encode_error(format!(
            "reference image too small ({w}x{h}px); shortest edge must be at least {MIN_LLM_VISION_EDGE_PX}px"
        )));
    }
    if within_budget(bytes.len(), w, h, &LLM_REFERENCE_BUDGET) {
        return Ok(EncodedLlmImage {
            data_url: to_data_url(mime, bytes),
            payload_bytes: bytes.len() as u64,
        });
    }
    let shrunk = shrink_image_to_budget(bytes, &LLM_REFERENCE_BUDGET).map_err(encode_error)?;
    Ok(EncodedLlmImage {
        data_url: to_data_url(shrunk.mime, &shrunk.bytes),
        payload_bytes: shrunk.bytes.len() as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps(supports: bool, max: u32) -> VisionInputCaps {
        VisionInputCaps {
            supports_vision: supports,
            max_images: max,
        }
    }

    fn png_bytes(w: u32, h: u32) -> Vec<u8> {
        let img: image::ImageBuffer<image::Rgb<u8>, Vec<u8>> =
            image::ImageBuffer::from_fn(w, h, |_, _| image::Rgb([7u8, 8, 9]));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn zero_images_is_text_only() {
        assert_eq!(decide(&caps(true, 9), 0), VisionAttachDecision::TextOnly);
    }

    #[test]
    fn no_vision_with_images_is_text_only() {
        assert_eq!(decide(&caps(false, 0), 3), VisionAttachDecision::TextOnly);
        assert_eq!(decide(&caps(true, 0), 3), VisionAttachDecision::TextOnly);
    }

    #[test]
    fn local_vl_max_4_truncates_6() {
        assert_eq!(
            decide(&caps(true, 4), 6),
            VisionAttachDecision::Attach {
                take: 4,
                truncated: true
            }
        );
    }

    #[test]
    fn cloud_vl_max_9_keeps_2() {
        assert_eq!(
            decide(&caps(true, 9), 2),
            VisionAttachDecision::Attach {
                take: 2,
                truncated: false
            }
        );
    }

    #[test]
    fn ipc_max_rels_matches_app_contract() {
        let raw = include_str!("../../../../packages/app-contracts/v1/prompt-optimize-vision-limits.json");
        let v: serde_json::Value = serde_json::from_str(raw).expect("contract json");
        assert_eq!(
            MAX_IPC_RELS,
            v["maxIpcRels"].as_u64().expect("maxIpcRels") as u32
        );
        assert_ne!(MAX_IPC_RELS, 0);
    }

    #[test]
    fn seventeen_images_channel_a_errors_and_channel_b_caps() {
        let over = MAX_IPC_RELS + 1;
        let err = reject_if_over_submitted_cap(over).unwrap_err();
        assert_eq!(err.code, error_codes::IMAGE_PROMPT_REFERENCE_LIMIT);
        assert!(err.message.contains("上限"));
        assert!(reject_if_over_submitted_cap(MAX_IPC_RELS).is_ok());
        let over_images: Vec<SubmittedImage> = (1..=over)
            .map(|i| SubmittedImage {
                bytes: b"x".to_vec(),
                declared_order: i,
            })
            .collect();
        let (take, truncated) = cap_optional_channel(over_images);
        assert_eq!(take.len() as u32, MAX_IPC_RELS);
        assert!(truncated);
        let exact: Vec<SubmittedImage> = (1..=MAX_IPC_RELS)
            .map(|i| SubmittedImage {
                bytes: b"x".to_vec(),
                declared_order: i,
            })
            .collect();
        let (keep, flag) = cap_optional_channel(exact);
        assert_eq!(keep.len() as u32, MAX_IPC_RELS);
        assert!(!flag);
    }

    #[test]
    fn submitted_image_keeps_original_bytes_even_below_model_minimum_edge() {
        let bytes = png_bytes(2, 3);
        let encoded = encode_original_image_bytes(&bytes).unwrap();
        let payload = encoded
            .data_url
            .strip_prefix("data:image/png;base64,")
            .unwrap();
        assert_eq!(
            base64::engine::general_purpose::STANDARD
                .decode(payload)
                .unwrap(),
            bytes
        );
        assert_eq!(encoded.payload_bytes, bytes.len() as u64);
        let mut large = bytes.clone();
        large.resize(DEEPSEEK_VISION_INLINE_MAX_BYTES + 1, 0);
        let encoded_large = encode_original_image_bytes(&large).unwrap();
        assert_eq!(encoded_large.payload_bytes, large.len() as u64);
        assert_eq!(
            base64::engine::general_purpose::STANDARD
                .decode(
                    encoded_large
                        .data_url
                        .strip_prefix("data:image/png;base64,")
                        .unwrap()
                )
                .unwrap(),
            large
        );
        assert!(encode_original_image_bytes(&[]).is_err());
        assert!(encode_original_image_bytes(b"not an image").is_err());
    }

    #[test]
    fn encode_16px_png_succeeds() {
        let out = encode_image_bytes_for_llm(&png_bytes(16, 16)).expect("png");
        assert!(out.data_url.starts_with("data:image/png;base64,"));
        assert!(out.payload_bytes > 0);
    }

    #[test]
    fn encode_1x1_png_is_explicit_err() {
        let err = encode_image_bytes_for_llm(&png_bytes(1, 1)).unwrap_err();
        assert!(err.message.contains("too small"), "err={}", err.message);
    }

    #[test]
    fn encode_random_bytes_sniff_err() {
        let err = encode_image_bytes_for_llm(&[0x00, 0x01, 0x02, 0x03, 0x04]).unwrap_err();
        assert!(
            err.message.contains("unrecognized")
                || err.message.contains("unsupported")
                || err.message.contains("magic"),
            "err={}",
            err.message
        );
    }

    #[test]
    fn encode_over_32mib_is_explicit_err() {
        let oversized = vec![0u8; DEEPSEEK_VISION_INLINE_MAX_BYTES + 1];
        let err = encode_image_bytes_for_llm(&oversized).unwrap_err();
        assert!(err.message.contains("32MB"), "err={}", err.message);
    }

    #[test]
    fn jpeg_is_not_labelled_png() {
        let img: image::ImageBuffer<image::Rgb<u8>, Vec<u8>> =
            image::ImageBuffer::from_fn(16, 16, |_, _| image::Rgb([9u8, 8, 7]));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Jpeg).unwrap();
        let bytes = out.into_inner();
        let encoded = encode_original_image_bytes(&bytes).unwrap();
        assert!(encoded.data_url.starts_with("data:image/jpeg;base64,"));
        assert!(!encoded.data_url.starts_with("data:image/png"));
    }
}