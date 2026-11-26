//! Cross-vendor HTTP class + unmatched fallback categories.

use super::signal::{contains_any_case_insensitive, extract_http_status_prefix};
use super::{hit, ProviderErrorMatch};

pub fn match_generic(
    _api_type: &str,
    _provider_kind: &str,
    raw: &str,
) -> Option<ProviderErrorMatch> {
    let status = extract_http_status_prefix(raw);
    let lower = raw.to_ascii_lowercase();

    if contains_any_case_insensitive(raw, &["不支持负面提示词"]) {
        return Some(hit("negative_prompt_unsupported", false));
    }

    if status.as_deref() == Some("402")
        || contains_any_case_insensitive(
            raw,
            &[
                "insufficient_balance",
                "insufficient balance",
                "insufficient_quota",
                "account balance not enough",
                "Payment Required",
            ],
        )
    {
        return Some(hit("vendor_insufficient_balance", false));
    }

    if status.as_deref() == Some("401")
        || contains_any_case_insensitive(
            raw,
            &[
                "invalid api key",
                "invalidapikey",
                "incorrect api key",
                "no api-key",
                "unauthorized",
                "invalid authentication",
            ],
        )
    {
        return Some(hit("auth_failed", false));
    }

    // P05: real 403 at nested remote exits (vendor-specific matchers run first).
    if status.as_deref() == Some("403")
        || contains_any_case_insensitive(raw, &["access denied", "permission denied"])
    {
        return Some(hit("auth_failed", false));
    }

    if status.as_deref() == Some("429")
        || contains_any_case_insensitive(
            raw,
            &[
                "rate limit",
                "too many requests",
                "quota exceeded",
                "throttling",
                "limit_requests",
            ],
        )
    {
        return Some(hit("rate_limited", true));
    }

    if status.as_deref() == Some("404")
        || contains_any_case_insensitive(
            raw,
            &[
                "model_not_found",
                "model not found",
                "does not exist",
                "invalid model",
                "model is not available",
            ],
        )
    {
        return Some(hit("model_not_found", false));
    }

    // Classify the actual upstream rejection, never infer support from model metadata.
    if status.as_deref() == Some("400") || status.as_deref() == Some("422") {
        let body = raw
            .find('{')
            .and_then(|offset| serde_json::from_str::<serde_json::Value>(&raw[offset..]).ok());
        let code = body
            .as_ref()
            .and_then(|value| value.pointer("/error/code").or_else(|| value.get("code")))
            .and_then(|value| value.as_str());
        if code == Some("image_input_not_supported")
            || contains_any_case_insensitive(
                raw,
                &[
                    "model does not support image inputs",
                    "model does not support image input",
                    "image inputs are not supported",
                    "image_url is only supported by certain models",
                ],
            )
        {
            return Some(hit("model_image_input_unsupported", false));
        }
        if code == Some("unsupported_response_format") {
            return Some(hit("model_response_format_unsupported", false));
        }
    }

    if status.as_deref() == Some("400")
        && contains_any_case_insensitive(
            raw,
            &[
                "context length",
                "maximum context",
                "token limit",
                "too many tokens",
                "max_tokens",
            ],
        )
    {
        return Some(hit("context_too_long", false));
    }

    // DashScope CosyVoice voice-design etc.: InternalError / feat extract after long run.
    if status.as_deref() == Some("500")
        || status.as_deref() == Some("502")
        || status.as_deref() == Some("503")
        || contains_any_case_insensitive(
            raw,
            &[
                "\"code\":\"InternalError\"",
                "\"code\": \"InternalError\"",
                "feat extract error",
                "internal server error",
            ],
        )
    {
        return Some(hit("vendor_internal_error", true));
    }

    if lower.contains("overloaded") {
        return Some(hit("overloaded", true));
    }

    None
}

#[cfg(test)]
mod submitted_input_tests {
    use super::*;

    #[test]
    fn actual_rejection_is_distinct_from_auth_network_and_invalid_image() {
        for (raw, expected) in [
            (
                "HTTP 400: {\"error\":{\"code\":\"image_input_not_supported\"}}",
                Some("model_image_input_unsupported"),
            ),
            (
                "HTTP 400: {\"error\":{\"code\":\"unsupported_image\"}}",
                None,
            ),
            (
                "HTTP 422: This model does not support image inputs",
                Some("model_image_input_unsupported"),
            ),
            (
                "HTTP 400: {\"error\":{\"code\":\"unsupported_response_format\"}}",
                Some("model_response_format_unsupported"),
            ),
            (
                "HTTP 401: unauthorized unsupported_image",
                Some("auth_failed"),
            ),
            ("HTTP 400: invalid image bytes", None),
            ("connection refused", None),
        ] {
            assert_eq!(
                match_generic("text", "text_openai_compat", raw)
                    .as_ref()
                    .map(|m| m.category.as_str()),
                expected,
                "{raw}"
            );
        }
    }
}

/// Poll/domain fallback when no vendor table hit (literals for i18n gate).
pub fn fallback_category(requested: &str) -> ProviderErrorMatch {
    match requested {
        "comfyui_process_down" => hit("comfyui_process_down", true),
        "comfyuiUnreachable" => hit("comfyuiUnreachable", false),
        "poll_wall_timeout" => hit("poll_wall_timeout", true),
        "vendor_query_http" => hit("vendor_query_http", true),
        "vendor_query_transport" => hit("vendor_query_transport", true),
        "download_failed" => hit("download_failed", true),
        "validation_rejected" => hit("validation_rejected", false),
        _ => hit("vendor_request_failed", false),
    }
}
