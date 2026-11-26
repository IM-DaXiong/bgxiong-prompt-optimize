//! Shared no-database types and HTTP helpers for every text exit in this crate.
//!
//! Frozen `lib.rs` cannot declare a top-level support module, so other channel
//! modules import these items via `crate::openai::shared`.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Connect-stage timeout used when the caller does not set a whole-request timeout.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// Text API response size cap (bytes). Matches host `http_limits::MAX_TEXT_RESPONSE_SIZE`.
pub const MAX_TEXT_RESPONSE_SIZE: u64 = 512 * 1024;

/// Maximum POSTs for one completion, including json_object / max_tokens retries.
pub const MAX_CHAT_COMPLETION_POSTS: u32 = 4;

/// OpenAI-compatible family kinds covered by [`crate::openai`].
pub const OPENAI_COMPAT_KINDS: &[&str] = &[
    "text_openai_compat",
    "text_volc_ark_compat",
    "text_deepseek_compat",
    "text_custom",
];

pub const TEXT_KIND_QWEN: &str = "text_qwen";
pub const TEXT_KIND_MINIMAX: &str = "text_minimax_m3";
pub const TEXT_KIND_LOCAL: &str = "text_local_model";
pub const TEXT_KIND_ADAPTER: &str = "text_vendor_adapter";

/// Must match `modelIdPrefix` in `packages/app-contracts/v1/vendor-adapter.json`.
pub const VENDOR_ADAPTER_MODEL_ID_PREFIX: &str = "adapter:";

/// Subset of host `AiServiceConfig` needed to build and send a text request.
/// No database path, no host secrets store.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TextServiceConfig {
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub provider: Option<String>,
    pub timeout_secs: Option<u64>,
}

impl TextServiceConfig {
    pub fn provider_kind(&self) -> &str {
        self.provider
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("text_openai_compat")
    }

    pub fn require_api_key(&self) -> Result<String, String> {
        self.api_key
            .as_deref()
            .map(normalize_api_key_for_bearer)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| "未配置 api_key".to_string())
    }

    pub fn require_model(&self) -> Result<&str, String> {
        self.model
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| "未指定 model（请在配置或覆盖中设置）".to_string())
    }

    /// Whole-request timeout only when a positive value is configured.
    pub fn request_timeout(&self) -> Option<Duration> {
        self.timeout_secs
            .filter(|&s| s > 0)
            .map(Duration::from_secs)
    }
}

/// Normalize an API key for a Bearer header: strip BOM / zero-width / quotes / repeated `Bearer `.
pub fn normalize_api_key_for_bearer(key: &str) -> String {
    fn strip_invisible_and_linebreaks(s: &str) -> String {
        s.chars()
            .filter(|&c| {
                !matches!(
                    c,
                    '\u{FEFF}' | '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{2060}' | '\u{00AD}'
                ) && !matches!(c, '\r' | '\n' | '\t')
            })
            .map(|c| if c == '\u{00A0}' { ' ' } else { c })
            .collect()
    }

    fn strip_outer_wrappers(mut s: String) -> String {
        const PAIRS: &[(char, char)] = &[
            ('"', '"'),
            ('\u{201C}', '\u{201D}'),
            ('\u{2018}', '\u{2019}'),
            ('「', '」'),
        ];
        for _ in 0..3 {
            let t = s.trim();
            let mut next: Option<String> = None;
            let chars: Vec<char> = t.chars().collect();
            if chars.len() < 2 {
                break;
            }
            for &(a, b) in PAIRS {
                if chars[0] == a && chars[chars.len() - 1] == b {
                    next = Some(
                        chars[1..chars.len() - 1]
                            .iter()
                            .collect::<String>()
                            .trim()
                            .to_string(),
                    );
                    break;
                }
            }
            match next {
                Some(n) => s = n,
                None => break,
            }
        }
        s
    }

    let mut s = strip_invisible_and_linebreaks(key);
    s = s.trim().to_string();
    s = strip_outer_wrappers(s);

    loop {
        let t = s.trim();
        let b = t.as_bytes();
        if b.len() >= 7 && b[..7].eq_ignore_ascii_case(b"bearer ") {
            s = t[7..].trim().to_string();
            continue;
        }
        break;
    }
    s.trim().to_string()
}

