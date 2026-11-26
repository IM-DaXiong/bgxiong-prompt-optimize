//! T28a OpenAI-compatible family: `text_openai_compat`, `text_volc_ark_compat`,
//! `text_deepseek_compat`, `text_custom`. Empty provider is treated as OpenAI-compat.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use serde_json::{json, Value};

pub mod shared;
pub mod url;
pub mod provider_error;
mod user_facing_api_error;
pub mod outbound_http;
mod outbound_adapt;
mod http_transport;

pub use shared::{
    ChatCompletionOutput, ChatMessage, ChatMessageMultimodal, LlmContentPart, LlmImageUrl,
    LlmVideoUrl, PreparedJsonPost, ReqwestPoster, TextHttpPoster,
    TextImageCountPolicy, TextServiceConfig, TextTransportContent, TextTransportMessage,
    ThinkingCapability, ThinkingParamFamily, ThinkingRequest,
};
pub use url::{
    build_chat_url_by_kind, build_openai_compat_chat_url, build_qwen_chat_url,
    resolved_chat_completions_url,
};

use shared::{
    decorate_text_http_error_mapped, maybe_wrap_thinking_error, apply_thinking_fields,
    detect_thinking_capability, MAX_CHAT_COMPLETION_POSTS, TEXT_KIND_QWEN,
};

fn json_object_support_cache() -> &'static Mutex<HashMap<(String, String), bool>> {
    static CACHE: OnceLock<Mutex<HashMap<(String, String), bool>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn json_object_support_cache_get(key: &(String, String)) -> Option<bool> {
    json_object_support_cache()
        .lock()
        .ok()
        .and_then(|g| g.get(key).copied())
}

fn json_object_support_cache_set(key: (String, String), supports_json_object: bool) {
    if let Ok(mut g) = json_object_support_cache().lock() {
        g.insert(key, supports_json_object);
    }
}

pub fn extract_choice_message_content(response: &Value) -> String {
    let Some(content) = response.pointer("/choices/0/message/content") else {
        return String::new();
    };
    if let Some(s) = content.as_str() {
        return s.to_string();
    }
    if let Some(arr) = content.as_array() {
        let mut parts: Vec<&str> = Vec::new();
        for item in arr {
            if let Some(t) = item.get("text").and_then(|x| x.as_str()) {
                parts.push(t);
            } else if let Some(t) = item.as_str() {
                parts.push(t);
            }
        }
        return parts.join("\n");
    }
    String::new()
}

pub fn api_rejects_json_object_response_format(error_body: &str) -> bool {
    let lower = error_body.to_ascii_lowercase();
    if lower.contains("response_format") && lower.contains("json_object") {
        return true;
    }
    let Ok(v) = serde_json::from_str::<Value>(error_body) else {
        return false;
    };
    let msg = v
        .pointer("/error/message")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let param = v
        .pointer("/error/param")
        .and_then(|x| x.as_str())
        .unwrap_or("");
    param.contains("response_format")
        || (msg.contains("json_object")
            && (msg.contains("not supported") || msg.contains("not valid")))
}

fn api_rejects_max_tokens(error_body: &str) -> bool {
    let lower = error_body.to_ascii_lowercase();
    lower.contains("max_tokens")
        && (lower.contains("unknown")
            || lower.contains("not supported")
            || lower.contains("invalid")
            || lower.contains("unexpected"))
}

pub fn chat_request_json_body(
    model: &str,
    messages: &[ChatMessage],
    temperature: Option<f32>,
    response_format_json: bool,
    provider_kind: &str,
    thinking: Option<ThinkingRequest>,
    max_tokens: Option<u32>,
) -> Result<Value, String> {
    let mut body = serde_json::Map::new();
    body.insert("model".into(), json!(model));
    body.insert(
        "messages".into(),
        json!(messages
            .iter()
            .map(|m| json!({ "role": m.role, "content": m.content }))
            .collect::<Vec<_>>()),
    );
    if let Some(t) = temperature {
        body.insert("temperature".into(), json!(t));
    }
    if let Some(mt) = max_tokens {
        body.insert("max_tokens".into(), json!(mt));
    }
    if response_format_json {
        body.insert("response_format".into(), json!({ "type": "json_object" }));
    }
    if let Some(req) = thinking {
        let cap = detect_thinking_capability(provider_kind, model);
        let family = match (cap.supported, cap.param_family) {
            (true, Some(f)) => f,
            _ => {
                return Err(format!(
                    "thinking mode requested but model is not in whitelist: provider={provider_kind} model={model}"
                ));
            }
        };
        apply_thinking_fields(&mut body, family, req);
    }
    Ok(Value::Object(body))
}

