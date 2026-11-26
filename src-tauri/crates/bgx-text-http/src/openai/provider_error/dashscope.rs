//! DashScope / 万相 / Coding Plan.

use super::signal::{contains_any_case_insensitive, extract_http_status_prefix};
use super::{hit, ProviderErrorMatch};

pub fn match_dashscope(
    api_type: &str,
    provider_kind: &str,
    raw: &str,
) -> Option<ProviderErrorMatch> {
    let video = api_type == "video";
    let image = api_type == "image";
    let status = extract_http_status_prefix(raw);
    let lower = raw.to_ascii_lowercase();

    if status.as_deref() == Some("405")
        && (lower.contains("coding plan is currently only available")
            || lower.contains("coding agents"))
    {
        return Some(hit("coding_plan_not_allowed", false));
    }

    if video
        && (provider_kind == "video_dashscope_wan" || provider_kind == "video_dashscope_happyhorse")
    {
        if status.as_deref() == Some("401")
            || contains_any_case_insensitive(raw, &["invalidapikey", "no api-key", "unauthorized"])
        {
            return Some(hit("auth_failed", false));
        }
        if contains_any_case_insensitive(
            raw,
            &["current user api does not support synchronous calls"],
        ) {
            return Some(hit("async_header_required", false));
        }
    }

    if image && provider_kind == "image_dashscope_wan"
        && (status.as_deref() == Some("404")
            || contains_any_case_insensitive(raw, &["compatible-mode"]))
        {
            return Some(hit("endpoint_misconfigured", false));
        }

    if image && contains_any_case_insensitive(raw, &["未指定 model", "model 去除空白后为空"])
    {
        return Some(hit("model_not_configured", false));
    }

    None
}