/// Build a reqwest client with the long-running text contract:
/// connect timeout 30s; whole-request timeout only if `timeout_secs > 0`.
pub fn client_for_long_running(timeout_secs: Option<u64>) -> Result<reqwest::Client, String> {
    // Destination-aware proxy / HTTP version is applied by ReqwestPoster via SendOpts.
    Ok(crate::openai::outbound_http::clients::get(
        crate::openai::outbound_http::ClientFingerprint::long_running(timeout_secs),
    ))
}

pub fn format_reqwest_error(e: &reqwest::Error) -> String {
    crate::openai::outbound_http::error::format_reqwest_source_chain(e)
}

pub async fn read_response_text_limited(
    res: reqwest::Response,
    max_size: u64,
) -> Result<String, String> {
    if let Some(len) = res.content_length() {
        if len > max_size {
            return Err(format!(
                "响应过大：Content-Length {len} bytes 超过限制 {max_size} bytes"
            ));
        }
    }
    let bytes = res.bytes().await.map_err(|e| crate::openai::http_transport::append_body_read_transport_hint(&format_reqwest_error(&e)))?;
    if bytes.len() as u64 > max_size {
        return Err(format!(
            "响应过大：实际 {} bytes 超过限制 {max_size} bytes",
            bytes.len()
        ));
    }
    String::from_utf8(bytes.to_vec()).map_err(|e| format!("响应不是合法 UTF-8: {e}"))
}

/// A JSON POST prepared for a text channel, used by both live HTTP and fake transports.
#[derive(Debug, Clone)]
pub struct PreparedJsonPost {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Value,
    pub provider_kind: String,
}

#[derive(Debug, Clone)]
pub struct HttpTextResponse {
    pub status: u16,
    pub body: String,
}

