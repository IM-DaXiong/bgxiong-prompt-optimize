//! Jimeng / 即梦 visual codes + shared reference-image copyright / real-person.

use super::signal::{contains_any_case_insensitive, extract_numeric_code_from_text};
use super::{hit, ProviderErrorMatch};

fn is_jimeng_visual_provider(kind: &str) -> bool {
    kind.contains("jimeng") || kind.contains("volcengine_jimeng")
}

pub fn match_jimeng(api_type: &str, provider_kind: &str, raw: &str) -> Option<ProviderErrorMatch> {
    let image = api_type == "image";
    let video = api_type == "video";
    let code = extract_numeric_code_from_text(raw);
    let lower = raw.to_ascii_lowercase();

    if (image || video)
        && (code == Some(50518)
            || (contains_any_case_insensitive(raw, &["pre img risk not pass"])
                && (raw.contains("Copyright") || lower.contains("copyright"))))
    {
        return Some(hit("reference_image_copyright_rejected", false));
    }

    if contains_any_case_insensitive(
        raw,
        &[
            "InputImageSensitiveContentDetected.PrivacyInformation",
            "input image may contain real person",
            "may contain real person",
        ],
    ) {
        return Some(hit("reference_image_real_person_rejected", false));
    }

    if contains_any_case_insensitive(
        raw,
        &[
            "InputImageSensitiveContentDetected",
            "ImageSensitiveContentDetected",
        ],
    ) || (contains_any_case_insensitive(raw, &["input image", "reference image"])
        && contains_any_case_insensitive(raw, &["sensitive", "content", "detected", "policy"]))
    {
        return Some(hit("reference_image_content_rejected", false));
    }

    if !(is_jimeng_visual_provider(provider_kind) || image) {
        return None;
    }

    if code == Some(50412) || contains_any_case_insensitive(raw, &["text risk not pass"]) {
        return Some(hit("prompt_risk_rejected", false));
    }
    if code == Some(50411)
        || code == Some(50518)
        || contains_any_case_insensitive(raw, &["pre img risk not pass"])
    {
        return Some(hit("reference_image_risk_rejected", false));
    }
    if code == Some(50511) || contains_any_case_insensitive(raw, &["post img risk not pass"]) {
        return Some(hit("output_image_risk_rejected", false));
    }
    if code == Some(50516) || contains_any_case_insensitive(raw, &["post video risk not pass"]) {
        return Some(hit("output_video_risk_rejected", true));
    }
    if code == Some(50500)
        || (contains_any_case_insensitive(raw, &["internal error"])
            && contains_any_case_insensitive(raw, &["jimeng", "即梦", "50500"]))
    {
        return Some(hit("vendor_internal_error", true));
    }
    if contains_any_case_insensitive(
        raw,
        &["Image Decode Error", "invalid binary data file size"],
    ) || code == Some(50207)
    {
        return Some(hit("reference_image_invalid", false));
    }
    if contains_any_case_insensitive(raw, &["Error when parsing request"]) {
        return Some(hit("request_body_parse_failed", false));
    }
    None
}
