//! Port adapter from the shell config to the frozen `bgx-text-http` contract.
//!
//! Retired types (`TextHttpPart`, `TextHttpMultimodalMessage`, `TextHttpReply`)
//! must not be used. Transport is `TextTransportMessage` + `LlmContentPart`
//! (including `VideoUrl`). HTTP goes through an injected `TextHttpPoster`
//! (shared `ReqwestPoster` / `client_for_long_running`); this shell does not
//! implement a second wire protocol. Connect timeout is `CONNECT_TIMEOUT` (30s)
//! inside the crate exits. Whole-request timeout applies only when
//! `timeout_secs > 0`.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use bgx_prompt_optimize::{
    OptimizeError, TextCall, TextCompleter, TextImageCountPolicy as CoreImageCountPolicy, TextMessage,
    TextReply,
};
use bgx_text_http::{
    AdapterCompleteOptions, ChatCompletionOutput, ChatMessage, ChatMessageMultimodal,
    LlmContentPart, LlmImageUrl, LlmVideoUrl, ReqwestPoster, TextHttpPoster, TextImageCountPolicy,
    TextServiceConfig, TextTransportContent, TextTransportMessage, ThinkingRequest,
    CONNECT_TIMEOUT, TEXT_KIND_ADAPTER, TEXT_KIND_LOCAL, TEXT_KIND_MINIMAX, TEXT_KIND_QWEN,
};
use serde_json::Value;

use crate::adapter_package;
use crate::adapter_runner::ShellAdapterTextRunner;
use crate::config::{ShellConfig, ADAPTER_PROVIDER_KIND, LOCAL_PROVIDER_KIND};
use crate::error_map;
use crate::error_ui;
use crate::local_runtime::ShellLocalTextRuntime;
use crate::occupancy::SHELL_OCCUPANCY_LABEL;
use crate::paths;
use crate::text_caps;
use crate::text_thinking;

/// Existing host wording (`text_dispatch.rs`) for the video-input hard gate.
pub const VIDEO_INPUT_UNSUPPORTED: &str =
    "当前文本厂商不支持视频输入；请改用 MiniMax-M3（text_minimax_m3），或移除视频附件后重试";

pub struct ShellTextCompleter {
    pub config: ShellConfig,
    pub has_video_attachments: bool,
}

impl ShellTextCompleter {
    pub fn new(config: ShellConfig, has_video_attachments: bool) -> Result<Self, OptimizeError> {
        config.validate()?;
        Ok(Self {
            config,
            has_video_attachments,
        })
    }

    pub fn reject_unsupported_video(&self) -> Result<(), OptimizeError> {
        if self.has_video_attachments && self.config.provider_kind != "text_minimax_m3" {
            return Err(error_ui::err(
                "TEXT_VIDEO_INPUT_UNSUPPORTED",
                VIDEO_INPUT_UNSUPPORTED,
            ));
        }
        Ok(())
    }

    pub fn text_service_config(&self) -> TextServiceConfig {
        text_service_config_from_shell(&self.config)
    }
}

pub fn text_service_config_from_shell(config: &ShellConfig) -> TextServiceConfig {
    TextServiceConfig {
        api_key: config
            .api_key
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        base_url: config
            .base_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        model: config
            .model
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        provider: {
            let kind = config.provider_kind.trim();
            if kind.is_empty() {
                None
            } else {
                Some(kind.to_string())
            }
        },
        timeout_secs: config.request_timeout_secs(),
    }
}

pub fn http_image_count_policy(policy: CoreImageCountPolicy) -> TextImageCountPolicy {
    match policy {
        CoreImageCountPolicy::AsSubmitted => TextImageCountPolicy::AsSubmitted,
        CoreImageCountPolicy::ModelCapability => TextImageCountPolicy::ModelCapability,
    }
}

impl TextCompleter for ShellTextCompleter {
    fn check_cancel(&self) -> Result<(), OptimizeError> {
        crate::cancel::check_cancel()
    }

