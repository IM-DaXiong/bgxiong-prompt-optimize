//! Shared existing user-facing error wire contract, without transport ownership.
use serde::Serialize;
pub const USER_FACING_API_ERROR_PREFIX: &str = "__BGX_ERR_JSON__";

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserFacingApiError {
    pub api_type: String,
    pub provider: String,
    pub category: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    pub retryable: bool,
    pub user_hint: String,
    /// Optional interpolation / diagnostics payload for frontend i18n (`Locale.errors[code]`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

pub fn encode_user_facing_api_error(err: &UserFacingApiError) -> String {
    match serde_json::to_string(err) {
        Ok(s) => format!("{USER_FACING_API_ERROR_PREFIX}{s}"),
        Err(_) => format!("{}\n\n【提示】{}", err.message, err.user_hint),
    }
}

/// Extract the non-empty `userHint` from a `__BGX_ERR_JSON__` wire.
pub fn user_hint_from_provider_error_wire(s: &str) -> Option<String> {
    let json = s.strip_prefix(USER_FACING_API_ERROR_PREFIX)?.trim();
    if json.is_empty() {
        return None;
    }
    serde_json::from_str::<UserFacingApiError>(json)
        .ok()
        .map(|e| e.user_hint)
        .filter(|h| !h.trim().is_empty())
}
