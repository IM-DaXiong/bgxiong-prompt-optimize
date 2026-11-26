//! OpenAI-compatible chat/completions URL construction (explicit rules, no heuristics).

use super::shared::TextServiceConfig;

fn trim_base(base: &str) -> String {
    base.trim().trim_end_matches('/').to_string()
}

/// Build a chat/completions URL for the OpenAI-compatible family.
pub fn build_openai_compat_chat_url(provider_kind: &str, base_url: &str) -> Result<String, String> {
    let _ = provider_kind;
    let raw = trim_base(base_url);
    if raw.is_empty() {
        return Err("未配置 base_url".to_string());
    }

    if raw.to_ascii_lowercase().contains("/apps/anthropic") {
        return Err(
            "当前为 OpenAI 兼容（chat/completions），不能使用 …/apps/anthropic 地址。\
             百炼 Coding Plan 请填：https://coding.dashscope.aliyuncs.com/v1（与本应用文档主推大陆 endpoint 一致；密钥须与该地址配套）；\
             百炼按量请填：https://dashscope.aliyuncs.com/compatible-mode/v1。\
             密钥须与地址配套：Coding Plan 用控制台「Coding Plan」页的 sk-sp- 密钥，按量用普通 sk- 密钥。"
                .to_string(),
        );
    }

    if raw.contains("chat/completions") {
        return Ok(raw);
    }

    if raw.ends_with("/v1") || raw.ends_with("/api/v3") {
        Ok(format!("{raw}/chat/completions"))
    } else {
        Ok(format!("{raw}/v1/chat/completions"))
    }
}

/// DashScope native text-generation URL. Prefer [`crate::qwen::qwen_native_http_url`]
/// for model-aware routing (qwen3.8-max uses multimodal-generation).
pub fn build_qwen_chat_url(base_url: &str) -> String {
    let trimmed = base_url.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return "https://dashscope.aliyuncs.com/api/v1/services/aigc/text-generation/generation"
            .to_string();
    }
    format!("{trimmed}/api/v1/services/aigc/text-generation/generation")
}

pub fn build_chat_url_by_kind(
    service: &TextServiceConfig,
    provider_kind: &str,
) -> Result<String, String> {
    let base = service.base_url.as_deref().unwrap_or("");
    match provider_kind {
        "text_qwen" => Ok(build_qwen_chat_url(base)),
        "text_local_model" => {
            Err("本地模型 URL 由 llama-server 自动管理，不走 HTTP 构建".to_string())
        }
        _ => build_openai_compat_chat_url(provider_kind, base),
    }
}

pub fn resolved_chat_completions_url(service: &TextServiceConfig) -> Result<String, String> {
    build_chat_url_by_kind(service, service.provider_kind())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn svc_with_kind(base: &str, kind: &str) -> TextServiceConfig {
        TextServiceConfig {
            base_url: Some(base.to_string()),
            provider: Some(kind.to_string()),
            ..Default::default()
        }
    }

    fn svc(base: &str) -> TextServiceConfig {
        TextServiceConfig {
            base_url: Some(base.to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn ark_volces_base_chat_path() {
        assert_eq!(
            build_openai_compat_chat_url(
                "text_volc_ark_compat",
                "https://ark.cn-beijing.volces.com/api/v3"
            )
            .unwrap(),
            "https://ark.cn-beijing.volces.com/api/v3/chat/completions"
        );
        assert_eq!(
            build_openai_compat_chat_url(
                "text_volc_ark_compat",
                "https://ark.cn-beijing.volces.com/api/v3/"
            )
            .unwrap(),
            "https://ark.cn-beijing.volces.com/api/v3/chat/completions"
        );
    }

    #[test]
    fn openai_style_v1_base() {
        assert_eq!(
            build_openai_compat_chat_url("text_openai_compat", "https://api.openai.com/v1")
                .unwrap(),
            "https://api.openai.com/v1/chat/completions"
        );
    }

    #[test]
    fn bailian_coding_plan_openai_base() {
        assert_eq!(
            build_openai_compat_chat_url(
                "text_openai_compat",
                "https://coding.dashscope.aliyuncs.com/v1"
            )
            .unwrap(),
            "https://coding.dashscope.aliyuncs.com/v1/chat/completions"
        );
        assert_eq!(
            build_openai_compat_chat_url(
                "text_openai_compat",
                "https://coding.dashscope.aliyuncs.com"
            )
            .unwrap(),
            "https://coding.dashscope.aliyuncs.com/v1/chat/completions"
        );
    }

    #[test]
    fn bailian_coding_plan_anthropic_base_rejected() {
        let err = build_openai_compat_chat_url(
            "text_openai_compat",
            "https://coding.dashscope.aliyuncs.com/apps/anthropic",
        )
        .unwrap_err();
        assert!(err.contains("OpenAI"), "{err}");
    }

    #[test]
    fn generic_gateway_gets_v1_segment() {
        assert_eq!(
            build_openai_compat_chat_url("text_openai_compat", "https://api.example.com").unwrap(),
            "https://api.example.com/v1/chat/completions"
        );
    }

    #[test]
    fn deepseek_root_base_gets_v1_chat_completions() {
        assert_eq!(
            build_openai_compat_chat_url("text_deepseek_compat", "https://api.deepseek.com")
                .unwrap(),
            "https://api.deepseek.com/v1/chat/completions"
        );
        assert_eq!(
            build_openai_compat_chat_url("text_deepseek_compat", "https://api.deepseek.com/v1")
                .unwrap(),
            "https://api.deepseek.com/v1/chat/completions"
        );
    }

    #[test]
    fn resolved_url_with_provider_kind() {
        let svc = svc_with_kind("https://api.deepseek.com", "text_deepseek_compat");
        let url = resolved_chat_completions_url(&svc).unwrap();
        assert!(url.ends_with("/v1/chat/completions"), "{url}");
    }

    #[test]
    fn resolved_url_defaults_to_openai_compat() {
        let svc = svc("https://api.openai.com");
        let url = resolved_chat_completions_url(&svc).unwrap();
        assert!(url.ends_with("/v1/chat/completions"), "{url}");
    }

    #[test]
    fn dispatch_to_local_model_rejected() {
        let svc = svc_with_kind("", "text_local_model");
        assert!(build_chat_url_by_kind(&svc, "text_local_model").is_err());
    }
}
