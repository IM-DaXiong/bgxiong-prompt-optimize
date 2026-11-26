use serde::{Deserialize, Serialize};

/// Stable machine code plus existing user-facing message. No secrets, no serde column numbers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OptimizeError {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_reply: Option<String>,
    #[serde(skip)]
    pub compiled_video_candidate: Option<String>,
}

impl OptimizeError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            raw_reply: None,
            compiled_video_candidate: None,
        }
    }

    pub fn with_raw_reply(mut self, raw_reply: impl Into<String>) -> Self {
        self.raw_reply = Some(raw_reply.into());
        self
    }
}

pub mod error_codes {
    pub const OPTIMIZE_CANCELLED: &str = "OPTIMIZE_CANCELLED";
    pub const OPTIMIZE_TIMEOUT: &str = "OPTIMIZE_TIMEOUT";
    pub const IMAGE_PROMPT_USER_INPUT_REQUIRED: &str = "IMAGE_PROMPT_USER_INPUT_REQUIRED";
    pub const IMAGE_PROMPT_OUTPUT_INVALID: &str = "IMAGE_PROMPT_OUTPUT_INVALID";
    pub const AGENT_SKILL_DIGEST_MISMATCH: &str = "AGENT_SKILL_DIGEST_MISMATCH";
    pub const AGENT_SKILL_DIGEST_REQUIRED: &str = "AGENT_SKILL_DIGEST_REQUIRED";
    pub const INTERNAL_PROMPT_CONFIG: &str = "INTERNAL_PROMPT_CONFIG";
    pub const VIDEO_PROMPT_PROJECTOR_MISSING: &str = "VIDEO_PROMPT_PROJECTOR_MISSING";
    pub const VIDEO_PROMPT_INPUT_MODE_UNSUPPORTED: &str = "VIDEO_PROMPT_INPUT_MODE_UNSUPPORTED";
    /// Frozen target names a reference id with no inventory row. Format has not started.
    pub const VIDEO_PROMPT_MODEL_REFERENCE_INVALID: &str = "VIDEO_PROMPT_MODEL_REFERENCE_INVALID";
    pub const VIDEO_PROMPT_REFERENCE_ROW_MISSING: &str = "VIDEO_PROMPT_REFERENCE_ROW_MISSING";
    /// The same frozen reference id has two inventory rows that are not the same data.
    pub const VIDEO_PROMPT_REFERENCE_ROW_CONFLICT: &str = "VIDEO_PROMPT_REFERENCE_ROW_CONFLICT";
    /// Expression draft exists, but the shared review request could not be assembled.
    pub const VIDEO_PROMPT_FORMAT_REVIEW_PREPARE_FAILED: &str =
        "VIDEO_PROMPT_FORMAT_REVIEW_PREPARE_FAILED";
    pub const SKILL_LIBRARY_BUSY: &str = "SKILL_LIBRARY_BUSY";
    /// Coordinator supplement 2026-11-16; channel A over-cap. Do not reuse USER_INPUT_REQUIRED.
    pub const IMAGE_PROMPT_REFERENCE_LIMIT: &str = "IMAGE_PROMPT_REFERENCE_LIMIT";
}