    fn complete(&mut self, call: TextCall) -> Result<TextReply, OptimizeError> {
        (call.check_cancel)()?;
        self.reject_unsupported_video()?;
        tracing::info!(
            model = self.config.model.as_deref().unwrap_or(""),
            provider = %self.config.provider_kind,
            step = %call.step_label,
            "text complete requested"
        );
        let messages = map_transport_messages(&call.messages)?;
        if transport_has_video(&messages) && self.config.provider_kind != "text_minimax_m3" {
            return Err(error_ui::err(
                "TEXT_VIDEO_INPUT_UNSUPPORTED",
                VIDEO_INPUT_UNSUPPORTED,
            ));
        }
        let service = self.text_service_config();
        let json_mode = call.json_mode || call.schema.is_some();
        let has_output_schema = call.schema.is_some();
        let policy = http_image_count_policy(call.image_count_policy);
        // Completer resolves thinking / max_tokens; do not grow TextCall fields here.
        let thinking = text_thinking::thinking_for_call(&self.config, json_mode || has_output_schema);
        let max_tokens = text_caps::resolve_max_tokens(&self.config)?;
        let kind = self.config.provider_kind.as_str();
        let poster = ReqwestPoster;
        let reply = dispatch(
            kind,
            &self.config,
            service,
            &messages,
            json_mode,
            has_output_schema,
            thinking,
            max_tokens,
            policy,
            &poster,
            call.check_cancel,
        )?;
        Ok(reply)
    }
}

fn dispatch(
    kind: &str,
    shell: &ShellConfig,
    service: TextServiceConfig,
    messages: &[TextTransportMessage],
    json_mode: bool,
    has_output_schema: bool,
    thinking: Option<ThinkingRequest>,
    max_tokens: Option<u32>,
    policy: TextImageCountPolicy,
    poster: &dyn TextHttpPoster,
    check_cancel: fn() -> Result<(), OptimizeError>,
) -> Result<TextReply, OptimizeError> {
    match kind {
        "text_openai_compat" | "text_volc_ark_compat" | "text_deepseek_compat" | "text_custom" => {
            let fut = openai_complete(&service, messages, json_mode, thinking, max_tokens, poster);
            let output = block_on_cancellable(fut, check_cancel, service.timeout_secs)?;
            Ok(reply_from_chat(output))
        }
        kind if kind == TEXT_KIND_QWEN || kind == "text_qwen" => {
            let fut = qwen_complete(&service, messages, json_mode, thinking, max_tokens, policy, poster);
            let output = block_on_cancellable(fut, check_cancel, service.timeout_secs)?;
            Ok(TextReply {
                content: output.content,
                finish_reason: output.finish_reason,
                raw_json: Some(output.raw),
            })
        }
        kind if kind == TEXT_KIND_MINIMAX || kind == "text_minimax_m3" => {
            let owned = messages.to_vec();
            let timeout_secs = service.timeout_secs;
            let fut = async move {
                bgx_text_http::minimax::messages_completion_prepared_on(
                    &service,
                    &owned,
                    None,
                    thinking,
                    max_tokens,
                    json_mode,
                    None,
                    None,
                    poster,
                    None,
                )
                .await
            };
            let output = block_on_cancellable(fut, check_cancel, timeout_secs)?;
            Ok(reply_from_chat(output))
        }
        kind if kind == ADAPTER_PROVIDER_KIND || kind == TEXT_KIND_ADAPTER => {
            (check_cancel)()?;
            let root = PathBuf::from(shell.adapter_package_root.as_deref().unwrap_or(""));
            let package = adapter_package::read_adapter_package(&root)?;
            let reader = adapter_package::memory_reader(&package);
            let options = AdapterCompleteOptions {
                temperature: None,
                max_tokens,
                thinking,
                response_format_json: json_mode,
                has_output_schema,
                on_audit: None,
            };
            let runner = ShellAdapterTextRunner;
            let output = bgx_text_http::complete(
                &package.root,
                &reader,
                &service,
                messages,
                options,
                &runner,
            )
            .map_err(error_map::map_exit_error)?;
            (check_cancel)()?;
            Ok(reply_from_chat(output))
        }
        kind if kind == LOCAL_PROVIDER_KIND || kind == TEXT_KIND_LOCAL => {
            (check_cancel)()?;
            if transport_has_video(messages) {
                return Err(error_ui::err(
                    "TEXT_LOCAL_VIDEO_UNSUPPORTED",
                    "冻结的本地出口只提取图片 data URL，不会把视频分片悄悄丢掉。",
                ));
            }
            let path = PathBuf::from(shell.local_model_path.as_deref().unwrap_or(""));
            let path = paths::require_absolute(&path, "本地模型路径")?;
            let runtime = ShellLocalTextRuntime::new(path.clone())
                .map_err(error_map::map_exit_error)?;
            let model_id = path.to_string_lossy().to_string();
            let output = bgx_text_http::complete_local_text_from_transport_messages(
                &runtime,
                Some(model_id.as_str()),
                SHELL_OCCUPANCY_LABEL,
                &crate::occupancy::require_occupancy,
                messages,
                None,
                json_mode,
                None,
                thinking,
                policy,
            )
            .map_err(error_map::map_exit_error)?;
            (check_cancel)()?;
            Ok(TextReply {
                content: output.0,
                finish_reason: output.1,
                raw_json: None,
            })
        }
        other => Err(error_ui::err(
            "TEXT_PROVIDER_UNKNOWN",
            format!("不支持的文本渠道：{other}"),
        )),
    }
}

