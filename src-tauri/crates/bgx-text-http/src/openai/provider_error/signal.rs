//! Parse vendor HTTP/JSON signals (status, code, error.type, request_id).

use serde_json::Value;

pub fn contains_any_case_insensitive(haystack: &str, needles: &[&str]) -> bool {
    let h = haystack.to_ascii_lowercase();
    needles.iter().any(|n| h.contains(&n.to_ascii_lowercase()))
}

pub fn normalize_symbolic_token(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

pub fn contains_any_normalized_token(haystack: &str, needles: &[&str]) -> bool {
    let h = normalize_symbolic_token(haystack);
    needles
        .iter()
        .any(|n| h.contains(&normalize_symbolic_token(n)))
}

pub fn extract_request_id_from_text(s: &str) -> Option<String> {
    for key in ["\"request_id\":\"", "\"Request id: ", "Request id: "] {
        if let Some(idx) = s.find(key) {
            let tail = &s[idx + key.len()..];
            let end = tail.find('"').unwrap_or(tail.len().min(96));
            let rid = tail[..end].trim();
            if !rid.is_empty() {
                return Some(rid.to_string());
            }
        }
    }
    None
}

pub fn extract_numeric_code_from_text(s: &str) -> Option<i64> {
    if let Some(code) = extract_json_numeric_code(s) {
        return Some(code);
    }
    extract_paren_code(s)
}

fn extract_json_numeric_code(s: &str) -> Option<i64> {
    let key = "\"code\":";
    let idx = s.find(key)?;
    let tail = &s[idx + key.len()..];
    let digits: String = tail
        .chars()
        .skip_while(|c| c.is_whitespace())
        .take_while(|c| c.is_ascii_digit() || *c == '-')
        .collect();
    digits.parse().ok()
}

/// MiniMax-style `insufficient balance (1008)`.
fn extract_paren_code(s: &str) -> Option<i64> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'(' {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            if j > i + 1 && j < bytes.len() && bytes[j] == b')' {
                if let Ok(n) = s[i + 1..j].parse::<i64>() {
                    return Some(n);
                }
            }
        }
        i += 1;
    }
    None
}

/// HTTP status from `HTTP 402` anywhere (including `Context-IR HTTP 402 Payment Required`).
pub fn extract_http_status_prefix(s: &str) -> Option<String> {
    let idx = s.find("HTTP ")?;
    let rest = &s[idx + 5..];
    let token: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if token.is_empty() {
        None
    } else {
        Some(token)
    }
}

pub fn extract_vendor_error_type(s: &str) -> Option<String> {
    let start = s.find('{')?;
    let json = &s[start..];
    let end = json.rfind('}')?;
    let v: Value = serde_json::from_str(&json[..=end]).ok()?;
    v.pointer("/error/type")
        .and_then(|t| t.as_str())
        .filter(|t| !t.is_empty() && *t != "error")
        .map(|t| t.to_string())
}

pub fn extract_key_usize(raw: &str, key: &str) -> Option<usize> {
    let idx = raw.find(key)?;
    let tail = &raw[idx + key.len()..];
    let digits: String = tail.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_status_from_context_ir_prefix() {
        let raw = r#"Context-IR HTTP 402 Payment Required: {"http_code":"402"}"#;
        assert_eq!(extract_http_status_prefix(raw).as_deref(), Some("402"));
    }

    #[test]
    fn paren_code_1008() {
        assert_eq!(
            extract_numeric_code_from_text("insufficient balance (1008)"),
            Some(1008)
        );
    }

    #[test]
    fn error_type_from_minimax_body() {
        let raw = r#"Context-IR HTTP 402 Payment Required: {"type":"error","error":{"type":"insufficient_balance_error","message":"insufficient balance (1008)"}}"#;
        assert_eq!(
            extract_vendor_error_type(raw).as_deref(),
            Some("insufficient_balance_error")
        );
    }
}
