//! Synchronous `run_image` and `run_literary`.

use serde_json::{json, Value};

use crate::error::{error_codes, OptimizeError};
use crate::image_job::{
    assemble_messages, load_template, output_schema, parse_optimized_prompt, AssembleVisionFacts,
    ImageOptimizeRequest, ImageOptimizeResult, VisionChannel,
};
use crate::text_port::{TextCall, TextCompleter, TextMessage};
use crate::vision::{
    cap_optional_channel, decide, encode_image_bytes_for_llm, encode_original_image_bytes,
    reject_if_over_submitted_cap, VisionAttachDecision, VisionInputCaps, VISION_SKIP_NO_CANDIDATES,
    VISION_SKIP_NONE, VISION_SKIP_NO_VISION,
};

const IMAGE_STEP: &str = "image-prompt-optimize";

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

pub(crate) fn splice_last_user_image_parts(
    mut messages: Vec<TextMessage>,
    data_urls: &[String],
) -> Result<Vec<TextMessage>, OptimizeError> {
    if data_urls.is_empty() {
        return Ok(messages);
    }
    let last_user_idx = messages
        .iter()
        .rposition(|m| m.role.eq_ignore_ascii_case("user"))
        .ok_or_else(|| {
            OptimizeError::new(
                error_codes::INTERNAL_PROMPT_CONFIG,
                "prompt_optimize_vision: no user message to attach images",
            )
        })?;
    let text = match &messages[last_user_idx].content {
        Value::String(s) => s.clone(),
        other => other
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| other.to_string()),
    };
    let mut parts = vec![json!({"type": "text", "text": text})];
    for url in data_urls {
        let u = url.trim();
        if u.is_empty() {
            continue;
        }
        parts.push(json!({
            "type": "image_url",
            "image_url": {"url": u, "detail": "high"}
        }));
    }
    messages[last_user_idx].content = Value::Array(parts);
    Ok(messages)
}

struct PreparedVision {
    data_urls: Vec<String>,
    attached_image_count: u32,
    vision_truncated: bool,
    vision_skip_reason: String,
    attached_order: Vec<Value>,
    images_read: bool,
}

fn prepare_vision(request: &ImageOptimizeRequest) -> Result<PreparedVision, OptimizeError> {
    let count = u32::try_from(request.images.len()).map_err(|_| {
        OptimizeError::new(
            "IMAGE_PROMPT_REFERENCE_COUNT_INVALID",
            "IMAGE_PROMPT_REFERENCE_COUNT_INVALID",
        )
    })?;
    match request.channel {
        VisionChannel::ReferenceOnly => {
            reject_if_over_submitted_cap(request.reference_image_count)?;
            Ok(PreparedVision {
                data_urls: Vec::new(),
                attached_image_count: 0,
                vision_truncated: false,
                vision_skip_reason: VISION_SKIP_NONE.into(),
                attached_order: Vec::new(),
                images_read: false,
            })
        }
        VisionChannel::AsSubmitted => {
            reject_if_over_submitted_cap(count)?;
            let mut data_urls = Vec::new();
            let mut attached_order = Vec::new();
            for image in &request.images {
                let encoded = encode_original_image_bytes(&image.bytes)?;
                data_urls.push(encoded.data_url);
                attached_order.push(Value::from(image.declared_order));
            }
            Ok(PreparedVision {
                attached_image_count: count,
                vision_truncated: false,
                vision_skip_reason: VISION_SKIP_NONE.into(),
                attached_order,
                images_read: count > 0,
                data_urls,
            })
        }
        VisionChannel::Optional => {
            let (capped, truncated_ipc) = cap_optional_channel(request.images.clone());
            let take = u32::try_from(capped.len()).map_err(|_| {
                OptimizeError::new(
                    "IMAGE_PROMPT_REFERENCE_COUNT_INVALID",
                    "IMAGE_PROMPT_REFERENCE_COUNT_INVALID",
                )
            })?;
            if take == 0 {
                return Ok(PreparedVision {
                    data_urls: Vec::new(),
                    attached_image_count: 0,
                    vision_truncated: false,
                    vision_skip_reason: VISION_SKIP_NO_CANDIDATES.into(),
                    attached_order: Vec::new(),
                    images_read: false,
                });
            }
            let caps = VisionInputCaps::from(&request.text_vision);
            match decide(&caps, take) {
                VisionAttachDecision::TextOnly => Ok(PreparedVision {
                    data_urls: Vec::new(),
                    attached_image_count: 0,
                    vision_truncated: truncated_ipc,
                    vision_skip_reason: VISION_SKIP_NO_VISION.into(),
                    attached_order: Vec::new(),
                    images_read: false,
                }),
                VisionAttachDecision::Attach {
                    take: attach,
                    truncated,
                } => {
                    let mut data_urls = Vec::new();
                    let mut attached_order = Vec::new();
                    for image in capped.iter().take(attach as usize) {
                        let encoded = encode_image_bytes_for_llm(&image.bytes)?;
                        data_urls.push(encoded.data_url);
                        attached_order.push(Value::from(image.declared_order));
                    }
                    Ok(PreparedVision {
                        attached_image_count: attach,
                        vision_truncated: truncated || truncated_ipc,
                        vision_skip_reason: VISION_SKIP_NONE.into(),
                        attached_order,
                        images_read: attach > 0,
                        data_urls,
                    })
                }
            }
        }
    }
}

