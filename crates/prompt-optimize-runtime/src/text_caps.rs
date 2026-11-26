//! MiniMax official output cap resolution for the standalone shell.
//!
//! SSOT for catalog caps is `caps_json.maxOutputTokens` from the same model
//! directory used by `src-tauri/src/db/model_catalog_repo.rs`. The shell does
//! not open story.db or the main window, so it cannot call that cache directly.
//! When a legal no-DB catalog port exists, read it here first. Until then, only
//! a user-supplied official value in settings is accepted. Never invent 1024
//! or any other default budget.

use bgx_prompt_optimize::OptimizeError;
use bgx_text_http::TEXT_KIND_MINIMAX;

use crate::config::ShellConfig;
use crate::error_ui;

pub const TEXT_OUTPUT_CAP_UNREGISTERED: &str = "TEXT_OUTPUT_CAP_UNREGISTERED";

pub fn is_minimax_kind(kind: &str) -> bool {
    kind == "text_minimax_m3" || kind == TEXT_KIND_MINIMAX
}

/// OpenAI / Qwen / local / ordinary adapter may omit max_tokens (None).
/// MiniMax protocol requires a real official cap: catalog first, else settings.
pub fn resolve_max_tokens(config: &ShellConfig) -> Result<Option<u32>, OptimizeError> {
    let kind = config.provider_kind.trim();
    if !is_minimax_kind(kind) {
        return Ok(None);
    }
    if let Some(cap) = catalog_max_output_tokens(kind, config.model.as_deref()) {
        return Ok(Some(cap));
    }
    if let Some(cap) = config.official_max_output_tokens() {
        return Ok(Some(cap));
    }
    Err(error_ui::err(
        TEXT_OUTPUT_CAP_UNREGISTERED,
        "MiniMax 请求缺少官方最大输出上限（maxOutputTokens）。请在设置中填写目录已登记的值；不会编造默认上限。",
    ))
}

/// Reserved port for a future no-DB catalog snapshot. Returns None today
/// because this shell must not open the main program database.
pub fn catalog_max_output_tokens(_provider_kind: &str, _model: Option<&str>) -> Option<u32> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_minimax_omits_max_tokens() {
        let cfg = ShellConfig {
            provider_kind: "text_openai_compat".into(),
            base_url: Some("https://example.invalid/v1".into()),
            model: Some("demo".into()),
            api_key: Some("x".into()),
            ..ShellConfig::default()
        };
        assert_eq!(resolve_max_tokens(&cfg).unwrap(), None);
    }

    #[test]
    fn minimax_without_cap_is_unregistered() {
        let cfg = ShellConfig {
            provider_kind: "text_minimax_m3".into(),
            base_url: Some("https://example.invalid".into()),
            model: Some("MiniMax-M3".into()),
            api_key: Some("x".into()),
            ..ShellConfig::default()
        };
        let err = resolve_max_tokens(&cfg).expect_err("missing cap");
        assert_eq!(err.code, TEXT_OUTPUT_CAP_UNREGISTERED);
    }

    #[test]
    fn minimax_uses_settings_official_value() {
        let cfg = ShellConfig {
            provider_kind: "text_minimax_m3".into(),
            base_url: Some("https://example.invalid".into()),
            model: Some("MiniMax-M3".into()),
            api_key: Some("x".into()),
            max_output_tokens: Some(8192),
            ..ShellConfig::default()
        };
        assert_eq!(resolve_max_tokens(&cfg).unwrap(), Some(8192));
    }
}
