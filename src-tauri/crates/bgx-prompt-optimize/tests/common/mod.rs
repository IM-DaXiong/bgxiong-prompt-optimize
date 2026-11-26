//! Shared fixtures for T30 offline contract integration tests.
//! These helpers are not a second golden set; T34 goldens stay in `golden_video.rs`.
#![allow(dead_code)]

use std::collections::{BTreeMap, VecDeque};

use bgx_agent_skills_core::digest::package_digest;
use bgx_prompt_optimize::error::OptimizeError;
use bgx_prompt_optimize::skill_material::FrozenSkillBytes;
use bgx_prompt_optimize::template::{freeze_from_files, pack_rules, Language};
use bgx_prompt_optimize::text_port::{
    cancelled_error, TextCall, TextCompleter, TextImageCountPolicy, TextReply,
};
use bgx_prompt_optimize::video_job::VideoExpressionRequest;
use bgx_prompt_optimize::{draft_schema_for, SubmittedImage};
use bgx_video_prompt_contract::document::DocumentDraftV1;
use bgx_video_prompt_contract::projection::VideoPromptTargetSnapshot;
use bgx_video_prompt_contract::ProjectionClosedSet;
use serde_json::{json, Value};

const IMAGE_REQUEST_JSON: &str = include_str!(
    "../../../bgx-internal-prompts/resources/internal-prompt-skills/image-prompt-optimize/request.json"
);
const IMAGE_SKILL_ZH: &str = include_str!(
    "../../../bgx-internal-prompts/resources/internal-prompt-skills/image-prompt-optimize/SKILL.md"
);
const IMAGE_METHOD_EN: &str = include_str!(
    "../../../bgx-internal-prompts/resources/internal-prompt-skills/image-prompt-optimize/prompts/method.en.md"
);
const GUIDANCE_ZH: &str = include_str!(
    "../../../bgx-internal-prompts/resources/internal-prompt-skills/shared/request-guidance.zh.md"
);
const GUIDANCE_EN: &str = include_str!(
    "../../../bgx-internal-prompts/resources/internal-prompt-skills/shared/request-guidance.en.md"
);
const CONTEXT_ZH: &str = include_str!(
    "../../../bgx-internal-prompts/resources/internal-prompt-skills/shared/context-heading.zh.md"
);
const CONTEXT_EN: &str = include_str!(
    "../../../bgx-internal-prompts/resources/internal-prompt-skills/shared/context-heading.en.md"
);
const OUTPUT_ZH: &str = include_str!(
    "../../../bgx-internal-prompts/resources/internal-prompt-skills/shared/output-heading.zh.md"
);
const OUTPUT_EN: &str = include_str!(
    "../../../bgx-internal-prompts/resources/internal-prompt-skills/shared/output-heading.en.md"
);

pub const MOTHER_JSON: &str = r#"{"shots":[{"shotId":"s1","facts":[{"factId":"f1","kind":"action","text":"She walks.","semanticSlot":"action_interaction","polarity":"positive","provenance":"user_text","sourceFactId":null,"entityRefIds":[],"sourceUnitIds":["u1"]}],"quotes":[]}]}"#;

/// Official model-shaped author reply (no provenance / sourceFactId; those are filled by normalize).
pub const MOTHER_AUTHOR_REPLY: &str = r#"{"shots":[{"shotId":"s1","facts":[{"factId":"f1","kind":"action","text":"She walks.","semanticSlot":"action_interaction","polarity":"positive","entityRefIds":[],"sourceUnitIds":["u1"]}],"quotes":[]}]}"#;

pub const VALID_EXPRESSION_REPLY: &str = r#"{"lines":[{"shotId":"s1","factIds":["f1"],"quoteIds":[],"referenceUseIds":[],"text":"She walks."}],"conflicts":[]}"#;

pub const OUTSIDE_CLOSED_SET_REPLY: &str = r#"{"lines":[{"shotId":"s1","factIds":["not-a-fact"],"quoteIds":[],"referenceUseIds":[],"text":"x"}],"conflicts":[]}"#;

pub const CAPS_DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

pub fn pack_task_template(task: &str, request_json: &str) -> FrozenSkillBytes {
    let mut available = BTreeMap::new();
    available.insert(format!("{task}/request.json"), request_json.to_owned());
    let snapshot =
        freeze_from_files(Language::Zh, &[task], &available).expect("test template must freeze");
    pack_rules(&snapshot).expect("test template must pack")
}

