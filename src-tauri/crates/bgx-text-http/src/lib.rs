//! Shared text HTTP exits. Public contract is frozen in this file (integrator).
//! Module bodies are owned by lane B. This crate must not use `std::fs` or
//! `std::process` (crate-role `http-client`). wasmtime stays on the host.
//!
//! Goal: migrate the existing host exits, not a simplified replacement.
//! Production HTTP must be injected with [`TextHttpPoster`] backed by the host
//! `outbound_http` retry/proxy window. [`ReqwestPoster`] is only a crate-local
//! fallback and is not the production host path.

pub mod adapter;
pub mod local_gguf;
pub mod minimax;
pub mod openai;
pub mod qwen;
pub mod types;

pub use types::{
    AdapterCapabilityEntry, AdapterCompleteOptions, AdapterPackage, AdapterTextRunner,
    ChatCompletionOutput, ChatMessage, ChatMessageMultimodal, CONNECT_TIMEOUT, HttpTextResponse,
    LlmContentPart, LlmImageUrl, LlmVideoUrl, LocalModelView, LocalTextGenerationRequest,
    LocalTextGenerationResponse, LocalTextRuntime, MemoryPackageFileReader, PackageFileReader,
    PreparedJsonPost, ReqwestPoster, Selection, StructuredBodyHook, TEXT_KIND_ADAPTER,
    TEXT_KIND_LOCAL, TEXT_KIND_MINIMAX, TEXT_KIND_QWEN, TextHttpConfig, TextHttpErrorMapper,
    TextHttpPoster, TextImageCountPolicy, TextServiceConfig, TextTransportContent,
    TextTransportMessage, ThinkingCapability, ThinkingParamFamily, ThinkingRequest,
    VENDOR_ADAPTER_MODEL_ID_PREFIX, image_count_policy_from_submitted_channel,
};

pub use openai::{
    chat_completion, chat_completion_multimodal, chat_completion_multimodal_on,
    chat_completion_with_max_tokens, chat_completion_with_max_tokens_on,
};

pub use openai::shared::map_text_http_error_to_user_facing_wire;

pub use qwen::{
    QwenMessage, QwenOutput, qwen_chat_completion, qwen_chat_completion_multimodal,
    qwen_chat_completion_multimodal_on, qwen_chat_completion_on,
};

pub use minimax::{messages_completion_prepared, messages_completion_prepared_on};

use std::path::Path;

use serde_json::Value;

/// Frozen plan name for the MiniMax Anthropic Messages exit.
/// The implementation is [`messages_completion_prepared`]; thinking, multimodal
/// transport, audit, and structured-output hook must not be dropped.
pub async fn minimax_m3_messages_completion_prepared(
    service: &TextServiceConfig,
    messages: &[TextTransportMessage],
    temperature: Option<f32>,
    thinking: Option<ThinkingRequest>,
    max_tokens: Option<u32>,
    response_format_json: bool,
    on_audit: Option<&dyn Fn(&str)>,
) -> Result<ChatCompletionOutput, String> {
    messages_completion_prepared(
        service,
        messages,
        temperature,
        thinking,
        max_tokens,
        response_format_json,
        on_audit,
    )
    .await
}

/// Frozen plan name for local GGUF text. Synchronous, matching the host port.
/// The host/shell owns the runtime lifecycle and passes occupancy explicitly.
pub fn complete_local_text_from_transport_messages(
    runtime: &dyn LocalTextRuntime,
    model_id: Option<&str>,
    occupancy_label: &str,
    require_occupancy: &dyn Fn(&str) -> Result<(), String>,
    messages: &[TextTransportMessage],
    temperature: Option<f32>,
    response_format_json: bool,
    output_schema: Option<&Value>,
    thinking: Option<ThinkingRequest>,
    image_count_policy: TextImageCountPolicy,
) -> Result<(String, Option<String>), String> {
    local_gguf::complete_from_transport_messages(
        runtime,
        model_id,
        occupancy_label,
        require_occupancy,
        messages,
        temperature,
        response_format_json,
        output_schema,
        thinking,
        image_count_policy,
    )
}

/// Vendor-adapter text complete. Synchronous (host may wrap with `spawn_blocking`).
///
/// `package_root` is the adapter-v2 root identity. All file access goes through
/// `reader`. The runner (process / wasmtime) stays on the host.
pub fn complete(
    package_root: &Path,
    reader: &dyn PackageFileReader,
    service: &TextServiceConfig,
    messages: &[TextTransportMessage],
    options: AdapterCompleteOptions<'_>,
    runner: &dyn AdapterTextRunner,
) -> Result<ChatCompletionOutput, String> {
    adapter::complete(package_root, reader, service, messages, options, runner)
}