//! Kling / 可灵 vendor codes.

use super::signal::{
    contains_any_case_insensitive, extract_http_status_prefix, extract_numeric_code_from_text,
};
use super::{hit, ProviderErrorMatch};

fn is_kling_provider(kind: &str) -> bool {
    kind.starts_with("kling_") || kind.starts_with("video_kling") || kind.contains("kling")
}

pub fn match_kling(provider_kind: &str, raw: &str) -> Option<ProviderErrorMatch> {
    if !is_kling_provider(provider_kind) && !contains_any_case_insensitive(raw, &["kling", "可灵"])
    {
        return None;
    }
    let code = extract_numeric_code_from_text(raw);
    let rl = raw.to_ascii_lowercase();

    if code == Some(1002) || rl.contains("access key not found") {
        return Some(hit("kling_access_key_invalid", false));
    }
    if code == Some(1102) || rl.contains("account balance not enough") {
        return Some(hit("kling_balance_insufficient", true));
    }
    if code == Some(1001) || (rl.contains("secret key") && rl.contains("invalid")) {
        return Some(hit("kling_secret_key_invalid", false));
    }
    if extract_http_status_prefix(raw).as_deref() == Some("401")
        || (rl.contains("401") && rl.contains("unauthorized"))
    {
        return Some(hit("kling_auth_failed", false));
    }
    if rl.contains("429") && rl.contains("too many requests") && !rl.contains("account balance") {
        return Some(hit("kling_rate_limited", true));
    }
    None
}
