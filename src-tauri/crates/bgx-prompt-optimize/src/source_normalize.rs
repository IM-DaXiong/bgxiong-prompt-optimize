//! Isolated source-normalize model call for H3 / video source understanding.
//!
//! The caller already rendered messages and owns the closed-set schema. This
//! runner only freezes the transport invariants (JSON mode, required schema,
//! no images, the given stage id) and calls the text completer. It is not
//! run_image and not the full run_video_optimize pipeline.

use serde_json::Value;

use crate::error::{error_codes, OptimizeError};
use crate::text_port::{
    no_cancel, TextCall, TextCompleter, TextImageCountPolicy, TextMessage,
};

/// Default production source-understanding step id. Callers may pass a more
/// specific sub-step (for example lyric understanding) in step_label.
pub const STAGE_SOURCE_NORMALIZE: &str = "source_normalize";

/// Already-rendered source-normalize request. No DbState, no template render.
pub struct SourceNormalizeRequest {
    pub messages: Vec<TextMessage>,
    /// Caller closed-set JSON schema; forwarded to the completer unchanged.
    pub schema: Value,
    pub step_label: String,
}

/// Raw model reply. Schema decode and review rejection stay in the host/caller.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceNormalizeResult {
    pub content: String,
    pub finish_reason: Option<String>,
    pub raw_json: Option<Value>,
}