pub fn run_image<C: TextCompleter>(
    request: ImageOptimizeRequest,
    completer: &mut C,
) -> Result<ImageOptimizeResult, OptimizeError> {
    let _ = load_template(&request)?;
    let prepared = prepare_vision(&request)?;
    let facts = AssembleVisionFacts {
        attached_image_count: prepared.attached_image_count,
        attached_image_order: prepared.attached_order.clone(),
        images_read: prepared.images_read,
    };
    let messages = assemble_messages(&request, &facts)?;
    let messages = splice_last_user_image_parts(messages, &prepared.data_urls)?;
    let schema = output_schema()?;
    completer.check_cancel()?;
    let reply = completer.complete(TextCall {
        messages,
        json_mode: true,
        schema: Some(schema),
        step_label: if request.task_id.is_empty() {
            IMAGE_STEP.to_owned()
        } else {
            request.task_id.clone()
        },
        check_cancel: crate::text_port::no_cancel,
        image_count_policy: crate::text_port::TextImageCountPolicy::from_vision_channel(request.channel),
    })?;
    completer.check_cancel()?;
    match parse_optimized_prompt(&reply.content) {
        Ok(optimized_prompt) => Ok(ImageOptimizeResult {
            optimized_prompt,
            raw_reply: reply.content,
            finish_reason: reply.finish_reason.clone(),
            output_incomplete: is_provider_output_truncated(reply.finish_reason.as_deref()),
            attached_image_count: prepared.attached_image_count,
            vision_truncated: prepared.vision_truncated,
            vision_skip_reason: prepared.vision_skip_reason,
            input_mode: if prepared.attached_image_count > 0 {
                "vision".into()
            } else {
                "text".into()
            },
        }),
        Err(mut err) => {
            if err.raw_reply.is_none() {
                err.raw_reply = Some(reply.content.clone());
            }
            Err(err)
        }
    }
}