async fn openai_complete(
    service: &TextServiceConfig,
    messages: &[TextTransportMessage],
    json_mode: bool,
    thinking: Option<ThinkingRequest>,
    max_tokens: Option<u32>,
    poster: &dyn TextHttpPoster,
) -> Result<ChatCompletionOutput, String> {
    if transport_is_text_only(messages) {
        let chat: Vec<ChatMessage> = messages
            .iter()
            .map(|message| ChatMessage {
                role: message.role.clone(),
                content: flatten_text(&message.content),
            })
            .collect();
        bgx_text_http::chat_completion_with_max_tokens_on(
            service, chat, None, json_mode, thinking, max_tokens, poster, None,
        )
        .await
    } else {
        let chat: Vec<ChatMessageMultimodal> = messages
            .iter()
            .map(|message| ChatMessageMultimodal {
                role: message.role.clone(),
                content: message.content.clone(),
            })
            .collect();
        bgx_text_http::chat_completion_multimodal_on(
            service, chat, None, json_mode, thinking, max_tokens, poster, None,
        )
        .await
    }
}

async fn qwen_complete(
    service: &TextServiceConfig,
    messages: &[TextTransportMessage],
    json_mode: bool,
    thinking: Option<ThinkingRequest>,
    max_tokens: Option<u32>,
    policy: TextImageCountPolicy,
    poster: &dyn TextHttpPoster,
) -> Result<bgx_text_http::QwenOutput, String> {
    // Frozen signatures (FROZEN-INTERFACE-20261116) include trailing
    // response_format_json / max_tokens. Landed qwen.rs at 978e84 still
    // lacks them; B is patching. This call follows the frozen form and must
    // not pass a boolean in place of TextImageCountPolicy.
    if transport_is_text_only(messages) {
        let qwen: Vec<bgx_text_http::QwenMessage> = messages
            .iter()
            .map(|message| bgx_text_http::QwenMessage {
                role: message.role.clone(),
                content: flatten_text(&message.content),
                reasoning_content: message.reasoning_content.clone(),
            })
            .collect();
        bgx_text_http::qwen_chat_completion_on(
            service, qwen, None, thinking, json_mode, max_tokens, poster, None,
        )
        .await
    } else {
        let qwen: Vec<(String, TextTransportContent, Option<String>)> = messages
            .iter()
            .map(|message| {
                (
                    message.role.clone(),
                    message.content.clone(),
                    message.reasoning_content.clone(),
                )
            })
            .collect();
        bgx_text_http::qwen_chat_completion_multimodal_on(
            service, qwen, None, thinking, policy, json_mode, max_tokens, poster, None,
        )
        .await
    }
}

