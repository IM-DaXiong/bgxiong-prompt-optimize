//! Map returned text-exit errors without inventing a second wire format.
//!
//! Crate decoration is currently `HTTP {status}: {body}`. Full host
//! `__BGX_ERR_JSON__` mapping lives in `provider_error` and cannot be linked
//! from this shell (it lives in the main program crate). When a payload already
//! carries that prefix, unwrap the user-facing message; otherwise keep the real
//! status and body instead of replacing them with a generic failure.

use bgx_prompt_optimize::OptimizeError;

use crate::error_ui;

const USER_FACING_API_ERROR_PREFIX: &str = "__BGX_ERR_JSON__";

pub fn map_exit_error(message: String) -> OptimizeError {
    let lower = message.to_ascii_lowercase();
    if message.contains("已取消")
        || message.contains("任务已取消")
        || lower.contains("cancelled")
        || lower.contains("canceled")
    {
        return error_ui::cancelled();
    }
    if message.contains("超时") || lower.contains("timeout") || lower.contains("timed out") {
        return error_ui::timeout();
    }
    if let Some(unwrapped) = unwrap_bgx_err_json(&message) {
        return error_ui::err("TEXT_HTTP", unwrapped);
    }
    error_ui::err("TEXT_HTTP", message)
}

fn unwrap_bgx_err_json(message: &str) -> Option<String> {
    let start = message.find(USER_FACING_API_ERROR_PREFIX)?;
    let json = message[start + USER_FACING_API_ERROR_PREFIX.len()..].trim();
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    if let Some(msg) = value.get("message").and_then(|v| v.as_str()) {
        let msg = msg.trim();
        if !msg.is_empty() {
            return Some(msg.to_string());
        }
    }
    // Keep the original payload if JSON has no message field.
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_http_status_and_body() {
        let err = map_exit_error("HTTP 401: {\"error\":{\"message\":\"Incorrect API key\"}}".into());
        assert_eq!(err.code, "TEXT_HTTP");
        assert!(err.message.contains("401"), "{}", err.message);
        assert!(err.message.contains("Incorrect API key"), "{}", err.message);
    }

    #[test]
    fn unwraps_existing_bgx_err_json() {
        let raw = format!(
            "{USER_FACING_API_ERROR_PREFIX}{}",
            r#"{"message":"密钥无效","category":"auth_failed"}"#
        );
        let err = map_exit_error(raw);
        assert_eq!(err.message, "密钥无效");
    }
}