pub use crate::literary_job::run_literary;
pub use crate::video_job::run_video_expression;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image_job::{SubmittedImage, TextVisionCaps, VisionChannel};
    use crate::skill_material::FrozenSkillBytes;
    use crate::template::{freeze_from_files, pack_rules, Language};
    use crate::text_port::{cancelled_error, no_cancel, TextReply};
    use crate::vision::{MAX_IPC_RELS, VISION_SKIP_NO_VISION};
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

    const REQUEST_JSON: &str = include_str!(
        "../../bgx-internal-prompts/resources/internal-prompt-skills/image-prompt-optimize/request.json"
    );
    const SKILL_ZH: &str = include_str!(
        "../../bgx-internal-prompts/resources/internal-prompt-skills/image-prompt-optimize/SKILL.md"
    );
    const METHOD_EN: &str = include_str!(
        "../../bgx-internal-prompts/resources/internal-prompt-skills/image-prompt-optimize/prompts/method.en.md"
    );
    const GUIDANCE_ZH: &str = include_str!(
        "../../bgx-internal-prompts/resources/internal-prompt-skills/shared/request-guidance.zh.md"
    );
    const GUIDANCE_EN: &str = include_str!(
        "../../bgx-internal-prompts/resources/internal-prompt-skills/shared/request-guidance.en.md"
    );
    const CONTEXT_ZH: &str = include_str!(
        "../../bgx-internal-prompts/resources/internal-prompt-skills/shared/context-heading.zh.md"
    );
    const CONTEXT_EN: &str = include_str!(
        "../../bgx-internal-prompts/resources/internal-prompt-skills/shared/context-heading.en.md"
    );
    const OUTPUT_ZH: &str = include_str!(
        "../../bgx-internal-prompts/resources/internal-prompt-skills/shared/output-heading.zh.md"
    );
    const OUTPUT_EN: &str = include_str!(
        "../../bgx-internal-prompts/resources/internal-prompt-skills/shared/output-heading.en.md"
    );

    fn authored_files() -> BTreeMap<String, String> {
        BTreeMap::from([
            ("image-prompt-optimize/request.json".into(), REQUEST_JSON.to_owned()),
            ("image-prompt-optimize/SKILL.md".into(), SKILL_ZH.to_owned()),
            ("image-prompt-optimize/prompts/method.en.md".into(), METHOD_EN.to_owned()),
            ("shared/request-guidance.zh.md".into(), GUIDANCE_ZH.to_owned()),
            ("shared/request-guidance.en.md".into(), GUIDANCE_EN.to_owned()),
            ("shared/context-heading.zh.md".into(), CONTEXT_ZH.to_owned()),
            ("shared/context-heading.en.md".into(), CONTEXT_EN.to_owned()),
            ("shared/output-heading.zh.md".into(), OUTPUT_ZH.to_owned()),
            ("shared/output-heading.en.md".into(), OUTPUT_EN.to_owned()),
        ])
    }

    fn png_bytes(w: u32, h: u32) -> Vec<u8> {
        let img: image::ImageBuffer<image::Rgb<u8>, Vec<u8>> =
            image::ImageBuffer::from_fn(w, h, |_, _| image::Rgb([7u8, 8, 9]));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    fn base_request() -> ImageOptimizeRequest {
        let snapshot =
            freeze_from_files(Language::Zh, &["image-prompt-optimize"], &authored_files()).unwrap();
        ImageOptimizeRequest {
            task_id: "job-1".into(),
            user_text: "把人物换成红衣".into(),
            method: None,
            template: pack_rules(&snapshot).unwrap(),
            language: "zh".into(),
            reference_image_count: 0,
            images: Vec::new(),
            text_vision: TextVisionCaps {
                supports_vision: true,
            },
            channel: VisionChannel::AsSubmitted,
        }
    }

    struct ScriptCompleter {
        called: AtomicBool,
        reply: Result<TextReply, OptimizeError>,
        saw_images: AtomicBool,
        hook: fn() -> Result<(), OptimizeError>,
    }

    impl TextCompleter for ScriptCompleter {
        fn check_cancel(&self) -> Result<(), OptimizeError> {
            (self.hook)()
        }

        fn complete(&mut self, call: TextCall) -> Result<TextReply, OptimizeError> {
            self.called.store(true, Ordering::SeqCst);
            let has_image = call.messages.iter().any(|message| {
                message.content.as_array().is_some_and(|parts| {
                    parts.iter().any(|part| part["type"] == "image_url")
                })
            });
            self.saw_images.store(has_image, Ordering::SeqCst);
            match &self.reply {
                Ok(reply) => Ok(reply.clone()),
                Err(err) => Err(err.clone()),
            }
        }
    }

    fn ok_reply(content: &str, finish: &str) -> Result<TextReply, OptimizeError> {
        Ok(TextReply {
            content: content.into(),
            finish_reason: Some(finish.into()),
            raw_json: None,
        })
    }

    #[test]
    fn run_image_truncated_complete_json_is_usable() {
        let mut completer = ScriptCompleter {
            called: AtomicBool::new(false),
            reply: ok_reply(r#"{"optimizedPrompt":"红衣人物站在原背景前"}"#, "length"),
            saw_images: AtomicBool::new(false),
            hook: no_cancel,
        };
        let result = run_image(base_request(), &mut completer).unwrap();
        assert_eq!(result.optimized_prompt, "红衣人物站在原背景前");
        assert!(result.output_incomplete);
        assert_eq!(result.finish_reason.as_deref(), Some("length"));
        assert_eq!(result.input_mode, "text");
        assert!(completer.called.load(Ordering::SeqCst));
    }

    #[test]
    fn run_image_truncated_incomplete_json_keeps_raw() {
        let raw = r#"{"optimizedPrompt":"未写完"#;
        let mut completer = ScriptCompleter {
            called: AtomicBool::new(false),
            reply: ok_reply(raw, "max_tokens"),
            saw_images: AtomicBool::new(false),
            hook: no_cancel,
        };
        let err = run_image(base_request(), &mut completer).unwrap_err();
        assert_eq!(err.code, error_codes::IMAGE_PROMPT_OUTPUT_INVALID);
        assert_eq!(err.raw_reply.as_deref(), Some(raw));
    }

    fn cancel_now() -> Result<(), OptimizeError> {
        Err(cancelled_error())
    }

    #[test]
    fn run_image_cancel_before_complete_does_not_read_reply() {
        let mut completer = ScriptCompleter {
            called: AtomicBool::new(false),
            reply: ok_reply(r#"{"optimizedPrompt":"MUST_NOT_BE_READ"}"#, "stop"),
            saw_images: AtomicBool::new(false),
            hook: cancel_now,
        };
        let err = run_image(base_request(), &mut completer).unwrap_err();
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
    fn run_image_cancel_after_complete_does_not_return_prompt() {
        CANCEL_AFTER_COMPLETE.store(0, Ordering::SeqCst);
        let mut completer = ScriptCompleter {
            called: AtomicBool::new(false),
            reply: ok_reply(r#"{"optimizedPrompt":"不该交给调用方"}"#, "stop"),
            saw_images: AtomicBool::new(false),
            hook: cancel_after_first_ok,
        };
        let err = run_image(base_request(), &mut completer).unwrap_err();
        assert_eq!(err.code, error_codes::OPTIMIZE_CANCELLED);
        assert!(completer.called.load(Ordering::SeqCst));
    }

    #[test]
    fn run_image_as_submitted_attaches_original_image() {
        let mut request = base_request();
        request.reference_image_count = 1;
        request.images = vec![SubmittedImage {
            bytes: png_bytes(8, 8),
            declared_order: 1,
        }];
        let mut completer = ScriptCompleter {
            called: AtomicBool::new(false),
            reply: ok_reply(r#"{"optimizedPrompt":"看图成稿"}"#, "stop"),
            saw_images: AtomicBool::new(false),
            hook: no_cancel,
        };
        let result = run_image(request, &mut completer).unwrap();
        assert_eq!(result.input_mode, "vision");
        assert_eq!(result.attached_image_count, 1);
        assert!(!result.vision_truncated);
        assert!(completer.saw_images.load(Ordering::SeqCst));
    }

    #[test]
    fn run_image_optional_channel_caps_and_skips_without_vision() {
        let mut request = base_request();
        request.channel = VisionChannel::Optional;
        request.text_vision = TextVisionCaps {
            supports_vision: false,
        };
        request.reference_image_count = MAX_IPC_RELS + 1;
        request.images = (1..=MAX_IPC_RELS + 1)
            .map(|i| SubmittedImage {
                bytes: png_bytes(16, 16),
                declared_order: i,
            })
            .collect();
        let mut completer = ScriptCompleter {
            called: AtomicBool::new(false),
            reply: ok_reply(r#"{"optimizedPrompt":"纯文本成稿"}"#, "stop"),
            saw_images: AtomicBool::new(false),
            hook: no_cancel,
        };
        let result = run_image(request, &mut completer).unwrap();
        assert_eq!(result.input_mode, "text");
        assert_eq!(result.attached_image_count, 0);
        assert_eq!(result.vision_skip_reason, VISION_SKIP_NO_VISION);
        assert!(!completer.saw_images.load(Ordering::SeqCst));
    }

    #[test]
    fn run_image_as_submitted_rejects_over_cap_before_complete() {
        let mut request = base_request();
        request.images = (1..=MAX_IPC_RELS + 1)
            .map(|i| SubmittedImage {
                bytes: png_bytes(8, 8),
                declared_order: i,
            })
            .collect();
        let mut completer = ScriptCompleter {
            called: AtomicBool::new(false),
            reply: ok_reply(r#"{"optimizedPrompt":"x"}"#, "stop"),
            saw_images: AtomicBool::new(false),
            hook: no_cancel,
        };
        let err = run_image(request, &mut completer).unwrap_err();
        assert!(err.message.contains("上限"));
        assert!(!completer.called.load(Ordering::SeqCst));
    }
}