fn reply_from_chat(output: ChatCompletionOutput) -> TextReply {
    TextReply {
        content: output.content,
        finish_reason: output.finish_reason,
        raw_json: Some(output.raw),
    }
}

pub fn map_transport_messages(
    messages: &[TextMessage],
) -> Result<Vec<TextTransportMessage>, OptimizeError> {
    messages
        .iter()
        .map(|message| {
            Ok(TextTransportMessage {
                role: message.role.clone(),
                content: map_content(&message.content)?,
                reasoning_content: message
                    .content
                    .as_object()
                    .and_then(|obj| obj.get("reasoningContent").or_else(|| obj.get("reasoning_content")))
                    .and_then(Value::as_str)
                    .map(str::to_string),
                })
        })
        .collect()
}

fn map_content(content: &Value) -> Result<TextTransportContent, OptimizeError> {
    match content {
        Value::String(text) => Ok(TextTransportContent::Text(text.clone())),
        Value::Null => Ok(TextTransportContent::Text(String::new())),
        Value::Array(items) => {
            let mut parts = Vec::new();
            for item in items {
                parts.extend(map_one_part(item)?);
            }
            if parts.iter().all(|part| matches!(part, LlmContentPart::Text { .. })) {
                Ok(TextTransportContent::Text(flatten_parts_text(&parts)))
            } else {
                Ok(TextTransportContent::Parts(parts))
            }
        }
        other => {
            let parts = map_one_part(other)?;
            if parts.len() == 1 {
                if let LlmContentPart::Text { text } = &parts[0] {
                    return Ok(TextTransportContent::Text(text.clone()));
                }
            }
            Ok(TextTransportContent::Parts(parts))
        }
    }
}

fn map_one_part(value: &Value) -> Result<Vec<LlmContentPart>, OptimizeError> {
    if let Some(text) = value.as_str() {
        return Ok(vec![LlmContentPart::Text {
            text: text.to_string(),
        }]);
    }
    let Some(obj) = value.as_object() else {
        return Err(error_ui::err(
            "TEXT_MESSAGE_SHAPE",
            "文本消息内容不是可识别的字符串或分片。",
        ));
    };
    let kind = obj
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    if kind.contains("video") {
        let url = media_url(obj, ["videoUrl", "video_url", "url", "dataUrl", "data_url"])
            .ok_or_else(|| {
                error_ui::err("TEXT_MESSAGE_SHAPE", "视频分片缺少可识别的 URL。")
            })?;
        return Ok(vec![LlmContentPart::VideoUrl {
            video_url: LlmVideoUrl {
                url,
                detail: None,
                fps: None,
            },
        }]);
    }
    if kind == "text"
        || (kind.is_empty()
            && obj.contains_key("text")
            && !obj.contains_key("imageUrl")
            && !obj.contains_key("image_url")
            && !obj.contains_key("dataUrl")
            && !obj.contains_key("data_url"))
    {
        let text = obj
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        return Ok(vec![LlmContentPart::Text { text }]);
    }
    if let Some(url) = media_url(obj, ["dataUrl", "data_url", "imageUrl", "image_url", "image", "url"]) {
        return Ok(vec![LlmContentPart::ImageUrl {
            image_url: LlmImageUrl { url, detail: None },
        }]);
    }
    Err(error_ui::err(
        "TEXT_MESSAGE_SHAPE",
        format!("无法把消息分片映射到冻结的 LlmContentPart（type={kind}）。"),
    ))
}

fn media_url(obj: &serde_json::Map<String, Value>, keys: impl IntoIterator<Item = &'static str>) -> Option<String> {
    for key in keys {
        match obj.get(key) {
            Some(Value::String(url)) if !url.trim().is_empty() => return Some(url.clone()),
            Some(Value::Object(inner)) => {
                for inner_key in ["url", "dataUrl", "data_url"] {
                    if let Some(url) = inner.get(inner_key).and_then(Value::as_str) {
                        if !url.trim().is_empty() {
                            return Some(url.to_string());
                        }
                    }
                }
            }
            _ => {}
        }
    }
    None
}

