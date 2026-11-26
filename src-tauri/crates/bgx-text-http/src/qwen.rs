//! T28b DashScope native Qwen text. Vision stays `{image}` / `{text}`, never OpenAI `image_url`.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::openai::shared::{
    apply_thinking_to_qwen_parameters, decorate_text_http_error_mapped, maybe_wrap_thinking_error,
    normalize_api_key_for_bearer, LlmContentPart, PreparedJsonPost, ReqwestPoster,
    TextHttpPoster, TextImageCountPolicy, TextServiceConfig, TextTransportContent, ThinkingRequest,
};

const DEFAULT_DASHSCOPE_HTTP_BASE: &str = "https://dashscope.aliyuncs.com";
const API_V1_SUFFIX: &str = "/api/v1";
const COMPATIBLE_MODE_V1_SUFFIX: &str = "/compatible-mode/v1";
const AIGC_SERVICE_PREFIX: &str = "/api/v1/services/aigc/";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QwenMessage {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub reasoning_content: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QwenOutput {
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
    #[serde(default)]
    pub raw: Value,
}

pub const QWEN_DEFAULT_BASE_URL: &str = DEFAULT_DASHSCOPE_HTTP_BASE;

pub fn is_qwen38_max(model: &str) -> bool {
    model.trim().eq_ignore_ascii_case("qwen3.8-max")
}

pub fn qwen_vision_unsupported_err(model: &str) -> String {
    format!("通义千问模型 `{model}` 不支持 vision 输入；请改用 qwen3.8-max（INV-QWEN38-VISION-02）")
}

fn strip_known_dashscope_suffixes(base: &str, strip_compatible_mode: bool) -> String {
    let mut out = base.trim().trim_end_matches('/').to_string();
    if out.is_empty() {
        return out;
    }
    let lower = out.to_ascii_lowercase();
    if let Some(idx) = lower.find(AIGC_SERVICE_PREFIX) {
        out.truncate(idx);
        out = out.trim_end_matches('/').to_string();
    } else if strip_compatible_mode && out.ends_with(COMPATIBLE_MODE_V1_SUFFIX) {
        out.truncate(out.len() - COMPATIBLE_MODE_V1_SUFFIX.len());
        out = out.trim_end_matches('/').to_string();
    } else if out.ends_with(API_V1_SUFFIX) {
        out.truncate(out.len() - API_V1_SUFFIX.len());
        out = out.trim_end_matches('/').to_string();
    }
    out
}

pub fn normalize_dashscope_native_http_base(raw: &str) -> String {
    let original = raw.trim().trim_end_matches('/').to_string();
    if original.is_empty() {
        return original;
    }
    strip_known_dashscope_suffixes(&original, true)
}

pub fn resolve_dashscope_native_http_base(raw: Option<&str>) -> String {
    raw.map(str::trim)
        .filter(|s| !s.is_empty())
        .map(normalize_dashscope_native_http_base)
        .unwrap_or_else(|| DEFAULT_DASHSCOPE_HTTP_BASE.to_string())
}

fn qwen_api_url(base: &str) -> String {
    let trimmed = base.trim();
    if trimmed.is_empty() {
        return format!("{QWEN_DEFAULT_BASE_URL}/api/v1/services/aigc/text-generation/generation");
    }
    let normalized = normalize_dashscope_native_http_base(trimmed);
    format!("{normalized}/api/v1/services/aigc/text-generation/generation")
}

fn qwen_multimodal_api_url(base: &str) -> String {
    let trimmed = base.trim();
    if trimmed.is_empty() {
        return format!(
            "{QWEN_DEFAULT_BASE_URL}/api/v1/services/aigc/multimodal-generation/generation"
        );
    }
    let normalized = normalize_dashscope_native_http_base(trimmed);
    format!("{normalized}/api/v1/services/aigc/multimodal-generation/generation")
}

/// qwen3.8-max always uses multimodal-generation, including text-only calls.
pub fn qwen_native_http_url(base: &str, model: &str) -> String {
    if is_qwen38_max(model) {
        qwen_multimodal_api_url(base)
    } else {
        qwen_api_url(base)
    }
}

fn qwen_hybrid_thinking_model(model: &str) -> bool {
    let m = model.trim().to_ascii_lowercase();
    m.starts_with("qwen3") || m.contains("qwen3.")
}

fn clamp_temperature_for_model(model: &str, temperature: Option<f32>) -> Option<f32> {
    let t = temperature?;
    if is_qwen38_max(model) && t < 0.6 {
        Some(0.6)
    } else {
        Some(t)
    }
}

fn json_temperature(t: f32) -> Value {
    json!((f64::from(t) * 1_000.0).round() / 1_000.0)
}

fn serialize_qwen_message(m: &QwenMessage) -> Value {
    let mut obj = serde_json::Map::new();
    obj.insert("role".into(), json!(m.role));
    obj.insert("content".into(), json!(m.content));
    if let Some(ref r) = m.reasoning_content {
        let trimmed = r.trim();
        if !trimmed.is_empty() {
            obj.insert("reasoning_content".into(), json!(trimmed));
        }
    }
    Value::Object(obj)
}

fn apply_thinking_parameters(
    model: &str,
    parameters: &mut serde_json::Map<String, Value>,
    thinking: Option<ThinkingRequest>,
) {
    match thinking {
        Some(req) => {
            apply_thinking_to_qwen_parameters(parameters, req);
            if is_qwen38_max(model) && req.enabled {
                parameters.insert("preserve_thinking".into(), json!(true));
            }
        }
        None => {
            if qwen_hybrid_thinking_model(model) {
                parameters.insert("enable_thinking".into(), json!(false));
            }
        }
    }
}

pub fn qwen_request_body(
    model: &str,
    messages: &[QwenMessage],
    temperature: Option<f32>,
    thinking: Option<ThinkingRequest>,
    response_format_json: bool,
    max_tokens: Option<u32>,
) -> Value {
    let mut input = serde_json::Map::new();
    let msgs: Vec<Value> = messages.iter().map(serialize_qwen_message).collect();
    input.insert("messages".into(), json!(msgs));

    let mut parameters = serde_json::Map::new();
    if let Some(t) = clamp_temperature_for_model(model, temperature) {
        parameters.insert("temperature".into(), json_temperature(t));
    }
    parameters.insert("result_format".into(), json!("message"));
    if let Some(mt) = max_tokens {
        parameters.insert("max_tokens".into(), json!(mt));
    }
    // Native DashScope has no `response_format.type=json_object`. The flag is accepted
    // so callers can keep the host contract; JSON shape stays prompt-constrained.
    let _ = response_format_json;
    apply_thinking_parameters(model, &mut parameters, thinking);

    json!({
        "model": model,
        "input": input,
        "parameters": parameters,
    })
}

fn native_multimodal_content_parts(content: &TextTransportContent) -> Result<Vec<Value>, String> {
    match content {
        TextTransportContent::Text(t) => Ok(vec![json!({ "text": t })]),
        TextTransportContent::Parts(parts) => {
            let mut out = Vec::with_capacity(parts.len());
            for p in parts {
                match p {
                    LlmContentPart::Text { text } => {
                        out.push(json!({ "text": text }));
                    }
                    LlmContentPart::ImageUrl { image_url } => {
                        let url = image_url.url.trim();
                        if url.is_empty() {
                            return Err("通义千问识图：image url 为空".into());
                        }
                        out.push(json!({ "image": url }));
                    }
                    LlmContentPart::VideoUrl { .. } => {
                        return Err(
                            "通义千问本期仅支持图片输入，不支持视频入 LLM（INV-QWEN38-VISION-03）"
                                .into(),
                        );
                    }
                }
            }
            if out.is_empty() {
                return Err("通义千问识图：content parts 为空".into());
            }
            Ok(out)
        }
    }
}

pub fn qwen_multimodal_request_body(
    model: &str,
    messages: &[(String, TextTransportContent, Option<String>)],
    temperature: Option<f32>,
    thinking: Option<ThinkingRequest>,
    input_policy: TextImageCountPolicy,
    response_format_json: bool,
    max_tokens: Option<u32>,
) -> Result<Value, String> {
    let mut msgs = Vec::with_capacity(messages.len());
    for (role, content, reasoning) in messages {
        let parts = native_multimodal_content_parts(content)?;
        let mut obj = serde_json::Map::new();
        obj.insert("role".into(), json!(role));
        obj.insert("content".into(), json!(parts));
        if let Some(r) = reasoning {
            let trimmed = r.trim();
            if !trimmed.is_empty() {
                obj.insert("reasoning_content".into(), json!(trimmed));
            }
        }
        msgs.push(Value::Object(obj));
    }

    let mut input = serde_json::Map::new();
    input.insert("messages".into(), json!(msgs));

    let mut parameters = serde_json::Map::new();
    if input_policy == TextImageCountPolicy::AsSubmitted {
        if let Some(t) = temperature {
            parameters.insert("temperature".into(), json!(t));
        }
    } else if let Some(t) = clamp_temperature_for_model(model, temperature) {
        parameters.insert("temperature".into(), json_temperature(t));
    }
    parameters.insert("result_format".into(), json!("message"));
    if let Some(mt) = max_tokens {
        parameters.insert("max_tokens".into(), json!(mt));
    }
    let _ = response_format_json;
    apply_thinking_parameters(model, &mut parameters, thinking);

    Ok(json!({
        "model": model,
        "input": input,
        "parameters": parameters,
    }))
}

fn extract_qwen_content(response: &Value) -> String {
    if let Some(s) = response
        .pointer("/output/choices/0/message/content")
        .and_then(|v| v.as_str())
    {
        return s.to_string();
    }
    if let Some(arr) = response
        .pointer("/output/choices/0/message/content")
        .and_then(|v| v.as_array())
    {
        let mut parts = Vec::new();
        for item in arr {
            if let Some(t) = item.get("text").and_then(|x| x.as_str()) {
                parts.push(t);
            } else if let Some(t) = item.as_str() {
                parts.push(t);
            }
        }
        return parts.join("");
    }
    response
        .pointer("/output/text")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

fn extract_qwen_finish_reason(response: &Value) -> Option<String> {
    response
        .pointer("/output/choices/0/finish_reason")
        .or_else(|| response.pointer("/output/finish_reason"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

fn extract_qwen_reasoning_content(response: &Value) -> Option<String> {
    response
        .pointer("/output/choices/0/message/reasoning_content")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

fn parse_qwen_response(text: &str) -> Result<QwenOutput, String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("JSON 解析失败: {e}"))?;
    if let Some(code) = v.get("code").and_then(|c| c.as_str()) {
        if let Some(msg) = v.get("message").and_then(|m| m.as_str()) {
            return Err(format!("[{code}] {msg}"));
        }
    }
    let content = extract_qwen_content(&v);
    let reasoning_content = extract_qwen_reasoning_content(&v);
    let finish_reason = extract_qwen_finish_reason(&v);
    Ok(QwenOutput {
        content,
        reasoning_content,
        finish_reason,
        raw: v,
    })
}

fn final_prompt_from_messages(messages: &[QwenMessage]) -> String {
    messages
        .iter()
        .map(|m| {
            if let Some(ref r) = m.reasoning_content {
                if !r.trim().is_empty() {
                    return format!(
                        "[{}]\n{}\n[reasoning_content]\n{}",
                        m.role.trim(),
                        m.content,
                        r
                    );
                }
            }
            format!("[{}]\n{}", m.role.trim(), m.content)
        })
        .collect::<Vec<String>>()
        .join("\n\n")
}

async fn qwen_post_json_on(
    service: &TextServiceConfig,
    url: &str,
    body: &Value,
    thinking: Option<ThinkingRequest>,
    poster: &dyn TextHttpPoster,
    error_mapper: Option<&crate::types::TextHttpErrorMapper>,
) -> Result<QwenOutput, String> {
    let api_key = service
        .api_key
        .as_deref()
        .map(normalize_api_key_for_bearer)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "未配置 api_key".to_string())?;
    let _ = final_prompt_from_messages;
    let request = PreparedJsonPost {
        url: url.to_string(),
        headers: vec![
            ("Authorization".into(), format!("Bearer {api_key}")),
            ("Content-Type".into(), "application/json".into()),
        ],
        body: body.clone(),
        provider_kind: "text_qwen".into(),
    };
    let res = poster
        .post_json(request, service.timeout_secs)
        .await?;
    if res.is_success() {
        parse_qwen_response(&res.body)
    } else {
        let err = decorate_text_http_error_mapped("text_qwen", res.status, &res.body, error_mapper);
        Err(maybe_wrap_thinking_error(thinking, &res.body, err))
    }
}

pub async fn qwen_chat_completion(
    service: &TextServiceConfig,
    messages: Vec<QwenMessage>,
    temperature: Option<f32>,
    thinking: Option<ThinkingRequest>,
    response_format_json: bool,
    max_tokens: Option<u32>,
) -> Result<QwenOutput, String> {
    qwen_chat_completion_on(
        service,
        messages,
        temperature,
        thinking,
        response_format_json,
        max_tokens,
        &ReqwestPoster,
        None,
    )
    .await
}

pub async fn qwen_chat_completion_on(
    service: &TextServiceConfig,
    messages: Vec<QwenMessage>,
    temperature: Option<f32>,
    thinking: Option<ThinkingRequest>,
    response_format_json: bool,
    max_tokens: Option<u32>,
    poster: &dyn TextHttpPoster,
    error_mapper: Option<&crate::types::TextHttpErrorMapper>,
) -> Result<QwenOutput, String> {
    let model = service.require_model()?.to_string();
    if is_qwen38_max(&model) {
        let mm: Vec<(String, TextTransportContent, Option<String>)> = messages
            .into_iter()
            .map(|m| {
                (
                    m.role,
                    TextTransportContent::Text(m.content),
                    m.reasoning_content,
                )
            })
            .collect();
        return qwen_chat_completion_multimodal_on(
            service,
            mm,
            temperature,
            thinking,
            TextImageCountPolicy::ModelCapability,
            response_format_json,
            max_tokens,
            poster,
            error_mapper,
        )
        .await;
    }
    let base_url = resolve_dashscope_native_http_base(service.base_url.as_deref());
    let url = qwen_native_http_url(&base_url, &model);
    let body = qwen_request_body(
        &model,
        &messages,
        temperature,
        thinking,
        response_format_json,
        max_tokens,
    );
    qwen_post_json_on(service, &url, &body, thinking, poster, error_mapper).await
}

pub async fn qwen_chat_completion_multimodal(
    service: &TextServiceConfig,
    messages: Vec<(String, TextTransportContent, Option<String>)>,
    temperature: Option<f32>,
    thinking: Option<ThinkingRequest>,
    input_policy: TextImageCountPolicy,
    response_format_json: bool,
    max_tokens: Option<u32>,
) -> Result<QwenOutput, String> {
    qwen_chat_completion_multimodal_on(
        service,
        messages,
        temperature,
        thinking,
        input_policy,
        response_format_json,
        max_tokens,
        &ReqwestPoster,
        None,
    )
    .await
}

pub async fn qwen_chat_completion_multimodal_on(
    service: &TextServiceConfig,
    messages: Vec<(String, TextTransportContent, Option<String>)>,
    temperature: Option<f32>,
    thinking: Option<ThinkingRequest>,
    input_policy: TextImageCountPolicy,
    response_format_json: bool,
    max_tokens: Option<u32>,
    poster: &dyn TextHttpPoster,
    error_mapper: Option<&crate::types::TextHttpErrorMapper>,
) -> Result<QwenOutput, String> {
    let model = service.require_model()?.to_string();
    if input_policy != TextImageCountPolicy::AsSubmitted && !is_qwen38_max(&model) {
        return Err(qwen_vision_unsupported_err(&model));
    }
    let base_url = resolve_dashscope_native_http_base(service.base_url.as_deref());
    let url = qwen_multimodal_api_url(&base_url);
    let body = qwen_multimodal_request_body(
        &model,
        &messages,
        temperature,
        thinking,
        input_policy,
        response_format_json,
        max_tokens,
    )?;
    qwen_post_json_on(service, &url, &body, thinking, poster, error_mapper).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openai::shared::{block_on_ready, FakePoster, LlmImageUrl, LlmVideoUrl};

    fn svc(model: &str) -> TextServiceConfig {
        TextServiceConfig {
            api_key: Some("sk-test".into()),
            base_url: Some("https://dashscope.aliyuncs.com".into()),
            model: Some(model.into()),
            provider: Some("text_qwen".into()),
            ..Default::default()
        }
    }

    fn ok_body(content: &str) -> String {
        json!({
            "output": {
                "choices": [{
                    "finish_reason": "stop",
                    "message": { "content": content }
                }]
            }
        })
        .to_string()
    }

    #[test]
    fn test_is_qwen38_max_exact_id() {
        assert!(is_qwen38_max("qwen3.8-max"));
        assert!(is_qwen38_max("Qwen3.8-Max"));
        assert!(!is_qwen38_max("qwen3.7-max"));
        assert!(!is_qwen38_max("qwen3-max"));
        assert!(!is_qwen38_max(""));
        let err = qwen_vision_unsupported_err("qwen3.7-max");
        assert!(err.contains("INV-QWEN38-VISION-02"), "{err}");
        assert!(err.contains("qwen3.7-max"), "{err}");
    }

    #[test]
    fn test_qwen_api_url() {
        assert_eq!(
            qwen_api_url(""),
            "https://dashscope.aliyuncs.com/api/v1/services/aigc/text-generation/generation"
        );
        assert_eq!(
            qwen_multimodal_api_url(""),
            "https://dashscope.aliyuncs.com/api/v1/services/aigc/multimodal-generation/generation"
        );
        assert_eq!(
            qwen_api_url("https://ws-baake0p88awyuvg7.cn-beijing.maas.aliyuncs.com/compatible-mode/v1"),
            "https://ws-baake0p88awyuvg7.cn-beijing.maas.aliyuncs.com/api/v1/services/aigc/text-generation/generation"
        );
        assert!(
            qwen_native_http_url("", "qwen3.8-max").contains("multimodal-generation/generation"),
            "qwen3.8-max must not use text-generation (F1 text-only 400 url error)"
        );
        assert!(
            qwen_native_http_url("", "qwen3.7-max").contains("text-generation/generation"),
            "qwen3.7-max stays on text-generation"
        );
        assert!(!qwen_native_http_url("", "qwen3.7-max").contains("multimodal-generation"));
    }

    #[test]
    fn test_qwen_request_body_disables_thinking_for_qwen3_hybrid() {
        let body = qwen_request_body(
            "qwen3.7-max",
            &[QwenMessage {
                role: "user".into(),
                content: "hi".into(),
                reasoning_content: None,
            }],
            Some(0.7),
            None,
            false,
            None,
        );
        assert!(body["parameters"].get("response_format").is_none());
        assert!(body["parameters"].get("max_tokens").is_none());
        assert_eq!(body["parameters"]["enable_thinking"], json!(false));
        assert_eq!(body["parameters"]["result_format"], json!("message"));
    }

    #[test]
    fn test_qwen38_thinking_on_sets_preserve_thinking() {
        let body = qwen_request_body(
            "qwen3.8-max",
            &[QwenMessage {
                role: "user".into(),
                content: "hi".into(),
                reasoning_content: None,
            }],
            Some(0.2),
            Some(ThinkingRequest { enabled: true }),
            false,
            Some(256),
        );
        assert_eq!(body["parameters"]["max_tokens"], json!(256));
        assert!(body.pointer("/parameters/response_format").is_none());
        assert!(body.get("response_format").is_none());
        assert_eq!(body["parameters"]["enable_thinking"], json!(true));
        assert_eq!(body["parameters"]["preserve_thinking"], json!(true));
        assert_eq!(body["parameters"]["temperature"], json!(0.6));
    }

    #[test]
    fn test_qwen38_text_only_multimodal_content_is_array() {
        let body = qwen_multimodal_request_body(
            "qwen3.8-max",
            &[(
                "user".into(),
                TextTransportContent::Text("hello".into()),
                None,
            )],
            None,
            None,
            TextImageCountPolicy::ModelCapability,
            false,
            None,
        )
        .unwrap();
        assert_eq!(
            body["input"]["messages"][0]["content"],
            json!([{ "text": "hello" }]),
            "DashScope multimodal requires content array even for text-only"
        );
    }

    #[test]
    fn test_multimodal_body_uses_native_image_key() {
        let body = qwen_multimodal_request_body(
            "qwen3.8-max",
            &[(
                "user".into(),
                TextTransportContent::Parts(vec![
                    LlmContentPart::ImageUrl {
                        image_url: LlmImageUrl {
                            url: "data:image/png;base64,abc".into(),
                            detail: None,
                        },
                    },
                    LlmContentPart::Text {
                        text: "look".into(),
                    },
                ]),
                None,
            )],
            None,
            None,
            TextImageCountPolicy::ModelCapability,
            false,
            None,
        )
        .unwrap();
        let content = &body["input"]["messages"][0]["content"];
        assert_eq!(content[0]["image"], json!("data:image/png;base64,abc"));
        assert!(content[0].get("image_url").is_none());
        assert_eq!(content[1]["text"], json!("look"));
    }

    #[test]
    fn test_multimodal_rejects_video_part() {
        let err = qwen_multimodal_request_body(
            "qwen3.8-max",
            &[(
                "user".into(),
                TextTransportContent::Parts(vec![LlmContentPart::VideoUrl {
                    video_url: LlmVideoUrl {
                        url: "https://example.com/a.mp4".into(),
                        detail: None,
                        fps: None,
                    },
                }]),
                None,
            )],
            None,
            None,
            TextImageCountPolicy::ModelCapability,
            false,
            None,
        )
        .unwrap_err();
        assert!(err.contains("INV-QWEN38-VISION-03") || err.contains("视频"), "{err}");
    }

    #[test]
    fn fake_http_plain_text_uses_text_generation_url() {
        let poster = FakePoster::new(200, ok_body("plain"));
        let out = block_on_ready(qwen_chat_completion_on(
            &svc("qwen3.7-max"),
            vec![QwenMessage {
                role: "user".into(),
                content: "hi".into(),
                reasoning_content: None,
            }],
            None,
            None,
            false,
            None,
            &poster,
            None,
        ))
        .expect("qwen3.7 completion");
        assert_eq!(out.content, "plain");
        let url = &poster.recorded()[0].url;
        assert!(
            url.contains("text-generation/generation"),
            "{url}"
        );
        assert!(
            !url.contains("multimodal-generation"),
            "{url}"
        );
    }

    #[test]
    fn fake_http_qwen38_uses_multimodal_generation_url() {
        let poster = FakePoster::new(200, ok_body("mm"));
        let out = block_on_ready(qwen_chat_completion_on(
            &svc("qwen3.8-max"),
            vec![QwenMessage {
                role: "user".into(),
                content: "hi".into(),
                reasoning_content: None,
            }],
            None,
            None,
            false,
            None,
            &poster,
            None,
        ))
        .expect("qwen3.8 completion");
        assert_eq!(out.content, "mm");
        let url = &poster.recorded()[0].url;
        assert!(
            url.contains("multimodal-generation/generation"),
            "{url}"
        );
        assert!(!url.contains("text-generation/generation"), "{url}");
    }

    #[test]
    fn non_vision_model_multimodal_returns_unsupported_without_http() {
        let poster = FakePoster::new(200, ok_body("should-not-be-used"));
        let err = block_on_ready(qwen_chat_completion_multimodal_on(
            &svc("qwen3.7-max"),
            vec![(
                "user".into(),
                TextTransportContent::Parts(vec![LlmContentPart::ImageUrl {
                    image_url: LlmImageUrl {
                        url: "data:image/png;base64,abc".into(),
                        detail: None,
                    },
                }]),
                None,
            )],
            None,
            None,
            TextImageCountPolicy::ModelCapability,
            false,
            None,
            &poster,
            None,
        ))
        .unwrap_err();
        assert!(err.contains("INV-QWEN38-VISION-02"), "{err}");
        assert!(poster.recorded().is_empty(), "must not POST for unsupported vision");
    }
}
