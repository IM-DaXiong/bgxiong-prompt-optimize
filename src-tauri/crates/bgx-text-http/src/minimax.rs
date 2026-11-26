//! T28c MiniMax-M3 Anthropic Messages. Audit is an optional callback, never a database.

use serde_json::{json, Value};

use crate::openai::shared::{
    apply_thinking_fields, decorate_text_http_error_mapped, detect_thinking_capability,
    empty_text_user_message, maybe_wrap_thinking_error, LlmContentPart, PreparedJsonPost,
    ReqwestPoster, TextHttpPoster, TextServiceConfig, TextTransportContent, TextTransportMessage,
    ThinkingParamFamily, ThinkingRequest, TEXT_KIND_MINIMAX,
};

pub const PROVIDER_KIND: &str = TEXT_KIND_MINIMAX;
pub const DEFAULT_BASE_URL: &str = "https://api.minimaxi.com";
pub const DEFAULT_MODEL: &str = "MiniMax-M3";

pub fn anthropic_messages_url(base: &str) -> String {
    let trimmed = base.trim().trim_end_matches('/');
    let root = if trimmed.is_empty() {
        DEFAULT_BASE_URL
    } else {
        trimmed
    };
    let root = root
        .trim_end_matches("/anthropic/v1/messages")
        .trim_end_matches("/anthropic/v1")
        .trim_end_matches("/anthropic")
        .trim_end_matches('/');
    format!("{root}/anthropic/v1/messages")
}

fn resolve_model(service: &TextServiceConfig) -> String {
    service
        .model
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_MODEL)
        .to_string()
}

fn resolve_base_url(service: &TextServiceConfig) -> String {
    service
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_BASE_URL)
        .to_string()
}

fn split_system_and_messages(
    messages: &[TextTransportMessage],
) -> Result<(Option<String>, Vec<Value>), String> {
    let mut system_parts: Vec<String> = Vec::new();
    let mut out: Vec<Value> = Vec::new();
    for m in messages {
        let role = m.role.trim().to_ascii_lowercase();
        if role == "system" {
            system_parts.push(m.content.text_preview());
            continue;
        }
        if role != "user" && role != "assistant" {
            return Err(format!(
                "MiniMax-M3 Anthropic 仅支持 user/assistant/system 角色，收到: {}",
                m.role
            ));
        }
        let content = content_to_anthropic_blocks(&m.content)?;
        out.push(json!({
            "role": role,
            "content": content,
        }));
    }
    let system = if system_parts.is_empty() {
        None
    } else {
        Some(system_parts.join("\n\n"))
    };
    Ok((system, out))
}

fn content_to_anthropic_blocks(content: &TextTransportContent) -> Result<Value, String> {
    match content {
        TextTransportContent::Text(s) => Ok(json!(s)),
        TextTransportContent::Parts(parts) => {
            let mut blocks = Vec::with_capacity(parts.len());
            for p in parts {
                blocks.push(part_to_anthropic_block(p)?);
            }
            if blocks.is_empty() {
                return Err("多模态消息 content parts 为空".into());
            }
            Ok(Value::Array(blocks))
        }
    }
}

fn part_to_anthropic_block(part: &LlmContentPart) -> Result<Value, String> {
    match part {
        LlmContentPart::Text { text } => Ok(json!({ "type": "text", "text": text })),
        LlmContentPart::ImageUrl { image_url } => {
            let mut source = media_source_from_url(&image_url.url, true)?;
            if let Some(detail) = image_url
                .detail
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                let d = if detail.eq_ignore_ascii_case("auto") {
                    "default"
                } else {
                    detail
                };
                source
                    .as_object_mut()
                    .expect("source object")
                    .insert("detail".into(), json!(d));
            }
            Ok(json!({ "type": "image", "source": source }))
        }
        LlmContentPart::VideoUrl { video_url } => {
            let mut source = media_source_from_url(&video_url.url, false)?;
            if let Some(detail) = video_url
                .detail
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                let d = if detail.eq_ignore_ascii_case("auto") {
                    "default"
                } else {
                    detail
                };
                source
                    .as_object_mut()
                    .expect("source object")
                    .insert("detail".into(), json!(d));
            }
            if let Some(fps) = video_url.fps {
                source
                    .as_object_mut()
                    .expect("source object")
                    .insert("fps".into(), json!(fps));
            }
            Ok(json!({ "type": "video", "source": source }))
        }
    }
}

fn anthropic_base64_media_type(media_type: &str) -> String {
    match media_type {
        "video/quicktime" => "video/mov".to_string(),
        other => other.to_string(),
    }
}

