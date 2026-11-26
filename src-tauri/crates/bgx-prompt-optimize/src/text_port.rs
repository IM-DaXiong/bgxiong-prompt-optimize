use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{error_codes, OptimizeError};

/// Host chat message mapped from existing `ChatMessage`. No database fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TextMessage {
    pub role: String,
    pub content: Value,
}

/// Image-count policy for vision text exits. 1:1 with `bgx_text_http::TextImageCountPolicy`.
/// `AsSubmitted` must be set when the optimize channel is `VisionChannel::AsSubmitted`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TextImageCountPolicy {
    #[default]
    ModelCapability,
    AsSubmitted,
}

impl TextImageCountPolicy {
    pub fn from_vision_channel(channel: crate::image_job::VisionChannel) -> Self {
        match channel {
            crate::image_job::VisionChannel::AsSubmitted => Self::AsSubmitted,
            crate::image_job::VisionChannel::ReferenceOnly | crate::image_job::VisionChannel::Optional => Self::ModelCapability,
        }
    }

    pub fn as_submitted(self) -> bool {
        matches!(self, Self::AsSubmitted)
    }
}

/// One text completion request. `check_cancel` is a runtime callback and is not serialized.
pub struct TextCall {
    pub messages: Vec<TextMessage>,
    pub json_mode: bool,
    pub schema: Option<Value>,
    pub step_label: String,
    pub check_cancel: fn() -> Result<(), OptimizeError>,
    /// Required for Qwen/local vision. Never default `AsSubmitted` down to `ModelCapability`.
    pub image_count_policy: TextImageCountPolicy,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TextReply {
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_json: Option<Value>,
}

pub trait TextCompleter {
    fn complete(&mut self, call: TextCall) -> Result<TextReply, OptimizeError>;

    /// Frozen `run_*` signatures no longer take a cancel callback.
    /// Hosts should override this so pre/post `complete` checks still work.
    /// `TextCall.check_cancel` remains available inside `complete`.
    fn check_cancel(&self) -> Result<(), OptimizeError> {
        Ok(())
    }
}

/// Invoke the cancel callback. Runners must call this before and after `complete`.
pub fn ensure_not_cancelled(
    check_cancel: fn() -> Result<(), OptimizeError>,
) -> Result<(), OptimizeError> {
    check_cancel()
}

/// Callback that never cancels.
pub fn no_cancel() -> Result<(), OptimizeError> {
    Ok(())
}

/// Standard cancelled error. Message matches existing task-cancel wording.
pub fn cancelled_error() -> OptimizeError {
    OptimizeError::new(error_codes::OPTIMIZE_CANCELLED, "任务已取消")
}

/// Standard timeout error. The core does not sleep; the port returns this code.
pub fn timeout_error(message: impl Into<String>) -> OptimizeError {
    OptimizeError::new(error_codes::OPTIMIZE_TIMEOUT, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    fn cancel_now() -> Result<(), OptimizeError> {
        Err(cancelled_error())
    }

    struct RecordingCompleter {
        called: AtomicBool,
        reply: TextReply,
    }

    impl TextCompleter for RecordingCompleter {
        fn complete(&mut self, call: TextCall) -> Result<TextReply, OptimizeError> {
            self.called.store(true, Ordering::SeqCst);
            let _ = call.messages;
            Ok(self.reply.clone())
        }
    }

    /// Models the T07 cancel points without the full image runner.
    fn run_with_port(
        completer: &mut impl TextCompleter,
        check_cancel: fn() -> Result<(), OptimizeError>,
    ) -> Result<TextReply, OptimizeError> {
        ensure_not_cancelled(check_cancel)?;
        let reply = completer.complete(TextCall {
            messages: vec![TextMessage {
                role: "user".into(),
                content: Value::String("hello".into()),
            }],
            json_mode: true,
            schema: None,
            step_label: "test".into(),
            check_cancel,
            image_count_policy: TextImageCountPolicy::ModelCapability,
        })?;
        ensure_not_cancelled(check_cancel)?;
        Ok(reply)
    }

    #[test]
    fn cancelled_callback_does_not_read_a_reply() {
        let mut completer = RecordingCompleter {
            called: AtomicBool::new(false),
            reply: TextReply {
                content: "MUST_NOT_BE_READ".into(),
                finish_reason: Some("stop".into()),
                raw_json: None,
            },
        };
        let err = run_with_port(&mut completer, cancel_now).expect_err("must cancel");
        assert_eq!(err.code, error_codes::OPTIMIZE_CANCELLED);
        assert_eq!(err.message, "任务已取消");
        assert!(!completer.called.load(Ordering::SeqCst));
    }

    #[test]
    fn no_cancel_callback_allows_complete() {
        let mut completer = RecordingCompleter {
            called: AtomicBool::new(false),
            reply: TextReply {
                content: "{\"optimizedPrompt\":\"ok\"}".into(),
                finish_reason: Some("stop".into()),
                raw_json: None,
            },
        };
        let reply = run_with_port(&mut completer, no_cancel).expect("ok");
        assert!(completer.called.load(Ordering::SeqCst));
        assert_eq!(reply.content, "{\"optimizedPrompt\":\"ok\"}");
    }

    #[test]
    fn text_call_and_reply_fields_exist() {
        let call = TextCall {
            messages: vec![TextMessage {
                role: "system".into(),
                content: Value::String("s".into()),
            }],
            json_mode: false,
            schema: Some(serde_json::json!({"type":"object"})),
            step_label: "video-expression".into(),
            check_cancel: no_cancel,
            image_count_policy: TextImageCountPolicy::ModelCapability,
        };
        assert!(!call.json_mode);
        assert_eq!(call.step_label, "video-expression");
        assert!(call.schema.is_some());
        let reply = TextReply {
            content: "x".into(),
            finish_reason: None,
            raw_json: None,
        };
        let encoded = serde_json::to_value(&reply).unwrap();
        assert_eq!(encoded["content"], "x");
        assert!(encoded.get("finishReason").is_none());
    }
}