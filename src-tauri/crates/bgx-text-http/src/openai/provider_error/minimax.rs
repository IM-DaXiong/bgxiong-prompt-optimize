//! MiniMax H3 / M3 (Anthropic) vendor errors.

use super::signal::{
    contains_any_case_insensitive, extract_http_status_prefix, extract_vendor_error_type,
};
use super::{hit, ProviderErrorMatch};

fn minimax_h3_surface(provider_kind: &str, raw: &str) -> bool {
    provider_kind == "video_minimax_h3"
        || provider_kind.contains("minimax_h3")
        || contains_any_case_insensitive(raw, &["MiniMax-H3", "Context-IR"])
}

fn minimax_image_surface(provider_kind: &str) -> bool {
    provider_kind.contains("minimax_image")
}

pub fn match_minimax(provider_kind: &str, raw: &str) -> Option<ProviderErrorMatch> {
    if provider_kind == "text_minimax_m3" && token_plan_exhausted(raw) {
        let mut matched = hit("minimax_token_plan_exhausted", false);
        matched.authoritative_vendor_code = Some("2056".into());
        return Some(matched);
    }
    let err_type = extract_vendor_error_type(raw).unwrap_or_default();
    let h3 = minimax_h3_surface(provider_kind, raw);
    let img = minimax_image_surface(provider_kind);

    if img {
        if contains_any_case_insensitive(raw, &["不支持负面提示词"]) {
            return Some(hit("negative_prompt_unsupported", false));
        }
        if contains_any_case_insensitive(
            raw,
            &[
                "status_code\":1002",
                "status_code\": 1002",
                "status_code=1002",
            ],
        ) {
            return Some(hit("rate_limited", true));
        }
        if contains_any_case_insensitive(
            raw,
            &[
                "status_code\":1004",
                "status_code\": 1004",
                "status_code=1004",
            ],
        ) {
            return Some(hit("auth_failed", false));
        }
        if contains_any_case_insensitive(
            raw,
            &[
                "status_code\":1008",
                "status_code\": 1008",
                "status_code=1008",
            ],
        ) {
            return Some(hit("vendor_insufficient_balance", false));
        }
        if contains_any_case_insensitive(
            raw,
            &[
                "status_code\":1026",
                "status_code\": 1026",
                "status_code=1026",
            ],
        ) {
            return Some(hit("prompt_risk_rejected", false));
        }
        if contains_any_case_insensitive(
            raw,
            &[
                "status_code\":2013",
                "status_code\": 2013",
                "status_code=2013",
            ],
        ) {
            return Some(hit("invalid_request", false));
        }
        if contains_any_case_insensitive(
            raw,
            &[
                "status_code\":2049",
                "status_code\": 2049",
                "status_code=2049",
            ],
        ) {
            return Some(hit("auth_failed", false));
        }
    }

    if h3
        && contains_any_case_insensitive(raw, &["TokenPlan"])
        && contains_any_case_insensitive(raw, &["Credit"])
        && contains_any_case_insensitive(raw, &["MiniMax-H3", "暂不支持"])
    {
        return Some(hit("minimax_h3_token_plan_or_credit_unsupported", false));
    }

    if h3
        && (contains_any_case_insensitive(
            raw,
            &["insufficient_balance_error", "insufficient balance"],
        ) || err_type == "insufficient_balance_error"
            || ((contains_any_case_insensitive(raw, &["HTTP 402", "Payment Required"])
                || contains_any_case_insensitive(raw, &["\"http_code\":\"402\""]))
                && contains_any_case_insensitive(raw, &["1008", "insufficient"])))
    {
        return Some(hit("minimax_h3_insufficient_balance", false));
    }

    // Anthropic error.type table is MiniMax-only. OpenAI uses the same
    // `invalid_request_error` token on 401 bodies; those must hit generic auth_failed.
    let minimax_provider = provider_kind.contains("minimax") || h3;
    if minimax_provider || err_type == "insufficient_balance_error" {
        match err_type.as_str() {
            "authentication_error" => return Some(hit("auth_failed", false)),
            "permission_error" => return Some(hit("resource_not_granted", false)),
            "not_found_error" => return Some(hit("model_not_found", false)),
            "request_too_large" => return Some(hit("request_too_large", false)),
            "rate_limit_error" => return Some(hit("rate_limited", true)),
            "overloaded_error" => return Some(hit("overloaded", true)),
            "invalid_request_error" if minimax_provider && contains_any_case_insensitive(raw, &["media exceeds size limit"]) => {
                return Some(hit("media_too_large", false))
            }
            "invalid_request_error" if minimax_provider => {
                return Some(hit("invalid_request", false))
            }
            "api_error" if minimax_provider => return Some(hit("api_error", true)),
            "insufficient_balance_error" => {
                return Some(hit("vendor_insufficient_balance", false));
            }
            _ => {}
        }
    }

    if extract_http_status_prefix(raw).as_deref() == Some("402")
        && contains_any_case_insensitive(raw, &["insufficient"])
        && contains_any_case_insensitive(raw, &["minimax", "MiniMax"])
    {
        return Some(hit("vendor_insufficient_balance", false));
    }

    None
}

/// MiniMax Anthropic Token Plan code, only from its strict HTTP error envelope.
fn token_plan_exhausted(raw: &str) -> bool {
    let Some(rest) = raw.strip_prefix("HTTP 429 Too Many Requests: ") else {
        return false;
    };
    let Ok(body) = serde_json::from_str::<serde_json::Value>(rest) else {
        return false;
    };
    body.get("type").and_then(|v| v.as_str()) == Some("error")
        && body.pointer("/error/type").and_then(|v| v.as_str()) == Some("rate_limit_error")
        && body.pointer("/error/message").and_then(|v| v.as_str())
            .is_some_and(|message| message.trim().ends_with("(2056)"))
}