fn media_source_from_url(url: &str, is_image: bool) -> Result<Value, String> {
    let u = url.trim();
    if u.is_empty() {
        return Err("媒体 URL 为空".into());
    }
    if let Some(rest) = u.strip_prefix("data:") {
        let (meta, data) = rest
            .split_once(',')
            .ok_or_else(|| "data URL 格式非法（缺少逗号）".to_string())?;
        let meta_l = meta.to_ascii_lowercase();
        if !meta_l.contains(";base64") {
            return Err("data URL 须为 base64 编码".into());
        }
        let media_type_raw = meta_l.split(';').next().unwrap_or("").trim();
        if media_type_raw.is_empty() {
            return Err("data URL 缺少 media_type".into());
        }
        if is_image && !media_type_raw.starts_with("image/") {
            return Err(format!("图片 data URL media_type 非法: {media_type_raw}"));
        }
        if !is_image && !media_type_raw.starts_with("video/") {
            return Err(format!("视频 data URL media_type 非法: {media_type_raw}"));
        }
        let media_type = anthropic_base64_media_type(media_type_raw);
        return Ok(json!({
            "type": "base64",
            "media_type": media_type,
            "data": data,
        }));
    }
    if u.starts_with("http://") || u.starts_with("https://") || u.starts_with("mm_file://") {
        return Ok(json!({ "type": "url", "url": u }));
    }
    Err(format!(
        "不支持的媒体 URL 协议（须 data: / http(s): / mm_file:）: {}",
        u.chars().take(48).collect::<String>()
    ))
}

fn require_protocol_max_tokens(max_tokens: Option<u32>) -> Result<u32, String> {
    max_tokens.ok_or_else(|| {
        "MiniMax 请求缺少协议必填的输出上限。这是配置错误，不是模型拒绝。".to_string()
    })
}

/// Build the Anthropic Messages body. `max_tokens` is already resolved by the caller.
/// Does not write OpenAI `response_format`.
pub fn build_request_body(
    model: &str,
    messages: &[TextTransportMessage],
    temperature: Option<f32>,
    thinking: Option<ThinkingRequest>,
    max_tokens: u32,
) -> Result<Value, String> {
    let (system, anthropic_messages) = split_system_and_messages(messages)?;
    let mut body = serde_json::Map::new();
    body.insert("model".into(), json!(model));
    body.insert("messages".into(), Value::Array(anthropic_messages));
    body.insert("max_tokens".into(), json!(max_tokens));
    body.insert("stream".into(), json!(false));
    if let Some(sys) = system {
        body.insert("system".into(), json!(sys));
    }
    if let Some(t) = temperature {
        body.insert("temperature".into(), json!(t));
    }
    match thinking {
        Some(req) => {
            let cap = detect_thinking_capability(PROVIDER_KIND, model);
            let family = match (cap.supported, cap.param_family) {
                (true, Some(f)) => f,
                _ => {
                    return Err(format!(
                        "thinking mode requested but model is not in whitelist: provider={PROVIDER_KIND} model={model}"
                    ));
                }
            };
            let family = match family {
                ThinkingParamFamily::MinimaxAdaptiveThinking => family,
                other => {
                    return Err(format!(
                        "MiniMax-M3 thinking family mismatch: expected MinimaxAdaptiveThinking, got {other:?}"
                    ));
                }
            };
            apply_thinking_fields(&mut body, family, req);
        }
        None => {
            body.insert("thinking".into(), json!({ "type": "disabled" }));
        }
    }
    Ok(Value::Object(body))
}

pub fn parse_anthropic_message_response(
    text: &str,
) -> Result<(String, Option<String>, Value), String> {
    let v: Value = serde_json::from_str(text).map_err(|e| format!("JSON 解析失败: {e}"))?;
    if v.get("type").and_then(|t| t.as_str()) == Some("error") {
        let msg = v
            .pointer("/error/message")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown anthropic error");
        return Err(msg.to_string());
    }
    let mut text_parts: Vec<String> = Vec::new();
    if let Some(arr) = v.get("content").and_then(|c| c.as_array()) {
        for block in arr {
            let ty = block.get("type").and_then(|t| t.as_str()).unwrap_or("");
            if ty == "text" {
                if let Some(t) = block.get("text").and_then(|x| x.as_str()) {
                    text_parts.push(t.to_string());
                }
            }
        }
    }
    let content = text_parts.join("");
    if content.trim().is_empty() {
        let stop_reason = v.get("stop_reason").and_then(|s| s.as_str());
        return Err(empty_text_user_message(stop_reason));
    }
    let finish_reason = v
        .get("stop_reason")
        .and_then(|s| s.as_str())
        .map(|s| s.to_string());
    Ok((content, finish_reason, v))
}