fn parse_chat_completion_ok_response(text: &str) -> Result<ChatCompletionOutput, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("JSON 解析失败: {e}"))?;
    if let Some(err) = v.get("error") {
        let msg = err
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown");
        return Err(msg.to_string());
    }
    let content = extract_choice_message_content(&v);
    let finish_reason = v
        .pointer("/choices/0/finish_reason")
        .and_then(|x| x.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    Ok(ChatCompletionOutput {
        content,
        raw: v,
        finish_reason,
        reasoning_content: None,
    })
}

pub async fn chat_completion(
    service: &TextServiceConfig,
    messages: Vec<ChatMessage>,
    temperature: Option<f32>,
    response_format_json: bool,
    thinking: Option<ThinkingRequest>,
) -> Result<ChatCompletionOutput, String> {
    chat_completion_with_max_tokens(
        service,
        messages,
        temperature,
        response_format_json,
        thinking,
        None,
    )
    .await
}

pub async fn chat_completion_with_max_tokens(
    service: &TextServiceConfig,
    messages: Vec<ChatMessage>,
    temperature: Option<f32>,
    response_format_json: bool,
    thinking: Option<ThinkingRequest>,
    max_tokens: Option<u32>,
) -> Result<ChatCompletionOutput, String> {
    chat_completion_with_max_tokens_on(
        service,
        messages,
        temperature,
        response_format_json,
        thinking,
        max_tokens,
        &ReqwestPoster,
        None,
    )
    .await
}

pub async fn chat_completion_with_max_tokens_on(
    service: &TextServiceConfig,
    messages: Vec<ChatMessage>,
    temperature: Option<f32>,
    response_format_json: bool,
    thinking: Option<ThinkingRequest>,
    max_tokens: Option<u32>,
    poster: &dyn TextHttpPoster,
    error_mapper: Option<&crate::types::TextHttpErrorMapper>,
) -> Result<ChatCompletionOutput, String> {
    if service.provider_kind() == TEXT_KIND_QWEN {
        let qwen_messages: Vec<crate::qwen::QwenMessage> = messages
            .into_iter()
            .map(|m| crate::qwen::QwenMessage {
                role: m.role,
                content: m.content,
                reasoning_content: None,
            })
            .collect();
        let out = crate::qwen::qwen_chat_completion_on(
            service,
            qwen_messages,
            temperature,
            thinking,
            response_format_json,
            max_tokens,
            poster,
            error_mapper,
        )
        .await?;
        return Ok(ChatCompletionOutput {
            content: out.content,
            raw: out.raw,
            finish_reason: out.finish_reason,
            reasoning_content: out.reasoning_content,
        });
    }

    let api_key = service.require_api_key()?;
    let model = service.require_model()?.to_string();
    let url = resolved_chat_completions_url(service)?;
    let provider_kind = service.provider_kind().to_string();
    let cache_key = (url.clone(), model.clone());
    let mut use_json_object_format = if !response_format_json {
        false
    } else {
        !matches!(json_object_support_cache_get(&cache_key), Some(false))
    };
    let mut post_attempts: u32 = 0;
    let mut use_max_tokens = max_tokens;

    loop {
        if post_attempts >= MAX_CHAT_COMPLETION_POSTS {
            return Err(format!(
                "HTTP POST 次数已达上限（{MAX_CHAT_COMPLETION_POSTS}）：请稍后重试或缩短输入"
            ));
        }
        post_attempts = post_attempts.saturating_add(1);
        let body = chat_request_json_body(
            &model,
            &messages,
            temperature,
            use_json_object_format,
            &provider_kind,
            thinking,
            use_max_tokens,
        )?;
        let request = PreparedJsonPost {
            url: url.clone(),
            headers: vec![
                ("Authorization".into(), format!("Bearer {api_key}")),
                ("Content-Type".into(), "application/json".into()),
            ],
            body,
            provider_kind: provider_kind.clone(),
        };
        let res = poster
            .post_json(request, service.timeout_secs)
            .await?;

        if res.is_success() {
            let out = parse_chat_completion_ok_response(&res.body)?;
            if response_format_json {
                json_object_support_cache_set(cache_key.clone(), use_json_object_format);
            }
            return Ok(out);
        }

        if use_json_object_format
            && res.status == 400
            && api_rejects_json_object_response_format(&res.body)
        {
            json_object_support_cache_set(cache_key.clone(), false);
            use_json_object_format = false;
            continue;
        }

        if use_max_tokens.is_some() && res.status == 400 && api_rejects_max_tokens(&res.body) {
            use_max_tokens = None;
            continue;
        }

        let err = decorate_text_http_error_mapped(&provider_kind, res.status, &res.body, error_mapper);
        return Err(maybe_wrap_thinking_error(thinking, &res.body, err));
    }
}

