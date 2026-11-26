//! Seedance 2.0 + shared output-video copyright English sentence.

use super::signal::{
    contains_any_case_insensitive, extract_http_status_prefix, extract_numeric_code_from_text,
};
use super::{hit, ProviderErrorMatch};

pub fn match_output_video_copyright(api_type: &str, raw: &str) -> Option<ProviderErrorMatch> {
    if api_type != "video" {
        return None;
    }
    let code = extract_numeric_code_from_text(raw);
    let lower = raw.to_ascii_lowercase();
    if contains_any_case_insensitive(
        raw,
        &[
            "output video may be related to copyright",
            "copyright restrictions",
        ],
    ) || ((code == Some(50516) || code == Some(50519))
        && (raw.contains("Copyright") || lower.contains("copyright")))
    {
        return Some(hit("output_video_copyright_rejected", true));
    }
    None
}

pub fn match_seedance(
    api_type: &str,
    provider_kind: &str,
    raw: &str,
) -> Option<ProviderErrorMatch> {
    if api_type != "video" || provider_kind != "video_seedance_2_0" {
        return None;
    }
    if extract_http_status_prefix(raw).as_deref() == Some("404") {
        return Some(hit("endpoint_not_found", false));
    }
    if contains_any_case_insensitive(
        raw,
        &[
            "first/last frame content cannot be mixed",
            "first/last frame content cannot be mixed with reference media",
        ],
    ) {
        return Some(hit("seedance_content_scene_mutex", false));
    }
    if extract_http_status_prefix(raw).as_deref() == Some("401")
        || contains_any_case_insensitive(raw, &["unauthorized", "authentication", "api key"])
    {
        return Some(hit("auth_failed", false));
    }
    None
}