fn actual_sent_prompt(body: &Value, messages: &[TextTransportMessage]) -> String {
    let mut parts = Vec::new();
    if let Some(system) = body.get("system").and_then(Value::as_str) {
        parts.push(format!("[system]\n{system}"));
    }
    parts.extend(
        messages
            .iter()
            .filter(|m| !m.role.trim().eq_ignore_ascii_case("system"))
            .map(|m| format!("[{}]\n{}", m.role.trim(), m.content.text_preview())),
    );
    if let Some(tools) = body.get("tools") {
        parts.push(format!("[tools]\n{tools}"));
    }
    if let Some(choice) = body.get("tool_choice") {
        parts.push(format!("[tool_choice]\n{choice}"));
    }
    parts.join("\n\n")
}

/// Optional hook so the host can apply a prepared structured-output envelope
/// without this crate depending on `sha2` / `prepared_schema`.
pub type StructuredBodyHook = dyn Fn(&mut Value) -> Result<(), String> + Send + Sync;

pub async fn messages_completion_prepared(
    service: &TextServiceConfig,
    messages: &[TextTransportMessage],
    temperature: Option<f32>,
    thinking: Option<ThinkingRequest>,
    max_tokens: Option<u32>,
    response_format_json: bool,
    on_audit: Option<&dyn Fn(&str)>,
) -> Result<crate::openai::ChatCompletionOutput, String> {
    messages_completion_prepared_on(
        service,
        messages,
        temperature,
        thinking,
        max_tokens,
        response_format_json,
        on_audit,
        None,
        &ReqwestPoster,
        None,
    )
    .await
}