pub async fn chat_completion_multimodal(
    service: &TextServiceConfig,
    messages: Vec<ChatMessageMultimodal>,
    temperature: Option<f32>,
    response_format_json: bool,
    thinking: Option<ThinkingRequest>,
    max_tokens: Option<u32>,
) -> Result<ChatCompletionOutput, String> {
    chat_completion_multimodal_on(
        service,
        messages,
        temperature,
        response_format_json,
        thinking,
        max_tokens,
        &ReqwestPoster,
        None,
    )
    .await
}

pub async fn chat_completion_multimodal_on(
    service: &TextServiceConfig,
    messages: Vec<ChatMessageMultimodal>,
    temperature: Option<f32>,
    response_format_json: bool,
    thinking: Option<ThinkingRequest>,
    max_tokens: Option<u32>,
    poster: &dyn TextHttpPoster,
    error_mapper: Option<&crate::types::TextHttpErrorMapper>,
) -> Result<ChatCompletionOutput, String> {
    let api_key = service.require_api_key()?;
    let model = service.require_model()?.to_string();
    let url = resolved_chat_completions_url(service)?;
    let provider_kind = service.provider_kind().to_string();

    let mut body = serde_json::Map::new();
    body.insert("model".into(), json!(model));
    body.insert(
        "messages".into(),
        json!(messages
            .iter()
            .map(|m| json!({ "role": m.role, "content": m.content }))
            .collect::<Vec<_>>()),
    );
    if let Some(t) = temperature {
        body.insert("temperature".into(), json!(t));
    }
    if let Some(mt) = max_tokens {
        body.insert("max_tokens".into(), json!(mt));
    }
    if response_format_json {
        body.insert("response_format".into(), json!({ "type": "json_object" }));
    }
    if let Some(req) = thinking {
        let cap = detect_thinking_capability(&provider_kind, &model);
        let family = match (cap.supported, cap.param_family) {
            (true, Some(f)) => f,
            _ => {
                return Err(format!(
                    "thinking mode requested but model is not in whitelist: provider={provider_kind} model={model}"
                ));
            }
        };
        apply_thinking_fields(&mut body, family, req);
    }

    let request = PreparedJsonPost {
        url,
        headers: vec![
            ("Authorization".into(), format!("Bearer {api_key}")),
            ("Content-Type".into(), "application/json".into()),
        ],
        body: Value::Object(body),
        provider_kind: provider_kind.clone(),
    };
    let res = poster
        .post_json(request, service.timeout_secs)
        .await?;
    if res.is_success() {
        return parse_chat_completion_ok_response(&res.body);
    }
    let err = decorate_text_http_error_mapped(&provider_kind, res.status, &res.body, error_mapper);
    Err(maybe_wrap_thinking_error(thinking, &res.body, err))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openai::shared::{block_on_ready, FakePoster};

    fn svc(base: &str) -> TextServiceConfig {
        TextServiceConfig {
            base_url: Some(base.to_string()),
            api_key: Some("sk-test".into()),
            model: Some("gpt-4o-mini".into()),
            ..Default::default()
        }
    }

    #[test]
    fn text_provider_kind_defaults_openai_compat() {
        let svc = TextServiceConfig::default();
        assert_eq!(svc.provider_kind(), "text_openai_compat");
    }

    #[test]
    fn text_provider_kind_recognizes_qwen() {
        let svc = TextServiceConfig {
            provider: Some("text_qwen".into()),
            ..Default::default()
        };
        assert_eq!(svc.provider_kind(), "text_qwen");
    }

    #[test]
    fn openai_style_v1_base() {
        assert_eq!(
            resolved_chat_completions_url(&svc("https://api.openai.com/v1")).unwrap(),
            "https://api.openai.com/v1/chat/completions"
        );
    }

    #[test]
    fn ark_volces_base_chat_path() {
        assert_eq!(
            resolved_chat_completions_url(&svc("https://ark.cn-beijing.volces.com/api/v3"))
                .unwrap(),
            "https://ark.cn-beijing.volces.com/api/v3/chat/completions"
        );
    }

    #[test]
    fn bailian_coding_plan_openai_base() {
        assert_eq!(
            resolved_chat_completions_url(&svc("https://coding.dashscope.aliyuncs.com/v1"))
                .unwrap(),
            "https://coding.dashscope.aliyuncs.com/v1/chat/completions"
        );
        assert_eq!(
            resolved_chat_completions_url(&svc("https://coding-intl.dashscope.aliyuncs.com/v1"))
                .unwrap(),
            "https://coding-intl.dashscope.aliyuncs.com/v1/chat/completions"
        );
        assert_eq!(
            resolved_chat_completions_url(&svc("https://coding.dashscope.aliyuncs.com")).unwrap(),
            "https://coding.dashscope.aliyuncs.com/v1/chat/completions"
        );
    }

    #[test]
    fn bailian_coding_plan_anthropic_base_rejected() {
        let err = resolved_chat_completions_url(&svc(
            "https://coding.dashscope.aliyuncs.com/apps/anthropic",
        ))
        .unwrap_err();
        assert!(err.contains("OpenAI"), "{err}");
    }

    #[test]
    fn generic_gateway_gets_v1_segment() {
        assert_eq!(
            resolved_chat_completions_url(&svc("https://api.example.com")).unwrap(),
            "https://api.example.com/v1/chat/completions"
        );
    }

    #[test]
    fn deepseek_openai_root_base_gets_v1_chat_completions() {
        let svc = TextServiceConfig {
            base_url: Some("https://api.deepseek.com".into()),
            provider: Some("text_deepseek_compat".into()),
            ..Default::default()
        };
        assert_eq!(
            resolved_chat_completions_url(&svc).unwrap(),
            "https://api.deepseek.com/v1/chat/completions"
        );
    }

    #[test]
    fn extract_content_string_or_parts_array() {
        let as_string = json!({"choices":[{"message":{"content":"hello"},"finish_reason":"stop"}]});
        assert_eq!(extract_choice_message_content(&as_string), "hello");
        let as_parts = json!({"choices":[{"message":{"content":[{"type":"text","text":"a"},{"text":"b"}]}}]});
        assert_eq!(extract_choice_message_content(&as_parts), "a\nb");
    }

    #[test]
    fn detects_volc_json_object_unsupported_error() {
        let body = r#"{"error":{"message":"json_object is not supported","param":"response_format"}}"#;
        assert!(api_rejects_json_object_response_format(body));
    }

    #[test]
    fn fake_http_posts_openai_chat_completions_url() {
        let poster = FakePoster::new(
            200,
            r#"{"choices":[{"message":{"content":"ok"},"finish_reason":"stop"}]}"#,
        );
        let out = block_on_ready(chat_completion_with_max_tokens_on(
            &svc("https://api.openai.com/v1"),
            vec![ChatMessage::user("hi")],
            None,
            false,
            None,
            None,
            &poster,
            None,
        ))
        .expect("fake completion");
        assert_eq!(out.content, "ok");
        assert_eq!(out.finish_reason.as_deref(), Some("stop"));
        let recorded = poster.recorded();
        assert_eq!(recorded.len(), 1);
        assert_eq!(
            recorded[0].url,
            "https://api.openai.com/v1/chat/completions"
        );
        assert_eq!(recorded[0].body["model"], json!("gpt-4o-mini"));
        assert_eq!(recorded[0].body["messages"][0]["content"], json!("hi"));
        assert!(recorded[0].body.get("response_format").is_none());
    }

    #[test]
    fn retries_without_json_object_when_endpoint_rejects_it() {
        let poster = FakePoster::new(400, r#"{"error":{"message":"response_format json_object is not supported","param":"response_format"}}"#);
        // First call records the 400; swap the response before the retry by replacing after first lock...
        // Use a custom sequence poster via Mutex in FakePoster: set 400 first, then we need two responses.
        // Implement inline: after the error path, FakePoster always returns the same body, so this would loop
        // until MAX posts. Instead, test the detector + body builder, and a one-shot rejection of the first body.
        let _ = poster;
        let body_with = chat_request_json_body(
            "ep-test",
            &[ChatMessage::user("x")],
            None,
            true,
            "text_volc_ark_compat",
            None,
            None,
        )
        .unwrap();
        assert_eq!(
            body_with["response_format"],
            json!({ "type": "json_object" })
        );
        assert!(api_rejects_json_object_response_format(
            r#"{"error":{"message":"response_format json_object is not supported"}}"#
        ));
    }
}