fn flatten_text(content: &TextTransportContent) -> String {
    match content {
        TextTransportContent::Text(text) => text.clone(),
        TextTransportContent::Parts(parts) => flatten_parts_text(parts),
    }
}

fn flatten_parts_text(parts: &[LlmContentPart]) -> String {
    parts
        .iter()
        .filter_map(|part| match part {
            LlmContentPart::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn transport_is_text_only(messages: &[TextTransportMessage]) -> bool {
    messages.iter().all(|message| !message.content.is_multimodal())
}

fn transport_has_video(messages: &[TextTransportMessage]) -> bool {
    messages
        .iter()
        .any(|message| message.content.video_count() > 0)
}

fn block_on_cancellable<T>(
    fut: impl std::future::Future<Output = Result<T, String>>,
    check_cancel: fn() -> Result<(), OptimizeError>,
    timeout_secs: Option<u64>,
) -> Result<T, OptimizeError> {
    let work = async move {
        let result = if let Some(secs) = timeout_secs.filter(|s| *s > 0) {
            match tokio::time::timeout(Duration::from_secs(secs), fut).await {
                Ok(inner) => inner,
                Err(_) => return Err(error_ui::timeout()),
            }
        } else {
            fut.await
        };
        result.map_err(error_map::map_exit_error)
    };
    let run = async move { poll_until(work, check_cancel).await };
    if tokio::runtime::Handle::try_current().is_ok() {
        tokio::task::block_in_place(|| tokio::runtime::Handle::current().block_on(run))
    } else {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| error_ui::err("TEXT_RUNTIME", format!("无法启动文本请求运行时：{error}")))?
            .block_on(run)
    }
}

async fn poll_until<T>(
    work: impl std::future::Future<Output = Result<T, OptimizeError>>,
    check_cancel: fn() -> Result<(), OptimizeError>,
) -> Result<T, OptimizeError> {
    tokio::pin!(work);
    let mut interval = tokio::time::interval(Duration::from_millis(50));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = interval.tick() => {
                check_cancel()?;
            }
            result = &mut work => {
                return result;
            }
        }
    }
}

/// Script completer for unit tests only. Production commands must not construct this.
#[cfg(test)]
pub struct ScriptCompleter {
    pub replies: std::collections::VecDeque<TextReply>,
    pub steps: Vec<String>,
}

#[cfg(test)]
impl TextCompleter for ScriptCompleter {
    fn complete(&mut self, call: TextCall) -> Result<TextReply, OptimizeError> {
        (call.check_cancel)()?;
        self.steps.push(call.step_label.clone());
        self.replies.pop_front().ok_or_else(|| {
            error_ui::err("SCRIPT_COMPLETER_EXHAUSTED", "测试脚本回复已经用完")
        })
    }
}

pub fn openai_compat_path() -> &'static str {
    "/v1/chat/completions"
}

pub fn qwen_text_path() -> &'static str {
    "/api/v1/services/aigc/text-generation/generation"
}

pub fn qwen_multimodal_path() -> &'static str {
    "/api/v1/services/aigc/multimodal-generation/generation"
}