pub async fn messages_completion_prepared_on(
    service: &TextServiceConfig,
    messages: &[TextTransportMessage],
    temperature: Option<f32>,
    thinking: Option<ThinkingRequest>,
    max_tokens: Option<u32>,
    response_format_json: bool,
    on_audit: Option<&dyn Fn(&str)>,
    apply_structured: Option<&StructuredBodyHook>,
    poster: &dyn TextHttpPoster,
    error_mapper: Option<&crate::types::TextHttpErrorMapper>,
) -> Result<crate::openai::ChatCompletionOutput, String> {
    if response_format_json && apply_structured.is_none() {
        // Anthropic Messages has no json_object wire field; contract stays prompt-side.
    }
    let max_tokens_u32 = require_protocol_max_tokens(max_tokens)?;
    let api_key = service.require_api_key()?;
    let model = resolve_model(service);
    let url = anthropic_messages_url(&resolve_base_url(service));
    let mut body = build_request_body(&model, messages, temperature, thinking, max_tokens_u32)?;
    if let Some(hook) = apply_structured {
        hook(&mut body)?;
    }
    let sent = actual_sent_prompt(&body, messages);
    if let Some(audit) = on_audit {
        audit(&sent);
    }
    let request = PreparedJsonPost {
        url,
        headers: vec![
            ("Authorization".into(), format!("Bearer {api_key}")),
            ("Content-Type".into(), "application/json".into()),
            ("anthropic-version".into(), "2023-06-01".into()),
        ],
        body,
        provider_kind: PROVIDER_KIND.into(),
    };
    let res = poster
        .post_json(request, service.timeout_secs)
        .await?;
    if res.is_success() {
        let (content, finish_reason, raw) = parse_anthropic_message_response(&res.body)?;
        return Ok(crate::openai::ChatCompletionOutput {
            content,
            raw,
            finish_reason,
            reasoning_content: None,
        });
    }
    let err = decorate_text_http_error_mapped(PROVIDER_KIND, res.status, &res.body, error_mapper);
    Err(maybe_wrap_thinking_error(thinking, &res.body, err))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openai::shared::{block_on_ready, FakePoster};

    fn svc() -> TextServiceConfig {
        TextServiceConfig {
            api_key: Some("sk-mm".into()),
            base_url: Some("https://api.minimaxi.com".into()),
            model: Some("MiniMax-M3".into()),
            provider: Some(PROVIDER_KIND.into()),
            ..Default::default()
        }
    }

    fn user(text: &str) -> TextTransportMessage {
        TextTransportMessage {
            role: "user".into(),
            content: TextTransportContent::Text(text.into()),
            reasoning_content: None,
        }
    }

    fn ok_body() -> String {
        json!({
            "content": [{"type": "text", "text": "hello-m3"}],
            "stop_reason": "end_turn"
        })
        .to_string()
    }

    #[test]
    fn anthropic_url_strips_accidental_suffixes() {
        assert_eq!(
            anthropic_messages_url(""),
            "https://api.minimaxi.com/anthropic/v1/messages"
        );
        assert_eq!(
            anthropic_messages_url("https://api.minimaxi.com/anthropic/v1"),
            "https://api.minimaxi.com/anthropic/v1/messages"
        );
    }

    #[test]
    fn build_request_body_writes_disabled_thinking_by_default() {
        let body = build_request_body("MiniMax-M3", &[user("hi")], None, None, 1024).unwrap();
        assert_eq!(body["stream"], json!(false));
        assert_eq!(body["max_tokens"], json!(1024));
        assert_eq!(body["thinking"]["type"], json!("disabled"));
        assert!(body.get("response_format").is_none());
    }

    #[test]
    fn missing_max_tokens_is_config_error() {
        let err = require_protocol_max_tokens(None).unwrap_err();
        assert!(err.contains("配置错误"), "{err}");
    }

    #[test]
    fn on_audit_none_still_returns_body_without_database() {
        let poster = FakePoster::new(200, ok_body());
        let out = block_on_ready(messages_completion_prepared_on(
            &svc(),
            &[user("hi")],
            None,
            None,
            Some(256),
            false,
            None,
            None,
            &poster,
            None,
        ))
        .expect("minimax completion");
        assert_eq!(out.content, "hello-m3");
        assert_eq!(out.finish_reason.as_deref(), Some("end_turn"));
        let rec = poster.recorded();
        assert_eq!(rec.len(), 1);
        assert_eq!(
            rec[0].url,
            "https://api.minimaxi.com/anthropic/v1/messages"
        );
        assert!(rec[0]
            .headers
            .iter()
            .any(|(k, v)| k == "anthropic-version" && v == "2023-06-01"));
    }

    #[test]
    fn on_audit_some_sees_sent_text() {
        let poster = FakePoster::new(200, ok_body());
        let sent = std::sync::Mutex::new(None);
        let out = block_on_ready(messages_completion_prepared_on(
            &svc(),
            &[
                TextTransportMessage {
                    role: "system".into(),
                    content: TextTransportContent::Text("sys-rule".into()),
                    reasoning_content: None,
                },
                user("write a poem"),
            ],
            None,
            None,
            Some(256),
            false,
            Some(&|text| {
                *sent.lock().expect("audit slot") = Some(text.to_string());
            }),
            None,
            &poster,
            None,
        ))
        .expect("minimax completion");
        assert_eq!(out.content, "hello-m3");
        let audited = sent.lock().expect("audit slot").clone().expect("audited");
        assert!(audited.contains("[system]\nsys-rule"), "{audited}");
        assert!(audited.contains("[user]\nwrite a poem"), "{audited}");
    }

    #[test]
    fn request_body_keeps_original_image_and_video_parts() {
        let msg = TextTransportMessage {
            role: "user".into(),
            content: TextTransportContent::Parts(vec![
                LlmContentPart::Text {
                    text: "describe".into(),
                },
                LlmContentPart::ImageUrl {
                    image_url: crate::openai::shared::LlmImageUrl {
                        url: "data:image/png;base64,abc".into(),
                        detail: Some("auto".into()),
                    },
                },
                LlmContentPart::VideoUrl {
                    video_url: crate::openai::shared::LlmVideoUrl {
                        url: "https://example.com/clip.mp4".into(),
                        detail: None,
                        fps: Some(8.0),
                    },
                },
            ]),
            reasoning_content: None,
        };
        let body = build_request_body("MiniMax-M3", &[msg], None, None, 256).unwrap();
        let parts = body["messages"][0]["content"].as_array().expect("parts");
        assert_eq!(parts[0]["type"], json!("text"));
        assert_eq!(parts[1]["type"], json!("image"));
        assert_eq!(parts[1]["source"]["type"], json!("base64"));
        assert_eq!(parts[1]["source"]["media_type"], json!("image/png"));
        assert_eq!(parts[1]["source"]["data"], json!("abc"));
        assert_eq!(parts[1]["source"]["detail"], json!("default"));
        assert_eq!(parts[2]["type"], json!("video"));
        assert_eq!(parts[2]["source"]["type"], json!("url"));
        assert_eq!(parts[2]["source"]["url"], json!("https://example.com/clip.mp4"));
        assert_eq!(parts[2]["source"]["fps"], json!(8.0));
    }
}
