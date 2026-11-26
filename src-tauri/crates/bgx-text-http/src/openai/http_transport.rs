//! Thin decorate leftover from the host `http_transport`: wire decorate + batch classifier.
//! A-layer retry lives in `crate::openai::outbound_http` (INV-NET-INTERRUPT).

use super::provider_error;
use super::user_facing_api_error::{
    encode_user_facing_api_error, UserFacingApiError, USER_FACING_API_ERROR_PREFIX,
};

/// C-layer only: json_object downgrade / schema repair POSTs that received a Response.
pub const MAX_CHAT_COMPLETION_POSTS: u32 = 4;

/// 批量编排识别：transport 已耗尽且非 rate_limit 时跳过整场二次重试。
pub const TRANSPORT_RETRIES_EXHAUSTED_MARKER: &str = "transport_retries_exhausted=";

/// body 读阶段提示词表；**禁止**用于 POST 自动重试决策。
pub fn is_body_read_transport_hint(msg: &str) -> bool {
    let lower = msg.to_ascii_lowercase();
    lower.contains("bytes remaining")
        || lower.contains("reading a body")
        || lower.contains("connection reset")
        || lower.contains("broken pipe")
        || lower.contains("unexpected eof")
        || lower.contains("stream closed")
}

/// 批量编排层：429 / rate limit / timeout 文案 / wire `retryable===true`。
/// 若已 `transport_retries_exhausted` 且非 rate_limit，返回 false（防放大）。
pub fn is_batch_orchestrator_retryable(msg: &str) -> bool {
    if msg.contains(TRANSPORT_RETRIES_EXHAUSTED_MARKER) && !is_rate_limit_message(msg) {
        return false;
    }
    if wire_retryable(msg) {
        return true;
    }
    let lower = msg.to_ascii_lowercase();
    lower.contains("rate limit")
        || lower.contains("rate_limit")
        || lower.contains("429")
        || lower.contains("schema")
}

fn is_rate_limit_message(msg: &str) -> bool {
    let lower = msg.to_ascii_lowercase();
    lower.contains("429")
        || lower.contains("rate limit")
        || lower.contains("rate_limit")
        || lower.contains("rate_limited")
}

fn wire_retryable(msg: &str) -> bool {
    let Some(wire) = provider_error::extract_provider_wire_from_err(msg) else {
        return false;
    };
    let Some(json) = wire.strip_prefix(USER_FACING_API_ERROR_PREFIX) else {
        return false;
    };
    serde_json::from_str::<UserFacingApiError>(json.trim())
        .ok()
        .is_some_and(|e| e.retryable)
}

/// 为 body 读失败追加排查提示（不触发 POST 重试）。
pub fn append_body_read_transport_hint(msg: &str) -> String {
    if !is_body_read_transport_hint(msg) {
        return msg.to_string();
    }
    let mut out = msg.to_string();
    if !out.contains("【排查】") {
        out.push_str(
            "\n\n【排查】常见于长耗时或大体积响应。若仍失败：在设置中提高超时秒数、关闭系统代理或换网络；\
             部分网关对 HTTP/2 不稳定时可避免在中间层启用 HTTP/2。",
        );
    }
    out
}

/// 将 transport 失败装饰为 `__BGX_ERR_JSON__` 线格式。
pub fn decorate_network_transient_wire(
    api_type: &str,
    provider_kind: &str,
    raw: String,
    category: &str,
    user_hint: &str,
    retryable: bool,
    transport_retries_exhausted: Option<u32>,
) -> String {
    let mut message = raw;
    if let Some(n) = transport_retries_exhausted {
        message.push_str(&format!(" {TRANSPORT_RETRIES_EXHAUSTED_MARKER}{n}"));
    }
    encode_user_facing_api_error(&UserFacingApiError {
        api_type: api_type.to_string(),
        provider: provider_kind.to_string(),
        category: category.to_string(),
        message: message.clone(),
        code: None,
        request_id: None,
        retryable,
        user_hint: user_hint.to_string(),
        params: None,
    })
}

#[cfg(test)]
fn decorate_connect_exhausted_wire(
    api_type: &str,
    provider_kind: &str,
    raw: String,
    attempts: u32,
) -> String {
    decorate_network_transient_wire(
        api_type,
        provider_kind,
        raw,
        "network_transient",
        "",
        true,
        Some(attempts),
    )
}

#[cfg(test)]
fn decorate_ambiguous_transport_wire(api_type: &str, provider_kind: &str, raw: String) -> String {
    decorate_network_transient_wire(
        api_type,
        provider_kind,
        raw,
        "network_ambiguous",
        "",
        true,
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_read_hint_true_for_broken_pipe() {
        assert!(is_body_read_transport_hint(
            "error reading a body: broken pipe"
        ));
    }

    #[test]
    fn batch_skips_when_transport_exhausted_non_rate_limit() {
        let msg = format!(
            "{USER_FACING_API_ERROR_PREFIX}{{\"retryable\":true,\"category\":\"network_transient\"}} transport_retries_exhausted=3"
        );
        assert!(!is_batch_orchestrator_retryable(&msg));
    }

    #[test]
    fn batch_allows_rate_limit_with_exhausted_marker() {
        let msg = "HTTP 429: rate limit transport_retries_exhausted=3";
        assert!(is_batch_orchestrator_retryable(&msg));
    }

    #[test]
    fn batch_honors_wire_retryable() {
        let wire = encode_user_facing_api_error(&UserFacingApiError {
            api_type: "text".into(),
            provider: "text_deepseek_compat".into(),
            category: "rate_limited".into(),
            message: "429".into(),
            code: None,
            request_id: None,
            retryable: true,
            user_hint: "稍后再试".into(),
            params: None,
        });
        assert!(is_batch_orchestrator_retryable(&wire));
    }

    #[test]
    fn decorate_connect_exhausted_emits_wire() {
        let out = decorate_connect_exhausted_wire(
            "text",
            "text_deepseek_compat",
            "connect error".into(),
            3,
        );
        assert!(out.starts_with(USER_FACING_API_ERROR_PREFIX));
        assert!(out.contains(TRANSPORT_RETRIES_EXHAUSTED_MARKER));
        assert!(out.contains("network_transient"));
    }

    #[test]
    fn decorate_ambiguous_emits_wire() {
        let out =
            decorate_ambiguous_transport_wire("text", "text_deepseek_compat", "broken pipe".into());
        assert!(out.starts_with(USER_FACING_API_ERROR_PREFIX));
        assert!(out.contains("network_ambiguous"));
    }
}
