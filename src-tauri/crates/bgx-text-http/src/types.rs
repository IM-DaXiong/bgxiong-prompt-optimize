//! Public text-http types. Canonical definitions live in `openai::shared`
//! (extracted from the existing host, not a simplified rewrite).
//!
//! The earlier skeleton types `TextHttpPart` / `TextHttpMultimodalMessage` /
//! `TextHttpReply` are retired: they only had `Text` and `ImageDataUrl`, dropped
//! video parts and `reasoning_content`, and must not be used by the host or shell.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub use crate::adapter::{AdapterCapabilityEntry, AdapterPackage, AdapterTextRunner, Selection};
pub use crate::local_gguf::{
    LocalModelView, LocalTextGenerationRequest, LocalTextGenerationResponse, LocalTextRuntime,
};
pub use crate::minimax::StructuredBodyHook;
pub use crate::openai::shared::{
    ChatCompletionOutput, ChatMessage, ChatMessageMultimodal, HttpTextResponse, LlmContentPart,
    LlmImageUrl, LlmVideoUrl, PreparedJsonPost, ReqwestPoster, TextHttpPoster, TextImageCountPolicy,
    TextServiceConfig, TextTransportContent, TextTransportMessage, ThinkingCapability,
    ThinkingParamFamily, ThinkingRequest, CONNECT_TIMEOUT, MAX_CHAT_COMPLETION_POSTS,
    MAX_TEXT_RESPONSE_SIZE, OPENAI_COMPAT_KINDS, TEXT_KIND_ADAPTER, TEXT_KIND_LOCAL,
    TEXT_KIND_MINIMAX, TEXT_KIND_QWEN, VENDOR_ADAPTER_MODEL_ID_PREFIX,
};

/// Compatibility alias. The canonical type is [`TextServiceConfig`], which includes
/// `provider` (volc / deepseek / custom / qwen / minimax / adapter / local) and
/// optional `base_url`. `timeout_secs` is the whole-request timeout and is applied
/// only when it is a positive number. The 30-second connect timeout is
/// [`CONNECT_TIMEOUT`] inside the crate exits, not a config field.
pub type TextHttpConfig = TextServiceConfig;

/// Host or shell supplies package bytes. This crate never calls `std::fs`.
///
/// `package_root` remains an identity/root argument so the crate can join
/// package-relative paths; every read goes through this port.
pub trait PackageFileReader: Send + Sync {
    fn read(&self, path: &Path) -> Result<Vec<u8>, String>;

    /// Default existence check is a read. Host/shell implementations should
    /// override this with a cheap `is_file` that still does not live in this crate.
    fn is_file(&self, path: &Path) -> bool {
        self.read(path).is_ok()
    }
}

/// In-memory reader for tests and for a host/shell that already loaded the tree.
#[derive(Clone, Debug, Default)]
pub struct MemoryPackageFileReader {
    files: HashMap<PathBuf, Vec<u8>>,
}

impl MemoryPackageFileReader {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, path: impl AsRef<Path>, bytes: Vec<u8>) -> &mut Self {
        self.files.insert(path.as_ref().to_path_buf(), bytes);
        self
    }
}

impl PackageFileReader for MemoryPackageFileReader {
    fn read(&self, path: &Path) -> Result<Vec<u8>, String> {
        self.files
            .get(path)
            .cloned()
            .ok_or_else(|| format!("{}: package asset not found", path.display()))
    }

    fn is_file(&self, path: &Path) -> bool {
        self.files.contains_key(path)
    }
}

/// Parameters retained from the existing host adapter text exit.
/// wasmtime / process execution stays in the host via [`AdapterTextRunner`].
pub struct AdapterCompleteOptions<'a> {
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub thinking: Option<ThinkingRequest>,
    pub response_format_json: bool,
    pub has_output_schema: bool,
    pub on_audit: Option<&'a dyn Fn(&str)>,
}

impl Default for AdapterCompleteOptions<'_> {
    fn default() -> Self {
        Self {
            temperature: None,
            max_tokens: None,
            thinking: None,
            response_format_json: false,
            has_output_schema: false,
            on_audit: None,
        }
    }
}

/// Optional hook so the host can keep `__BGX_ERR_JSON__` / `provider_error` mapping.
/// Default crate decoration is only `HTTP {status}: {body}` and is not a delivery mapping.
pub type TextHttpErrorMapper = dyn Fn(&str, u16, &str) -> String + Send + Sync;

/// SSOT mapping used when the caller already knows whether the channel is as-submitted.
/// `AsSubmitted` must not be defaulted to `ModelCapability`.
pub fn image_count_policy_from_submitted_channel(as_submitted: bool) -> TextImageCountPolicy {
    if as_submitted {
        TextImageCountPolicy::AsSubmitted
    } else {
        TextImageCountPolicy::ModelCapability
    }
}