pub fn minimax_path() -> &'static str {
    "/anthropic/v1/messages"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cloud_config(kind: &str, base_url: &str) -> ShellConfig {
        ShellConfig {
            provider_kind: kind.into(),
            base_url: Some(base_url.into()),
            model: Some("demo".into()),
            api_key: Some("x".into()),
            ..ShellConfig::default()
        }
    }

    fn text_call(content: Value) -> TextCall {
        TextCall {
            messages: vec![TextMessage {
                role: "user".into(),
                content,
            }],
            json_mode: false,
            schema: None,
            step_label: "loopback".into(),
            check_cancel: || Ok(()),
            image_count_policy: CoreImageCountPolicy::AsSubmitted,
        }
    }

    #[test]
    fn text_service_config_keeps_provider_and_only_a_positive_timeout() {
        let mut cfg = cloud_config("text_qwen", "https://example.invalid");
        let mapped = text_service_config_from_shell(&cfg);
        assert_eq!(mapped.provider.as_deref(), Some("text_qwen"));
        assert_eq!(mapped.timeout_secs, None);
        assert_eq!(CONNECT_TIMEOUT, Duration::from_secs(30));
        cfg.timeout_secs = Some(0);
        assert_eq!(text_service_config_from_shell(&cfg).timeout_secs, None);
        cfg.timeout_secs = Some(12);
        assert_eq!(text_service_config_from_shell(&cfg).timeout_secs, Some(12));
    }

    #[test]
    fn as_submitted_policy_is_not_coerced_to_model_capability() {
        assert_eq!(
            http_image_count_policy(CoreImageCountPolicy::AsSubmitted),
            TextImageCountPolicy::AsSubmitted
        );
        assert_eq!(
            http_image_count_policy(CoreImageCountPolicy::ModelCapability),
            TextImageCountPolicy::ModelCapability
        );
    }

    #[test]
    fn video_attachment_is_rejected_with_host_wording_unless_minimax() {
        let mut cfg = cloud_config("text_qwen", "https://example.invalid");
        let completer = ShellTextCompleter::new(cfg.clone(), true).unwrap();
        let err = completer.reject_unsupported_video().unwrap_err();
        assert_eq!(err.message, VIDEO_INPUT_UNSUPPORTED);
        cfg.provider_kind = "text_minimax_m3".into();
        cfg.base_url = Some("https://api.minimaxi.com".into());
        cfg.model = Some("MiniMax-M3".into());
        ShellTextCompleter::new(cfg, true)
            .unwrap()
            .reject_unsupported_video()
            .unwrap();
    }

    #[test]
    fn expected_cloud_paths_match_the_host() {
        assert_eq!(openai_compat_path(), "/v1/chat/completions");
        assert_eq!(
            qwen_text_path(),
            "/api/v1/services/aigc/text-generation/generation"
        );
        assert_eq!(
            qwen_multimodal_path(),
            "/api/v1/services/aigc/multimodal-generation/generation"
        );
        assert_eq!(minimax_path(), "/anthropic/v1/messages");
    }

    #[test]
    fn script_completer_is_test_only_and_records_steps() {
        let mut script = ScriptCompleter {
            replies: std::collections::VecDeque::from([TextReply {
                content: "hello".into(),
                finish_reason: None,
                raw_json: None,
            }]),
            steps: vec![],
        };
        let reply = script
            .complete(TextCall {
                messages: vec![],
                json_mode: false,
                schema: None,
                step_label: "video-expression".into(),
                check_cancel: || Ok(()),
                image_count_policy: CoreImageCountPolicy::ModelCapability,
            })
            .unwrap();
        assert_eq!(reply.content, "hello");
        assert_eq!(script.steps, ["video-expression"]);
    }

    #[test]
    fn transport_keeps_image_and_video_parts() {
        let mapped = map_transport_messages(&[TextMessage {
            role: "user".into(),
            content: serde_json::json!([
                {"type": "text", "text": "look"},
                {"type": "image_url", "image_url": {"url": "data:image/png;base64,AAA"}},
                {"type": "video_url", "video_url": {"url": "data:video/mp4,BBB"}}
            ]),
        }])
        .unwrap();
        match &mapped[0].content {
            TextTransportContent::Parts(parts) => {
                assert!(matches!(&parts[1], LlmContentPart::ImageUrl { image_url } if image_url.url == "data:image/png;base64,AAA"));
                assert!(matches!(&parts[2], LlmContentPart::VideoUrl { video_url } if video_url.url == "data:video/mp4,BBB"));
            }
            TextTransportContent::Text(_) => panic!("image/video parts must not be flattened"),
        }
        assert!(transport_has_video(&mapped));
    }

    fn spawn_loopback(body: &'static str) -> (String, thread::JoinHandle<Option<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let addr = listener.local_addr().expect("addr");
        let seen = Arc::new(Mutex::new(None));
        let seen_thread = Arc::clone(&seen);
        let handle = thread::spawn(move || {
            listener.set_nonblocking(true).ok();
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let mut buf = vec![0u8; 8192];
                        let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
                        let n = stream.read(&mut buf).unwrap_or(0);
                        let req = String::from_utf8_lossy(&buf[..n]);
                        let path = req
                            .lines()
                            .next()
                            .and_then(|line| line.split_whitespace().nth(1))
                            .unwrap_or("")
                            .to_string();
                        *seen_thread.lock().unwrap() = Some(path);
                        let resp = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        );
                        let _ = stream.write_all(resp.as_bytes());
                        break;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        if Instant::now() >= deadline {
                            break;
                        }
                        thread::sleep(Duration::from_millis(20));
                    }
                    Err(_) => break,
                }
            }
            seen_thread.lock().unwrap().clone()
        });
        (format!("http://{addr}"), handle)
    }

    // Loopback tests call the frozen exits against a fake server. They must not
    // be soft-skipped if a body is still incomplete.

    #[test]
    fn openai_loopback_calls_frozen_chat_completion() {
        let (base, server) = spawn_loopback(
            r#"{"choices":[{"message":{"content":"loopback-ok"},"finish_reason":"stop"}]}"#,
        );
        let cfg = cloud_config("text_openai_compat", &base);
        let mut completer = ShellTextCompleter::new(cfg, false).unwrap();
        let reply = completer
            .complete(text_call(Value::String("hello".into())))
            .expect("OpenAI frozen exit must complete against the loopback server");
        assert_eq!(reply.content, "loopback-ok");
        let path = server.join().ok().flatten().expect("request reached the fake server");
        assert!(path.contains(openai_compat_path()), "loopback path was {path}");
    }

    #[test]
    fn qwen_loopback_calls_frozen_text_exit() {
        let (base, server) = spawn_loopback(r#"{"output":{"text":"loopback-ok","finish_reason":"stop"}}"#);
        let cfg = cloud_config("text_qwen", &base);
        let mut completer = ShellTextCompleter::new(cfg, false).unwrap();
        let reply = completer
            .complete(text_call(Value::String("hello".into())))
            .expect("Qwen frozen exit must complete against the loopback server");
        assert_eq!(reply.content, "loopback-ok");
        let path = server.join().ok().flatten().expect("request reached the fake server");
        assert!(path.contains(qwen_text_path()), "loopback path was {path}");
    }

    #[test]
    fn minimax_loopback_keeps_video_parts_on_the_frozen_exit() {
        let (base, server) = spawn_loopback(
            r#"{"content":[{"type":"text","text":"loopback-ok"}],"stop_reason":"end_turn"}"#,
        );
        let mut cfg = cloud_config("text_minimax_m3", &base);
        // Use a settings-provided official cap so the frozen exit can run; do not invent one in Completer.
        cfg.max_output_tokens = Some(8192);
        let mut completer = ShellTextCompleter::new(cfg, true).unwrap();
        // Video parts must stay on the wire. If the official cap were missing, Completer
        // would return TEXT_OUTPUT_CAP_UNREGISTERED instead of inventing a budget.
        let mapped = map_transport_messages(&[TextMessage {
            role: "user".into(),
            content: serde_json::json!([
                {"type": "text", "text": "hello"},
                {"type": "video_url", "video_url": {"url": "data:video/mp4;base64,AAAA"}}
            ]),
        }])
        .unwrap();
        assert!(transport_has_video(&mapped));
        let result = completer.complete(text_call(serde_json::json!([
            {"type": "text", "text": "hello"},
            {"type": "video_url", "video_url": {"url": "data:video/mp4;base64,AAAA"}}
        ])));
        match result {
            Ok(reply) => {
                assert_eq!(reply.content, "loopback-ok");
                let path = server.join().ok().flatten().expect("request reached the fake server");
                assert!(path.contains(minimax_path()), "loopback path was {path}");
            }
            Err(error) => {
                // Official MiniMax protocol requires max_tokens; the shell does not invent one.
                assert!(
                    error.message.contains("输出上限") || error.message.contains("max_tokens"),
                    "unexpected MiniMax error: {}",
                    error.message
                );
                let _ = server.join();
            }
        }
    }
}