pub fn expression_review_template() -> FrozenSkillBytes {
    pack_task_template(
        "video-expression-review",
        r#"{
            "formatVersion": 2,
            "task": "video-expression-review",
            "kind": "request",
            "messages": [
                {
                    "id": "user",
                    "role": "user",
                    "parts": [
                        { "input": "taskContext", "encoding": "json" },
                        { "input": "outputContract", "encoding": "json" }
                    ]
                }
            ]
        }"#,
    )
}

pub fn expression_template() -> FrozenSkillBytes {
    pack_task_template(
        "video-expression",
        r#"{
            "formatVersion": 2,
            "task": "video-expression",
            "kind": "request",
            "messages": [
                {
                    "id": "user",
                    "role": "user",
                    "parts": [
                        { "input": "taskContext", "encoding": "json" },
                        { "input": "outputContract", "encoding": "json" }
                    ]
                }
            ]
        }"#,
    )
}

pub fn content_template() -> FrozenSkillBytes {
    pack_task_template(
        "video-content-optimize",
        r#"{
            "formatVersion": 2,
            "task": "video-content-optimize",
            "kind": "request",
            "messages": [
                {
                    "id": "user",
                    "role": "user",
                    "parts": [
                        { "input": "taskContext", "encoding": "json" },
                        { "input": "outputContract", "encoding": "json" }
                    ]
                }
            ]
        }"#,
    )
}

pub fn format_method() -> FrozenSkillBytes {
    let body = "generic-video-prompt-writing";
    let files: [(&str, &[u8]); 1] = [("SKILL.md", body.as_bytes())];
    FrozenSkillBytes {
        stable_id: "generic-video-prompt-writing".into(),
        package_digest: package_digest(files),
        body: body.into(),
        resources: BTreeMap::from([("SKILL.md".into(), body.as_bytes().to_vec())]),
    }
}

pub fn mother() -> DocumentDraftV1 {
    serde_json::from_str(MOTHER_JSON).expect("fixture mother must deserialize")
}

pub fn caps_snapshot() -> Value {
    json!({ "capsDigest": CAPS_DIGEST })
}


pub fn frozen_method(profile_id: &str, profile_version: u64) -> bgx_video_prompt_contract::projection::VideoPromptFrozenMethod {
    serde_json::from_value(serde_json::json!({
        "selectionKind": "custom",
        "activationId": "act-test-format",
        "stableId": "generic-video-prompt-writing",
        "packageDigest": format_method().package_digest,
        "name": "generic-video-prompt-writing",
        "bodySha256": "a".repeat(64),
        "resources": [],
        "bindingProfileId": profile_id,
        "bindingProfileVersion": profile_version
    }))
    .expect("test frozen method must deserialize")
}

pub fn expression_request(
    profile_id: &str,
    profile_version: u64,
    input_mode: &str,
) -> VideoExpressionRequest {
    VideoExpressionRequest {
        profile_id: profile_id.into(),
        profile_version,
        input_mode: input_mode.into(),
        format_method: format_method(),
        content_method: None,
        mother: mother(),
        references: Vec::new(),
        capability_snapshot: caps_snapshot(),
        duration_seconds: None,
        expression_language: "zh".into(),
        template: expression_template(),
        images: Vec::new(),
        expansion_policy: None,
        negative_context: None,
        method: None,
        identity_locks: Vec::new(),
    }
}

pub fn expected_closed_set(request: &VideoExpressionRequest) -> ProjectionClosedSet {
    let target = bgx_prompt_optimize::video_job::target_from_request(request)
        .expect("fixture target must build");
    ProjectionClosedSet::new(&request.mother, &target)
        .with_expansion_policy(request.expansion_policy.clone())
        .with_negative_context(request.negative_context.clone())
}

pub fn expected_expression_schema(request: &VideoExpressionRequest) -> Value {
    draft_schema_for(&expected_closed_set(request))
}

pub fn target_value(profile_id: &str, profile_version: u64, input_mode: &str) -> Value {
    let target = VideoPromptTargetSnapshot {
        profile_id: profile_id.into(),
        profile_version,
        input_mode: input_mode.into(),
        duration_seconds: None,
        caps_digest: CAPS_DIGEST.into(),
        expression_language:
            bgx_video_prompt_contract::projection::VideoPromptExpressionLanguage::Zh,
        reference_use_ids: vec![],
    };
    serde_json::to_value(target).expect("target snapshot serializes")
}

