//! Content policy, minors block, reference-image capacity.

use serde_json::json;

use super::signal::{
    contains_any_case_insensitive, contains_any_normalized_token, extract_key_usize,
};
use super::{hit, hit_params, ProviderErrorMatch};

fn parse_seg_video_reference_capacity_exceeded(raw: &str) -> Option<(usize, usize)> {
    if !raw.contains("SEG_VIDEO_REFERENCE_CAPACITY_EXCEEDED") {
        return None;
    }
    let declared = extract_key_usize(raw, "declaredCount=")?;
    let max_allowed = extract_key_usize(raw, "maxAllowed=")?;
    Some((declared, max_allowed))
}

pub fn match_policy(api_type: &str, provider_kind: &str, raw: &str) -> Option<ProviderErrorMatch> {
    let lower = raw.to_ascii_lowercase();
    let video = api_type == "video";

    if contains_any_case_insensitive(
        raw,
        &[
            "CONTENT_MINORS_SEXUAL_BLOCKED",
            "未成年人性内容",
            "sexual content involving minors",
        ],
    ) {
        return Some(hit("content_minors_sexual_blocked", false));
    }

    if let Some((declared_count, max_allowed)) = parse_seg_video_reference_capacity_exceeded(raw) {
        return Some(hit_params(
            "seg_video_reference_capacity_exceeded",
            false,
            json!({
                "declaredCount": declared_count,
                "maxAllowed": max_allowed,
            }),
        ));
    }

    let phrase_policy_hit = contains_any_case_insensitive(
        raw,
        &[
            "content policy",
            "safety",
            "risk not pass",
            "policy violation",
            "moderation",
        ],
    );
    let symbolic_policy_hit =
        contains_any_normalized_token(raw, &["data_inspection_failed", "content_filter"]);
    if !(phrase_policy_hit
        || symbolic_policy_hit
        || (lower.contains("sensitive") && lower.contains("content")))
    {
        return None;
    }

    let local_ideogram_safety = contains_any_case_insensitive(
        raw,
        &[
            "Ideogram",
            "灰底占位",
            "comfyuiIdeogramSafetyBlocked",
            "Image blocked by safety filter",
        ],
    );
    let local_comfy_or_gguf = provider_kind.contains("comfyui")
        || contains_any_case_insensitive(raw, &["local", "gguf", "llama-server"]);

    if local_ideogram_safety {
        return Some(hit("local_ideogram_safety_blocked", false));
    }
    if local_comfy_or_gguf {
        return Some(hit("local_model_refused", false));
    }
    if video {
        return Some(hit("provider_content_policy_rejected_video", false));
    }
    if api_type == "text" {
        return Some(hit("provider_content_policy_rejected_text", false));
    }
    Some(hit("provider_content_policy_rejected_image", false))
}