/// Run one source-normalize (or lyric-understand) completion through the text port.
pub fn run_source_normalize<C: TextCompleter>(
    request: SourceNormalizeRequest,
    completer: &mut C,
) -> Result<SourceNormalizeResult, OptimizeError> {
    if request.schema.is_null() {
        return Err(OptimizeError::new(
            error_codes::INTERNAL_PROMPT_CONFIG,
            "source_normalize requires the caller's closed-set schema",
        ));
    }
    let step_label = if request.step_label.trim().is_empty() {
        STAGE_SOURCE_NORMALIZE.to_string()
    } else {
        request.step_label
    };
    completer.check_cancel()?;
    let reply = completer.complete(TextCall {
        messages: request.messages,
        json_mode: true,
        schema: Some(request.schema),
        step_label,
        check_cancel: no_cancel,
        image_count_policy: TextImageCountPolicy::ModelCapability,
    })?;
    completer.check_cancel()?;
    Ok(SourceNormalizeResult {
        content: reply.content,
        finish_reason: reply.finish_reason,
        raw_json: reply.raw_json,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text_port::{cancelled_error, TextReply};
    use serde_json::json;
    use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

    struct RecordingCompleter {
        called: AtomicBool,
        json_mode: AtomicBool,
        step_label: std::sync::Mutex<String>,
        schema: std::sync::Mutex<Option<Value>>,
        policy: std::sync::Mutex<Option<TextImageCountPolicy>>,
        reply: Result<TextReply, OptimizeError>,
        hook: fn() -> Result<(), OptimizeError>,
    }

    impl TextCompleter for RecordingCompleter {
        fn check_cancel(&self) -> Result<(), OptimizeError> {
            (self.hook)()
        }

        fn complete(&mut self, call: TextCall) -> Result<TextReply, OptimizeError> {
            self.called.store(true, Ordering::SeqCst);
            self.json_mode.store(call.json_mode, Ordering::SeqCst);
            *self.step_label.lock().unwrap() = call.step_label.clone();
            *self.schema.lock().unwrap() = call.schema.clone();
            *self.policy.lock().unwrap() = Some(call.image_count_policy);
            match &self.reply {
                Ok(reply) => Ok(reply.clone()),
                Err(err) => Err(err.clone()),
            }
        }
    }

    fn closed_schema() -> Value {
        json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["facts"],
            "properties": {
                "facts": {"type": "array"}
            }
        })
    }

    fn request(schema: Value, step: &str, content: &str) -> SourceNormalizeRequest {
        SourceNormalizeRequest {
            messages: vec![TextMessage {
                role: "user".into(),
                content: json!(content),
            }],
            schema,
            step_label: step.into(),
        }
    }

    fn ok_reply(content: &str) -> Result<TextReply, OptimizeError> {
        Ok(TextReply {
            content: content.into(),
            finish_reason: Some("stop".into()),
            raw_json: None,
        })
    }

    fn completer(reply: Result<TextReply, OptimizeError>) -> RecordingCompleter {
        RecordingCompleter {
            called: AtomicBool::new(false),
            json_mode: AtomicBool::new(false),
            step_label: std::sync::Mutex::new(String::new()),
            schema: std::sync::Mutex::new(None),
            policy: std::sync::Mutex::new(None),
            reply,
            hook: no_cancel,
        }
    }

    #[test]
    fn forwards_caller_schema_json_mode_and_stage_id() {
        let schema = closed_schema();
        let raw = r#"{"facts":[]}"#;
        let mut completer = completer(ok_reply(raw));
        let result = run_source_normalize(
            request(schema.clone(), STAGE_SOURCE_NORMALIZE, "原稿"),
            &mut completer,
        )
        .unwrap();
        assert!(completer.called.load(Ordering::SeqCst));
        assert!(completer.json_mode.load(Ordering::SeqCst));
        assert_eq!(
            completer.step_label.lock().unwrap().as_str(),
            STAGE_SOURCE_NORMALIZE
        );
        assert_eq!(completer.schema.lock().unwrap().as_ref(), Some(&schema));
        assert_eq!(
            *completer.policy.lock().unwrap(),
            Some(TextImageCountPolicy::ModelCapability)
        );
        assert_eq!(result.content, raw);
        assert_eq!(result.finish_reason.as_deref(), Some("stop"));
    }

    #[test]
    fn preserves_lyric_sub_step_and_does_not_rewrite_schema() {
        let schema = json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["sungSpans"],
            "properties": {"sungSpans": {"type": "array"}}
        });
        let mut completer = completer(ok_reply(r#"{"sungSpans":[]}"#));
        run_source_normalize(
            request(schema.clone(), "source_lyric_understand", "歌词"),
            &mut completer,
        )
        .unwrap();
        assert_eq!(
            completer.step_label.lock().unwrap().as_str(),
            "source_lyric_understand"
        );
        assert_eq!(completer.schema.lock().unwrap().as_ref(), Some(&schema));
    }

    #[test]
    fn returns_raw_content_without_decoding() {
        let raw = r#"{"facts":["not-validated-here"]}"#;
        let mut completer = completer(ok_reply(raw));
        let result =
            run_source_normalize(request(closed_schema(), STAGE_SOURCE_NORMALIZE, "原稿"), &mut completer)
                .unwrap();
        assert_eq!(result.content, raw);
    }

    #[test]
    fn empty_step_label_defaults_to_source_normalize() {
        let mut completer = completer(ok_reply("{}"));
        run_source_normalize(request(closed_schema(), "  ", "原稿"), &mut completer).unwrap();
        assert_eq!(
            completer.step_label.lock().unwrap().as_str(),
            STAGE_SOURCE_NORMALIZE
        );
    }

    #[test]
    fn null_schema_is_rejected_before_complete() {
        let mut completer = completer(ok_reply("{}"));
        let err = run_source_normalize(request(Value::Null, STAGE_SOURCE_NORMALIZE, "原稿"), &mut completer)
            .unwrap_err();
        assert_eq!(err.code, error_codes::INTERNAL_PROMPT_CONFIG);
        assert!(!completer.called.load(Ordering::SeqCst));
    }

    fn cancel_now() -> Result<(), OptimizeError> {
        Err(cancelled_error())
    }

    #[test]
    fn cancel_before_complete_does_not_read_reply() {
        let mut completer = completer(ok_reply("MUST_NOT_BE_READ"));
        completer.hook = cancel_now;
        let err = run_source_normalize(
            request(closed_schema(), STAGE_SOURCE_NORMALIZE, "原稿"),
            &mut completer,
        )
        .unwrap_err();
        assert_eq!(err.code, error_codes::OPTIMIZE_CANCELLED);
        assert!(!completer.called.load(Ordering::SeqCst));
    }

    static CANCEL_AFTER_COMPLETE: AtomicU8 = AtomicU8::new(0);
    fn cancel_after_first_ok() -> Result<(), OptimizeError> {
        let n = CANCEL_AFTER_COMPLETE.fetch_add(1, Ordering::SeqCst);
        if n == 0 {
            Ok(())
        } else {
            Err(cancelled_error())
        }
    }

    #[test]
    fn cancel_after_complete_does_not_return_content() {
        CANCEL_AFTER_COMPLETE.store(0, Ordering::SeqCst);
        let mut completer = completer(ok_reply("不该交给调用方"));
        completer.hook = cancel_after_first_ok;
        let err = run_source_normalize(
            request(closed_schema(), STAGE_SOURCE_NORMALIZE, "原稿"),
            &mut completer,
        )
        .unwrap_err();
        assert_eq!(err.code, error_codes::OPTIMIZE_CANCELLED);
        assert!(completer.called.load(Ordering::SeqCst));
    }
}