pub fn official_image_template() -> FrozenSkillBytes {
    let files = BTreeMap::from([
        (
            "image-prompt-optimize/request.json".into(),
            IMAGE_REQUEST_JSON.to_owned(),
        ),
        (
            "image-prompt-optimize/SKILL.md".into(),
            IMAGE_SKILL_ZH.to_owned(),
        ),
        (
            "image-prompt-optimize/prompts/method.en.md".into(),
            IMAGE_METHOD_EN.to_owned(),
        ),
        (
            "shared/request-guidance.zh.md".into(),
            GUIDANCE_ZH.to_owned(),
        ),
        (
            "shared/request-guidance.en.md".into(),
            GUIDANCE_EN.to_owned(),
        ),
        ("shared/context-heading.zh.md".into(), CONTEXT_ZH.to_owned()),
        ("shared/context-heading.en.md".into(), CONTEXT_EN.to_owned()),
        ("shared/output-heading.zh.md".into(), OUTPUT_ZH.to_owned()),
        ("shared/output-heading.en.md".into(), OUTPUT_EN.to_owned()),
    ]);
    let snapshot = freeze_from_files(Language::Zh, &["image-prompt-optimize"], &files)
        .expect("official image template must freeze");
    pack_rules(&snapshot).expect("official image template must pack")
}

pub fn png_bytes(width: u32, height: u32, rgb: [u8; 3]) -> Vec<u8> {
    let img: image::ImageBuffer<image::Rgb<u8>, Vec<u8>> =
        image::ImageBuffer::from_fn(width, height, |_, _| image::Rgb(rgb));
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

pub fn submitted(order: u32, width: u32, height: u32, rgb: [u8; 3]) -> SubmittedImage {
    SubmittedImage {
        bytes: png_bytes(width, height, rgb),
        declared_order: order,
    }
}

pub fn ok_reply(content: &str, finish: &str) -> Result<TextReply, OptimizeError> {
    Ok(TextReply {
        content: content.into(),
        finish_reason: Some(finish.into()),
        raw_json: None,
    })
}

pub fn cancel_now() -> Result<(), OptimizeError> {
    Err(cancelled_error())
}

pub fn never_cancel() -> Result<(), OptimizeError> {
    Ok(())
}

#[derive(Debug, Clone)]
pub struct CapturedCall {
    pub schema: Option<Value>,
    pub step_label: String,
    pub image_urls: Vec<String>,
    pub image_count_policy: TextImageCountPolicy,
    pub json_mode: bool,
    /// Rendered message texts. Used to read the expression and review payloads.
    pub contents: Vec<String>,
}

pub fn image_urls_from_messages(call: &TextCall) -> Vec<String> {
    let mut urls = Vec::new();
    for message in &call.messages {
        if let Some(parts) = message.content.as_array() {
            for part in parts {
                if part["type"] == "image_url" {
                    if let Some(url) = part["image_url"]["url"].as_str() {
                        urls.push(url.to_owned());
                    }
                }
            }
        }
    }
    urls
}

pub struct RecordingCompleter {
    pub calls: Vec<CapturedCall>,
    replies: VecDeque<Result<TextReply, OptimizeError>>,
    cancel: fn() -> Result<(), OptimizeError>,
}

impl RecordingCompleter {
    pub fn new(replies: Vec<Result<TextReply, OptimizeError>>) -> Self {
        Self {
            calls: Vec::new(),
            replies: replies.into(),
            cancel: never_cancel,
        }
    }

    pub fn cancelling() -> Self {
        Self {
            calls: Vec::new(),
            replies: VecDeque::from([ok_reply(
                r#"{"optimizedPrompt":"MUST_NOT_BE_READ"}"#,
                "stop",
            )]),
            cancel: cancel_now,
        }
    }
}

impl TextCompleter for RecordingCompleter {
    fn check_cancel(&self) -> Result<(), OptimizeError> {
        (self.cancel)()
    }

    fn complete(&mut self, call: TextCall) -> Result<TextReply, OptimizeError> {
        self.calls.push(CapturedCall {
            schema: call.schema.clone(),
            step_label: call.step_label.clone(),
            image_urls: image_urls_from_messages(&call),
            image_count_policy: call.image_count_policy,
            json_mode: call.json_mode,
            contents: call
                .messages
                .iter()
                .map(|message| match &message.content {
                    Value::String(text) => text.clone(),
                    other => other.to_string(),
                })
                .collect(),
        });
        self.replies
            .pop_front()
            .expect("test completer received more calls than scripted replies")
    }
}