impl HttpTextResponse {
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

pub type PostFuture =
    Pin<Box<dyn Future<Output = Result<HttpTextResponse, String>> + Send>>;

/// Injectable JSON poster so tests can assert URL and body without a network.
pub trait TextHttpPoster: Send + Sync {
    fn post_json(&self, request: PreparedJsonPost, timeout_secs: Option<u64>) -> PostFuture;
}

/// Default poster: reqwest with the long-running timeout contract.
#[derive(Debug, Default, Clone, Copy)]
pub struct ReqwestPoster;

impl TextHttpPoster for ReqwestPoster {
    fn post_json(&self, request: PreparedJsonPost, timeout_secs: Option<u64>) -> PostFuture {
        Box::pin(async move {
            let endpoint = request
                .url
                .parse::<url::Url>()
                .map_err(|e| format!("无效的请求地址: {e}"))?;
            let mut opts = crate::openai::outbound_http::SendOpts::cloud_post()
                .with_timeout_secs(timeout_secs);
            opts.client = opts.client.for_destination_url(&endpoint);
            if opts.client.bypass_proxy {
                // INV-NET-LOOPBACK-01: loopback uses HTTP/1 and no system proxy.
                opts.client.http1_only = true;
            }
            let provider = if request.provider_kind.trim().is_empty() {
                "text_openai_compat".to_string()
            } else {
                request.provider_kind.clone()
            };
            let url = request.url.clone();
            let headers = request.headers.clone();
            let body = request.body.clone();
            let res = crate::openai::outbound_adapt::send_with_opts(
                "text",
                &provider,
                opts,
                move |c| {
                    let mut builder = c.post(&url);
                    for (name, value) in &headers {
                        builder = builder.header(name.as_str(), value.as_str());
                    }
                    builder.json(&body)
                },
            )
            .await?;
            let status = res.status().as_u16();
            let body = read_response_text_limited(res, MAX_TEXT_RESPONSE_SIZE).await?;
            Ok(HttpTextResponse { status, body })
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".into(),
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".into(),
            content: content.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessageMultimodal {
    pub role: String,
    pub content: TextTransportContent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatCompletionOutput {
    pub content: String,
    #[serde(default)]
    pub raw: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LlmContentPart {
    Text { text: String },
    ImageUrl { image_url: LlmImageUrl },
    VideoUrl { video_url: LlmVideoUrl },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmImageUrl {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmVideoUrl {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fps: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TextTransportContent {
    Text(String),
    Parts(Vec<LlmContentPart>),
}

impl TextTransportContent {
    pub fn is_multimodal(&self) -> bool {
        matches!(
            self,
            TextTransportContent::Parts(parts)
                if parts.iter().any(|p| {
                    matches!(
                        p,
                        LlmContentPart::ImageUrl { .. } | LlmContentPart::VideoUrl { .. }
                    )
                })
        )
    }

    pub fn text_preview(&self) -> String {
        match self {
            TextTransportContent::Text(s) => s.clone(),
            TextTransportContent::Parts(parts) => parts
                .iter()
                .map(|p| match p {
                    LlmContentPart::Text { text } => text.clone(),
                    LlmContentPart::ImageUrl { .. } => "[image]".to_string(),
                    LlmContentPart::VideoUrl { .. } => "[video]".to_string(),
                })
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }

    pub fn image_count(&self) -> usize {
        match self {
            TextTransportContent::Text(_) => 0,
            TextTransportContent::Parts(parts) => parts
                .iter()
                .filter(|p| matches!(p, LlmContentPart::ImageUrl { .. }))
                .count(),
        }
    }

    pub fn video_count(&self) -> usize {
        match self {
            TextTransportContent::Text(_) => 0,
            TextTransportContent::Parts(parts) => parts
                .iter()
                .filter(|p| matches!(p, LlmContentPart::VideoUrl { .. }))
                .count(),
        }
    }
}

impl From<String> for TextTransportContent {
    fn from(s: String) -> Self {
        TextTransportContent::Text(s)
    }
}

impl From<&str> for TextTransportContent {
    fn from(s: &str) -> Self {
        TextTransportContent::Text(s.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextTransportMessage {
    pub role: String,
    pub content: TextTransportContent,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
}

/// Image-count policy used by Qwen / local vision gates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextImageCountPolicy {
    ModelCapability,
    AsSubmitted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ThinkingParamFamily {
    AdapterBoolean,
    QwenEnableThinking,
    DeepSeekThinkingObject,
    GptOssReasoningEffort,
    MinimaxAdaptiveThinking,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThinkingCapability {
    pub supported: bool,
    pub label: Option<String>,
    pub param_family: Option<ThinkingParamFamily>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThinkingRequest {
    pub enabled: bool,
}

pub fn thinking_cap(
    supported: bool,
    label: impl Into<String>,
    family: ThinkingParamFamily,
) -> ThinkingCapability {
    ThinkingCapability {
        supported,
        label: Some(label.into()),
        param_family: Some(family),
    }
}

pub fn thinking_unsupported() -> ThinkingCapability {
    ThinkingCapability {
        supported: false,
        label: None,
        param_family: None,
    }
}

pub fn apply_thinking_fields(
    body: &mut serde_json::Map<String, Value>,
    family: ThinkingParamFamily,
    req: ThinkingRequest,
) {
    match family {
        ThinkingParamFamily::AdapterBoolean => {
            body.insert("thinking".into(), json!(req.enabled));
        }
        ThinkingParamFamily::QwenEnableThinking => {
            body.insert("enable_thinking".into(), json!(req.enabled));
        }
        ThinkingParamFamily::DeepSeekThinkingObject => {
            let ty = if req.enabled { "enabled" } else { "disabled" };
            body.insert("thinking".into(), json!({ "type": ty }));
        }
        ThinkingParamFamily::GptOssReasoningEffort => {
            let effort = if req.enabled { "medium" } else { "low" };
            body.insert("reasoning_effort".into(), json!(effort));
        }
        ThinkingParamFamily::MinimaxAdaptiveThinking => {
            let ty = if req.enabled { "adaptive" } else { "disabled" };
            body.insert("thinking".into(), json!({ "type": ty }));
        }
    }
}

pub fn apply_thinking_to_qwen_parameters(
    parameters: &mut serde_json::Map<String, Value>,
    req: ThinkingRequest,
) {
    parameters.insert("enable_thinking".into(), json!(req.enabled));
}

/// Cloud / OpenAI-compat thinking whitelist. Vendor-adapter models are not resolved
/// against the host package store; pass [`detect_thinking_capability_with_adapter`]
/// when the caller already knows the package declaration.
pub fn detect_thinking_capability(provider_kind: &str, model_id: &str) -> ThinkingCapability {
    detect_thinking_capability_with_adapter(provider_kind, model_id, None)
}

pub fn detect_thinking_capability_with_adapter(
    provider_kind: &str,
    model_id: &str,
    adapter_supports_thinking: Option<bool>,
) -> ThinkingCapability {
    let pk = provider_kind.trim();
    let m = model_id.trim().to_ascii_lowercase();
    if pk.is_empty() || m.is_empty() {
        return thinking_unsupported();
    }
    if pk == TEXT_KIND_ADAPTER {
        return match adapter_supports_thinking {
            Some(true) => thinking_cap(
                true,
                "适配包声明的思考开关",
                ThinkingParamFamily::AdapterBoolean,
            ),
            _ => thinking_unsupported(),
        };
    }
    if pk == TEXT_KIND_QWEN && is_qwen3_family(&m) {
        return thinking_cap(
            true,
            "Qwen3 混合思考（enable_thinking）",
            ThinkingParamFamily::QwenEnableThinking,
        );
    }
    if pk == TEXT_KIND_LOCAL {
        return thinking_unsupported();
    }
    if pk == "text_deepseek_compat" && (is_deepseek_v4(&m) || is_deepseek_flash(&m)) {
        return thinking_cap(
            true,
            "DeepSeek V4（thinking.type）",
            ThinkingParamFamily::DeepSeekThinkingObject,
        );
    }
    if pk == TEXT_KIND_MINIMAX && is_minimax_m3(&m) {
        return thinking_cap(
            true,
            "MiniMax-M3 Adaptive Thinking",
            ThinkingParamFamily::MinimaxAdaptiveThinking,
        );
    }
    if matches!(
        pk,
        "text_openai_compat" | "text_custom" | "text_volc_ark_compat"
    ) {
        if is_gpt_oss(&m) {
            return thinking_cap(
                true,
                "GPT-OSS（reasoning_effort）",
                ThinkingParamFamily::GptOssReasoningEffort,
            );
        }
        if is_qwen3_family(&m) {
            return thinking_cap(
                true,
                "Qwen3 OpenAI 兼容（enable_thinking）",
                ThinkingParamFamily::QwenEnableThinking,
            );
        }
        if pk == "text_volc_ark_compat" && is_deepseek_v4(&m) {
            return thinking_cap(
                true,
                "火山 DeepSeek V4（thinking.type）",
                ThinkingParamFamily::DeepSeekThinkingObject,
            );
        }
    }
    thinking_unsupported()
}

/// Local-model whitelist against name and path file name / stem, never a bare UUID.
pub fn detect_local_thinking_capability(
    name: &str,
    model_path_file_name: Option<&str>,
    model_path_file_stem: Option<&str>,
) -> ThinkingCapability {
    let name = name.trim().to_ascii_lowercase();
    let path_name = model_path_file_name
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let path_stem = model_path_file_stem
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    for label in [&name, &path_name, &path_stem] {
        if label.is_empty() {
            continue;
        }
        if is_qwen3_family(label) {
            return thinking_cap(
                true,
                "本地 Qwen3（enable_thinking）",
                ThinkingParamFamily::QwenEnableThinking,
            );
        }
        if is_gpt_oss(label) {
            return thinking_cap(
                true,
                "本地 GPT-OSS（reasoning_effort）",
                ThinkingParamFamily::GptOssReasoningEffort,
            );
        }
    }
    thinking_unsupported()
}

fn is_qwen3_family(m: &str) -> bool {
    m.starts_with("qwen3") || m.contains("qwen3.") || m.contains("qwen3-") || m.contains("qwen3_")
}

fn is_gpt_oss(m: &str) -> bool {
    m.starts_with("gpt-oss") || m.contains("gpt-oss")
}

fn is_deepseek_v4(m: &str) -> bool {
    m.starts_with("deepseek-v4") || m.contains("deepseek-v4")
}

fn is_deepseek_flash(m: &str) -> bool {
    m.starts_with("deepseek-flash")
}

fn is_minimax_m3(m: &str) -> bool {
    m == "minimax-m3" || m.starts_with("minimax-m3")
}

pub fn is_thinking_related_error(error_body: &str) -> bool {
    let lower = error_body.to_ascii_lowercase();
    if lower.contains("enable_thinking")
        || lower.contains("reasoning_effort")
        || lower.contains("\"thinking\"")
    {
        return true;
    }
    if lower.contains("thinking")
        && (lower.contains("not support") || lower.contains("unsupported"))
        && (lower.contains("param")
            || lower.contains("parameter")
            || lower.contains("argument")
            || lower.contains("field"))
    {
        return true;
    }
    false
}

pub const THINKING_INCOMPATIBLE_PREFIX: &str = "thinking_mode_incompatible:";

pub fn wrap_thinking_incompatible_error(original: &str) -> String {
    format!("{THINKING_INCOMPATIBLE_PREFIX} {original}")
}

pub const EMPTY_BODY_TRUNCATED_ZH: &str = "输出在正文写出前已达到服务上限，本次未生成结果";

pub fn is_provider_output_truncated(reason: Option<&str>) -> bool {
    match reason.map(str::trim).filter(|s| !s.is_empty()) {
        Some(fr) => {
            let lower = fr.to_ascii_lowercase();
            lower == "length"
                || lower == "max_tokens"
                || lower == "max_output_tokens"
                || lower == "output_truncated"
        }
        None => false,
    }
}

pub fn empty_text_user_message(official_stop: Option<&str>) -> String {
    if is_provider_output_truncated(official_stop) {
        EMPTY_BODY_TRUNCATED_ZH.to_string()
    } else {
        "模型没有返回可用正文（仅有思考过程或内容为空）。".into()
    }
}

/// Render the host HTTP error line. `StatusCode` display is required
/// (`HTTP 401 Unauthorized: ...`), not a bare numeric status.
pub fn http_status_body_line(status: u16, body: &str) -> String {
    let status = reqwest::StatusCode::from_u16(status)
        .map(|code| code.to_string())
        .unwrap_or_else(|_| status.to_string());
    format!("HTTP {status}: {body}")
}

/// Default decoration is only the HTTP status/body line (not a delivery mapping).
/// Production should pass [`crate::types::TextHttpErrorMapper`] so the host
/// `provider_error` / `__BGX_ERR_JSON__` wire is preserved.
pub fn decorate_text_http_error(kind: &str, status: u16, body: &str) -> String {
    decorate_text_http_error_mapped(kind, status, body, None)
}

pub fn decorate_text_http_error_mapped(
    kind: &str,
    status: u16,
    body: &str,
    mapper: Option<&(dyn Fn(&str, u16, &str) -> String + Send + Sync)>,
) -> String {
    match mapper {
        Some(mapper) => mapper(kind, status, body),
        None => http_status_body_line(status, body),
    }
}

/// Crate-local mapper equivalent to host `to_user_facing_error("text", kind, raw)`.
/// Host production may inject this or its own `provider_error` wrapper.
pub fn map_text_http_error_to_user_facing_wire(kind: &str, status: u16, body: &str) -> String {
    crate::openai::provider_error::to_user_facing_error(
        "text",
        kind,
        http_status_body_line(status, body),
    )
}

pub fn maybe_wrap_thinking_error(
    thinking: Option<ThinkingRequest>,
    body: &str,
    err: String,
) -> String {
    if thinking.is_some() && is_thinking_related_error(body) {
        wrap_thinking_incompatible_error(&err)
    } else {
        err
    }
}

#[cfg(test)]
pub fn block_on_ready<T>(fut: impl Future<Output = T>) -> T {
    use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

    fn dummy_raw_waker() -> RawWaker {
        fn no_op(_: *const ()) {}
        fn clone(_: *const ()) -> RawWaker {
            dummy_raw_waker()
        }
        static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, no_op, no_op, no_op);
        RawWaker::new(std::ptr::null(), &VTABLE)
    }

    let waker = unsafe { Waker::from_raw(dummy_raw_waker()) };
    let mut cx = Context::from_waker(&waker);
    let mut fut = std::pin::pin!(fut);
    match fut.as_mut().poll(&mut cx) {
        Poll::Ready(value) => value,
        Poll::Pending => panic!("test future was not immediately ready"),
    }
}

#[cfg(test)]
#[derive(Clone, Default)]
pub struct FakePoster {
    pub inner: std::sync::Arc<FakePosterState>,
}

#[cfg(test)]
pub struct FakePosterState {
    pub requests: std::sync::Mutex<Vec<PreparedJsonPost>>,
    pub response: std::sync::Mutex<HttpTextResponse>,
}

#[cfg(test)]
impl Default for FakePosterState {
    fn default() -> Self {
        Self {
            requests: std::sync::Mutex::new(Vec::new()),
            response: std::sync::Mutex::new(HttpTextResponse {
                status: 0,
                body: String::new(),
            }),
        }
    }
}

#[cfg(test)]
impl FakePoster {
    pub fn new(status: u16, body: impl Into<String>) -> Self {
        let poster = Self::default();
        *poster.inner.response.lock().expect("fake poster response") = HttpTextResponse {
            status,
            body: body.into(),
        };
        poster
    }

    pub fn recorded(&self) -> Vec<PreparedJsonPost> {
        self.inner
            .requests
            .lock()
            .expect("fake poster requests")
            .clone()
    }
}

#[cfg(test)]
impl TextHttpPoster for FakePoster {
    fn post_json(&self, request: PreparedJsonPost, _timeout_secs: Option<u64>) -> PostFuture {
        let state = self.inner.clone();
        Box::pin(async move {
            state
                .requests
                .lock()
                .expect("fake poster requests")
                .push(request);
            let response = state
                .response
                .lock()
                .expect("fake poster response")
                .clone();
            Ok(response)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_api_key_bearer_and_bom() {
        assert_eq!(normalize_api_key_for_bearer("Bearer abc"), "abc");
        assert_eq!(normalize_api_key_for_bearer("  bearer  tok  "), "tok");
        assert_eq!(normalize_api_key_for_bearer("sk-no-prefix"), "sk-no-prefix");
        assert_eq!(normalize_api_key_for_bearer("Bearer Bearer sk-x"), "sk-x");
        assert_eq!(normalize_api_key_for_bearer("\u{FEFF}Bearer sk-x"), "sk-x");
        assert_eq!(normalize_api_key_for_bearer("\u{200B}sk-x\u{200B}"), "sk-x");
        assert_eq!(
            normalize_api_key_for_bearer("\u{201C}sk-inner\u{201D}"),
            "sk-inner"
        );
        assert_eq!(normalize_api_key_for_bearer("「tok-here」"), "tok-here");
        assert_eq!(
            normalize_api_key_for_bearer("sk-line\nbreak"),
            "sk-linebreak"
        );
    }

    #[test]
    fn long_running_connect_timeout_is_30s() {
        assert_eq!(CONNECT_TIMEOUT, Duration::from_secs(30));
        let fp = crate::openai::outbound_http::ClientFingerprint::long_running(None);
        assert_eq!(fp.connect_timeout_ms, CONNECT_TIMEOUT.as_millis() as u64);
        assert!(!fp.bypass_proxy);
        let loopback = fp.for_destination_url(&"http://127.0.0.1:11434/".parse().unwrap());
        assert!(loopback.bypass_proxy);
    }

    #[test]
    fn empty_provider_defaults_to_openai_compat() {
        let svc = TextServiceConfig::default();
        assert_eq!(svc.provider_kind(), "text_openai_compat");
    }

    #[test]
    fn request_timeout_only_when_positive() {
        let mut svc = TextServiceConfig::default();
        assert_eq!(svc.request_timeout(), None);
        svc.timeout_secs = Some(0);
        assert_eq!(svc.request_timeout(), None);
        svc.timeout_secs = Some(120);
        assert_eq!(svc.request_timeout(), Some(Duration::from_secs(120)));
    }

    #[test]
    fn qwen3_hybrid_supported() {
        let cap = detect_thinking_capability("text_qwen", "qwen3.7-max");
        assert!(cap.supported);
        assert!(matches!(
            cap.param_family,
            Some(ThinkingParamFamily::QwenEnableThinking)
        ));
    }

    #[test]
    fn thinking_error_detection_is_narrow() {
        assert!(is_thinking_related_error(
            "enable_thinking is not supported for this model"
        ));
        assert!(!is_thinking_related_error(
            "thinking about images is not supported"
        ));
        assert!(!is_thinking_related_error(
            "unsupported parameter: max_tokens"
        ));
    }

    #[test]
    fn default_decorate_keeps_status_reason_and_body() {
        let body =
            r#"{"error":{"message":"Incorrect API key provided","type":"invalid_request_error"}}"#;
        let out = decorate_text_http_error("text_openai_compat", 401, body);
        assert_eq!(out, format!("HTTP 401 Unauthorized: {body}"));
        assert!(!out.starts_with(crate::openai::user_facing_api_error::USER_FACING_API_ERROR_PREFIX));
    }

    #[test]
    fn maps_text_401_to_user_facing_wire() {
        let body =
            r#"{"error":{"message":"Incorrect API key provided","type":"invalid_request_error"}}"#;
        let out = map_text_http_error_to_user_facing_wire("text_openai_compat", 401, body);
        assert!(out.starts_with(crate::openai::user_facing_api_error::USER_FACING_API_ERROR_PREFIX));
        assert_eq!(
            crate::openai::provider_error::parse_wire_category(&out).as_deref(),
            Some("auth_failed")
        );
        assert!(out.contains("HTTP 401 Unauthorized:"));
    }

    #[test]
    fn maps_text_429_rate_limit() {
        let body = r#"{"error":{"message":"Rate limit reached","type":"rate_limit_error"}}"#;
        let out = map_text_http_error_to_user_facing_wire("text_openai_compat", 429, body);
        assert!(out.starts_with(crate::openai::user_facing_api_error::USER_FACING_API_ERROR_PREFIX));
        assert_eq!(
            crate::openai::provider_error::parse_wire_category(&out).as_deref(),
            Some("rate_limited")
        );
    }

    #[test]
    fn f1_execution_contract_policy_p05_text_401_and_403_are_auth_failed() {
        let body401 =
            r#"{"error":{"message":"Incorrect API key provided","type":"invalid_request_error"}}"#;
        let out401 = decorate_text_http_error_mapped(
            "text_openai_compat",
            401,
            body401,
            Some(&map_text_http_error_to_user_facing_wire),
        );
        assert!(out401.starts_with(crate::openai::user_facing_api_error::USER_FACING_API_ERROR_PREFIX));
        assert_eq!(
            crate::openai::provider_error::parse_wire_category(&out401).as_deref(),
            Some("auth_failed")
        );

        let body403 = r#"{"error":{"message":"Access denied","type":"permission_error"}}"#;
        let out403 = decorate_text_http_error_mapped(
            "text_openai_compat",
            403,
            body403,
            Some(&map_text_http_error_to_user_facing_wire),
        );
        assert!(out403.starts_with(crate::openai::user_facing_api_error::USER_FACING_API_ERROR_PREFIX));
        assert_eq!(
            crate::openai::provider_error::parse_wire_category(&out403).as_deref(),
            Some("auth_failed")
        );
        // SDK-facing wire must not echo vendor HTTP codes as product speech.
        assert!(!out403.contains("450000"));
        assert!(!out401.contains("sk-"));
    }
}
