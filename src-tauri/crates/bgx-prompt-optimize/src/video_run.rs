//! Shared `run_video_optimize` and `compress_video_prompt`.
//! `compress_only` must not enter `run_video_optimize` and must not load a Skill.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use bgx_video_prompt_contract::author_normalize::{
    normalize_author_reply, partition_author_outcome,
};
use bgx_video_prompt_contract::document::DocumentDraftV1;
use bgx_video_prompt_contract::identity::IdentityLockLine;
use bgx_video_prompt_contract::projection::{
    VideoPromptExpansionPolicy, VideoPromptFrozenMethod, VideoPromptProjectionDraft,
    VideoPromptTargetSnapshot,
};
use bgx_video_prompt_contract::inventory::ReferenceMediaInventoryItem;
use bgx_video_prompt_contract::FrozenNegativeContext;
use bgx_video_prompt_contract::ProjectionClosedSet;

use crate::error::{error_codes, OptimizeError};
use crate::ReferenceNumbers;
use crate::projectors::port::{ProjectionCompileInput, ProjectionSpanOrigin, ReviewedMother};
use crate::projectors::{compiler_registry, production_registry};
use crate::run::is_provider_output_truncated;
use crate::skill_material::FrozenSkillBytes;
use crate::template::{render_request, unpack_rules};
use crate::text_port::{no_cancel, TextCall, TextCompleter, TextMessage, TextReply};
use crate::video_job::{
    expression_compile_document, inventory_item, map_draft_references, numbered_expression_payload,
    reference_items_for_frozen_target, reference_numbers, review_schema_for,
    run_numbered_video_expression, semantic_fingerprint, target_from_request,
    VideoExpressionRejected, VideoExpressionRequest, VideoExpressionResult,
};

#[path = "stage_schema.rs"]
mod stage_schema;

pub use stage_schema::{author_batch_envelope, bind_mother_source_scopes};

/// Stage ids stay: video-source-understanding, video-reference-semantics,
/// video-content-optimize, video-content-review, video-expression
/// (plus existing video-expression-review / lyric-source-understanding).
pub const STAGE_REFERENCE: &str = "video-reference-semantics";
pub const STAGE_SOURCE: &str = "video-source-understanding";
pub const STAGE_LYRIC_SOURCE: &str = "lyric-source-understanding";
pub const STAGE_CONTENT: &str = "video-content-optimize";
pub const STAGE_CONTENT_REVIEW: &str = "video-content-review";
pub const STAGE_EXPRESSION: &str = "video-expression";
pub const STAGE_EXPRESSION_REVIEW: &str = "video-expression-review";

/// Official video-motion compress mid paragraph from `model_prompt_prepare`.
const FIXED_PREFIX_ZH: &str = "你是文本压缩编辑。用户的原始内容在紧随其后的 user 消息中给出。\n\
请将内容压缩为不超过 {max_chars} 个 Unicode 标量的单一终稿（计数方式与 Rust 字符串的 chars().count() 一致：每个汉字、拉丁字母、数字、标点各计 1；组合 emoji 可能占多个标量）。";
const FIXED_SUFFIX_ZH: &str = "保持原文语言（中文输入则输出中文，英文输入则输出英文）。不要输出标题、引号、序号、Markdown、代码块或任何解释性前后缀；只输出压缩后的正文一段。";
const VIDEO_MOTION_MID_ZH: &str = "压缩目标用于 AI 视频生成的运动与镜头描述。请保留：主体与动作、镜头运动（推拉摇移等）、节奏与时间顺序、场景/主体连续性、关键道具与冲突点；删除：静态绘画才需要的细节堆砌、与运动无关的长背景散文、重复镜头形容词。若存在明确镜头分段且字数仍超限，优先保留首尾与转折点。";

/// Official mother-author bind inputs from the host's already-frozen ledger.
/// Required when a `video-content-optimize` template is present. The core will
/// not invent shot IDs or a projected ledger, and will not send an unbound schema.
pub struct MotherAuthorBind {
    /// Official taskContext (host `mother_model_view::project_input(ledger)`).
    pub task_context: Value,
    pub expected_shot_ids: Vec<String>,
    pub source_unit_ids: Vec<String>,
    pub quote_candidates: Vec<Value>,
    /// Frozen span source scopes for official `bind_mother_source_scopes`.
    /// Host keeps collection/DB freeze and maps already-frozen `sourceMode` plus
    /// each unit's `unitId`/`writeScope`. `None` skips the bind, matching the
    /// host when no frozen source is present.
    pub source_scope: Option<MotherSourceScopeBind>,
    /// Ledger `sourceMode` for official `normalize_model_draft` provenance.
    /// Host uses `optimize_existing` when the ledger omits it.
    pub source_mode: String,
    /// Ledger `subjectIds` forwarded to official normalize (currently unused by
    /// the official body, kept for signature fidelity).
    pub subject_ids: Vec<String>,
}

/// Frozen input port for official `bind_mother_source_scopes`.
/// Only the fields that binder reads; remaining `frozenSource` fields stay on the host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MotherSourceScopeBind {
    pub source_mode: MotherSourceMode,
    pub source_units: Vec<MotherSourceUnitScope>,
}

/// Official `sourceMode` closed set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MotherSourceMode {
    Ordinary,
    Span,
}

/// Official `sourceWriteScope` closed set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MotherSourceWriteScope {
    Writable,
    ContextOnly,
    VisualOnly,
    BoundaryOnly,
}

/// Slim official source-unit fields required by `bind_mother_source_scopes`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MotherSourceUnitScope {
    pub unit_id: String,
    pub write_scope: MotherSourceWriteScope,
}

/// Host/shell snapshot plus already-collected bytes. No storyId, DbState, or AppHandle.
pub struct VideoOptimizeRequest {
    pub source_text: String,
    pub change_request: Option<String>,
    pub target_snapshot: serde_json::Value,
    pub format_method: FrozenSkillBytes,
    pub content_method: Option<FrozenSkillBytes>,
    pub templates: BTreeMap<String, FrozenSkillBytes>,
    pub expansion_policy: Option<VideoPromptExpansionPolicy>,
    pub negative_context: Option<FrozenNegativeContext>,
    /// Official mother closed-set bind + taskContext. Required for content authoring.
    pub mother_author: Option<MotherAuthorBind>,
    /// Optional official mother-review base (ledger / frozenSource / reviewable IDs).
    /// The authored draft is inserted as `draft` before schema bind. Legacy batches
    /// may omit this; the official binder then uses an empty frozen-source set.
    pub mother_review_base: Option<Value>,
    /// Official frozen format method from `FrozenRunInputs.method.frozen`.
    /// Required; compile must not invent `None`.
    pub method: VideoPromptFrozenMethod,
    /// Official identity lock lines from the host preflight (`identity_locks()`).
    /// Empty is honest only when that official preflight produced no lines.
    pub identity_locks: Vec<IdentityLockLine>,
    /// Same meaning as the host author path: empty source draft allows inferred facts.
    pub allow_inferred: bool,
    /// When the host already has a reviewed mother (project_existing / same-mother
    /// adapt_expand), skip content authoring and compile this document. `None`
    /// is the authoring path.
    pub existing_mother: Option<DocumentDraftV1>,
    /// Structured rows for this request. Expression and review look up
    /// `target_snapshot.referenceUseIds` here. Empty is valid only when that
    /// frozen id list is also empty.
    pub reference_inventory: Vec<ReferenceMediaInventoryItem>,
}

/// Result of the shared four-step optimize. Same shape as expression plus the compiled prompt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VideoOptimizePending {
    pub reason: String,
    pub issue_codes: Vec<String>,
}

/// Result of the shared optimize. Same shape as expression plus the compiled
/// prompt, the official mother, and a pending partition when some shots are unreadable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VideoOptimizeResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft: Option<bgx_video_prompt_contract::projection::VideoPromptProjectionDraft>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rejected: Option<VideoExpressionRejected>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_prompt: Option<String>,
    /// Authored or reused mother document the host writes back. Absent when
    /// authoring never produced an owned document.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mother: Option<DocumentDraftV1>,
    /// Official multi-shot pending (`MODEL_OUTPUT_UNREADABLE` / issue codes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending: Option<VideoOptimizePending>,
    /// Official compile source map. Not a wire field; host maps it onto the reviewed candidate.
    #[serde(default, skip)]
    pub source_map: Vec<crate::projectors::port::ProjectionSourceSpan>,
    /// Official compile diagnostics. Not a wire field; host maps them onto the reviewed candidate.
    #[serde(default, skip)]
    pub diagnostics: Vec<crate::projectors::port::ProjectionDiagnostic>,
    /// Official mother-review reply text when the content-review stage ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mother_review_raw: Option<String>,
    /// Official expression-review reply text when the expression-review stage ran.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expression_review_raw: Option<String>,
    /// Model review after reference resolution. The host still validates the review
    /// against the reviewed draft and the canonical closed set.
    #[serde(skip)]
    pub expression_review_resolved: Option<Result<String, String>>,
}

impl From<VideoExpressionResult> for VideoOptimizeResult {
    fn from(value: VideoExpressionResult) -> Self {
        Self {
            draft: value.draft,
            rejected: value.rejected,
            final_prompt: value.final_prompt,
            mother: None,
            pending: None,
            source_map: value.source_map,
            diagnostics: value.diagnostics,
            mother_review_raw: None,
            expression_review_raw: None,            expression_review_resolved: None,
        }
    }
}

fn expression_result(mother: DocumentDraftV1, value: VideoExpressionResult) -> VideoOptimizeResult {
    VideoOptimizeResult {
        draft: value.draft,
        rejected: value.rejected,
        final_prompt: value.final_prompt,
        mother: Some(mother),
        pending: None,
        source_map: value.source_map,
        diagnostics: value.diagnostics,
        mother_review_raw: None,
        expression_review_raw: None,        expression_review_resolved: None,
    }
}

/// Host-resolved compression request. No FrozenSkillBytes, no provider catalog, no DbState.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VideoCompressRequest {
    pub source_text: String,
    pub target_max_chars: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VideoCompressResult {
    pub compressed_text: String,
    pub raw_reply: String,
    #[serde(default)]
    pub output_incomplete: bool,
}

fn text_messages(rows: Vec<(String, String)>) -> Vec<TextMessage> {
    rows.into_iter()
        .map(|(role, content)| TextMessage {
            role,
            content: Value::String(content),
        })
        .collect()
}

fn build_compress_system_prompt(max_chars: usize) -> String {
    format!(
        "{}\n\n{}\n\n{}",
        FIXED_PREFIX_ZH.replace("{max_chars}", &max_chars.to_string()),
        VIDEO_MOTION_MID_ZH,
        FIXED_SUFFIX_ZH,
    )
}

fn build_second_attempt_system_prompt(max_chars: usize, last_n: usize) -> String {
    let base = build_compress_system_prompt(max_chars);
    format!(
        "{base}\n\n特别注意：上次输出约 {last_n} 个字符，超过上限 {max_chars}。请重新压缩，**输出必须 ≤ {max_chars}**，只输出正文。计数方式同上。"
    )
}

/// Shared compression entry used by the main host and the shell.
/// This is not `run_video_optimize` and must not load a Skill.
pub fn compress_video_prompt<C: TextCompleter>(
    request: VideoCompressRequest,
    completer: &mut C,
) -> Result<VideoCompressResult, OptimizeError> {
    let trimmed = request.source_text.trim();
    if trimmed.is_empty() {
        return Ok(VideoCompressResult {
            compressed_text: String::new(),
            raw_reply: String::new(),
            output_incomplete: false,
        });
    }
    let target = request.target_max_chars as usize;
    if trimmed.chars().count() <= target {
        return Ok(VideoCompressResult {
            compressed_text: trimmed.to_string(),
            raw_reply: String::new(),
            output_incomplete: false,
        });
    }
    let complete = |completer: &mut C, messages: Vec<TextMessage>, attempt: &str| {
        completer.check_cancel()?;
        let reply = completer.complete(TextCall {
            messages,
            json_mode: false,
            schema: None,
            step_label: format!("model_prompt_prepare.compress.{attempt}"),
            check_cancel: no_cancel,
            image_count_policy: crate::text_port::TextImageCountPolicy::ModelCapability,
        })?;
        completer.check_cancel()?;
        Ok::<TextReply, OptimizeError>(reply)
    };
    let first_messages = text_messages(vec![
        ("system".into(), build_compress_system_prompt(target)),
        ("user".into(), trimmed.to_string()),
    ]);
    let first = complete(completer, first_messages, "1")?;
    let s1 = first.content.trim().to_string();
    if s1.is_empty() {
        return Err(OptimizeError::new(
            error_codes::INTERNAL_PROMPT_CONFIG,
            "文本模型返回的压缩结果为空（第一次尝试）",
        ));
    }
    if s1.chars().count() <= target {
        return Ok(VideoCompressResult {
            compressed_text: s1,
            raw_reply: first.content,
            output_incomplete: is_provider_output_truncated(first.finish_reason.as_deref()),
        });
    }
    let user_feedback = format!(
        "你上次输出约 {} 个 Unicode 标量（chars().count()），超过上限 {target}。请重新压缩，输出必须 ≤ {target}，只输出正文。",
        s1.chars().count()
    );
    let second_messages = text_messages(vec![
        (
            "system".into(),
            build_second_attempt_system_prompt(target, s1.chars().count()),
        ),
        ("user".into(), trimmed.to_string()),
        ("assistant".into(), s1),
        ("user".into(), user_feedback),
    ]);
    let second = complete(completer, second_messages, "2")?;
    let s2 = second.content.trim().to_string();
    if s2.is_empty() {
        return Err(OptimizeError::new(
            error_codes::INTERNAL_PROMPT_CONFIG,
            "文本模型返回的压缩结果为空（第二次尝试）",
        ));
    }
    let s2_chars = s2.chars().count();
    Ok(VideoCompressResult {
        compressed_text: s2,
        raw_reply: second.content,
        output_incomplete: is_provider_output_truncated(second.finish_reason.as_deref())
            || s2_chars > target,
    })
}

fn render_stage(
    templates: &BTreeMap<String, FrozenSkillBytes>,
    stage: &str,
    task_context: Value,
    output_contract: &Value,
) -> Result<Vec<TextMessage>, OptimizeError> {
    let material = templates.get(stage).ok_or_else(|| {
        OptimizeError::new(
            error_codes::INTERNAL_PROMPT_CONFIG,
            format!("missing frozen template for stage {stage}"),
        )
    })?;
    let snapshot = unpack_rules(material)?;
    let inputs = BTreeMap::from([
        ("taskContext".into(), task_context),
        ("outputContract".into(), output_contract.clone()),
    ]);
    let rendered = if snapshot.tasks.contains_key(stage) {
        render_request(&snapshot, stage, &inputs)?
    } else {
        let task = snapshot.tasks.keys().next().ok_or_else(|| {
            OptimizeError::new(
                error_codes::INTERNAL_PROMPT_CONFIG,
                format!("stage {stage} template has no task"),
            )
        })?;
        render_request(&snapshot, task, &inputs)?
    };
    let mut messages = rendered
        .into_iter()
        .map(|message| TextMessage {
            role: message.role,
            content: Value::String(message.content),
        })
        .collect();
    crate::reference_guidance::apply(&mut messages, snapshot.language);
    Ok(messages)
}

fn complete_stage<C: TextCompleter>(
    completer: &mut C,
    stage: &str,
    messages: Vec<TextMessage>,
    schema: &Value,
) -> Result<TextReply, OptimizeError> {
    completer.check_cancel()?;
    let reply = completer.complete(TextCall {
        messages,
        json_mode: true,
        schema: Some(schema.clone()),
        step_label: stage.to_string(),
        check_cancel: no_cancel,
        image_count_policy: crate::text_port::TextImageCountPolicy::ModelCapability,
    })?;
    completer.check_cancel()?;
    Ok(reply)
}

fn ensure_registered_projector(target: &VideoPromptTargetSnapshot) -> Result<(), OptimizeError> {
    production_registry()?.require(&target.profile_id, target.profile_version)?;
    if !production_registry()?.supports_input_mode(
        &target.profile_id,
        target.profile_version,
        &target.input_mode,
    ) {
        return Err(OptimizeError::new(
            error_codes::VIDEO_PROMPT_INPUT_MODE_UNSUPPORTED,
            format!(
                "档案 {}@{} 不支持模式 {}",
                target.profile_id, target.profile_version, target.input_mode
            ),
        ));
    }
    let compilers =
        compiler_registry().map_err(|error| OptimizeError::new(error.code(), error.to_string()))?;
    compilers
        .projector(&target.profile_id, target.profile_version)
        .ok_or_else(|| {
            OptimizeError::new(
                error_codes::VIDEO_PROMPT_PROJECTOR_MISSING,
                format!(
                    "没有 {}@{} 的生产投影器，不会改用通用档案。",
                    target.profile_id, target.profile_version
                ),
            )
        })?;
    Ok(())
}

fn expression_language_wire(
    language: &bgx_video_prompt_contract::projection::VideoPromptExpressionLanguage,
) -> &'static str {
    match language {
        bgx_video_prompt_contract::projection::VideoPromptExpressionLanguage::Zh => "zh",
        bgx_video_prompt_contract::projection::VideoPromptExpressionLanguage::En => "en",
    }
}

fn compiled_review_preview(
    request: &VideoExpressionRequest,
    draft: &VideoPromptProjectionDraft,
) -> Result<Value, OptimizeError> {
    let target = target_from_request(request)?;
    let (document, fingerprint) = expression_compile_document(
        &request.mother,
        draft,
        request.expansion_policy.as_ref(),
    )?;
    let references = request
        .references
        .iter()
        .map(inventory_item)
        .collect::<Result<Vec<_>, _>>()?;
    let candidate = compiler_registry()
        .map_err(|error| OptimizeError::new(error.code(), error.to_string()))?
        .projector(&request.profile_id, request.profile_version)
        .ok_or_else(|| {
            OptimizeError::new(
                error_codes::VIDEO_PROMPT_PROJECTOR_MISSING,
                format!(
                    "没有 {}@{} 的生产投影器，不会改用通用档案。",
                    request.profile_id, request.profile_version
                ),
            )
        })?
        .compile(&ProjectionCompileInput {
            mother: ReviewedMother {
                document: &document,
                canonical_fingerprint: &fingerprint,
            },
            references: &references,
            identity_locks: &request.identity_locks,
            target: &target,
            method: request.method.as_ref(),
            expression: Some(draft),
        })
        .map_err(|error| OptimizeError::new(error.code(), error.to_string()))?;
    let spans = candidate
        .source_map
        .iter()
        .filter_map(|span| match &span.origin {
            ProjectionSpanOrigin::ExpressionLine {
                line_index,
                shot_id,
                fact_ids,
            } => Some(json!({
                "kind": "expressionLine",
                "lineIndex": line_index,
                "shotId": shot_id,
                "factIds": fact_ids,
                "startByte": span.start,
                "endByte": span.end,
            })),
            ProjectionSpanOrigin::Quote { shot_id, quote_id } => Some(json!({
                "kind": "quote",
                "shotId": shot_id,
                "quoteId": quote_id,
                "startByte": span.start,
                "endByte": span.end,
            })),
            _ => None,
        })
        .collect::<Vec<_>>();
    Ok(json!({"text": candidate.text, "sourceMap": spans}))
}

fn format_review_prepare_error(error: OptimizeError) -> OptimizeError {
    OptimizeError::new(
        error_codes::VIDEO_PROMPT_FORMAT_REVIEW_PREPARE_FAILED,
        "格式稿已经拿到，但准备审阅请求失败，审阅没有开始。",
    )
    .with_raw_reply(format!("{}: {}", error.code, error.message))
}

/// Leave finding shape and semantic checks to the canonical review validator.
fn resolve_review_references(raw: &str, numbers: &ReferenceNumbers) -> Result<String, String> {
    let mut value: Value = serde_json::from_str(raw).map_err(|error| error.to_string())?;
    if let Some(findings) = value.get_mut("findings").and_then(Value::as_array_mut) {
        for (index, finding) in findings.iter_mut().enumerate() {
            if let Some(ids) = finding.get_mut("referenceUseIds").and_then(Value::as_array_mut) {
                for (position, id) in ids.iter_mut().enumerate() {
                    if let Some(number) = id.as_str() {
                        let resolved = numbers.resolve(number).ok_or_else(|| {
                            format!("findings[{index}].referenceUseIds[{position}]: unknown reference number {number}")
                        })?;
                        *id = json!(resolved);
                    }
                }
            }
        }
    }
    Ok(value.to_string())
}

/// Shared optimize entry. Hosts must not call this for `compress_only`.
pub fn run_video_optimize<C: TextCompleter>(
    request: VideoOptimizeRequest,
    completer: &mut C,
) -> Result<VideoOptimizeResult, OptimizeError> {
    let target: VideoPromptTargetSnapshot = serde_json::from_value(request.target_snapshot.clone())
        .map_err(|error| {
            OptimizeError::new(
                error_codes::INTERNAL_PROMPT_CONFIG,
                format!("target snapshot is invalid: {error}"),
            )
        })?;
    // Refuse an unregistered or unsupported projector before any model call.
    ensure_registered_projector(&target)?;

    // 2. Author from the complete draft, including speech at the point of action.
    // The legacy source/lyric templates can remain installed while this lane is paused.
    let mut mother_review_raw = None;
    let mother = if let Some(existing) = request.existing_mother.clone() {
        existing
    } else {
        if !request.templates.contains_key(STAGE_CONTENT) {
            return Err(OptimizeError::new(
                error_codes::INTERNAL_PROMPT_CONFIG,
                "run_video_optimize requires a video-content-optimize template to author the mother",
            ));
        }
        let mother_author = request.mother_author.as_ref().ok_or_else(|| {
            OptimizeError::new(
                error_codes::INTERNAL_PROMPT_CONFIG,
                "run_video_optimize requires official mother author bind inputs (projected taskContext and expected shot IDs)",
            )
        })?;
        let mut mother_schema = stage_schema::mother_run_schema(
            &mother_author.expected_shot_ids,
            &mother_author.source_unit_ids,
            &mother_author.quote_candidates,
        )?;
        if let Some(source) = &mother_author.source_scope {
            stage_schema::bind_mother_source_scopes(&mut mother_schema, source);
        }
        // Prompt embeds the full machine contract; multi-shot transport uses the
        // official batch envelope. Official per-shot Ready partitioning is shared.
        let transport_schema = if mother_author.expected_shot_ids.len() > 1 {
            stage_schema::author_batch_envelope(&mother_schema)
        } else {
            mother_schema.clone()
        };
        let author_messages = render_stage(
            &request.templates,
            STAGE_CONTENT,
            bgx_video_prompt_contract::project_mother_input(&mother_author.task_context),
            &mother_schema,
        )?;
        let author_reply = complete_stage(completer, STAGE_CONTENT, author_messages, &transport_schema)?;
        let shot_item_schema = mother_schema.get("properties").and_then(|value| value.get("shots")).and_then(|value| value.get("items"));
        let (draft, issues) = normalize_author_reply(
            author_reply.content.trim(),
            &mother_author.expected_shot_ids,
            if mother_author.source_mode.is_empty() {
                "optimize_existing"
            } else {
                &mother_author.source_mode
            },
            request.allow_inferred,
            &mother_author.subject_ids,
            &mother_author.source_unit_ids,
            shot_item_schema,
        );
        let parts = partition_author_outcome(&mother_author.expected_shot_ids, &draft, &issues)
            .map_err(|error| {
                OptimizeError::new("VIDEO_PROMPT_AUTHOR_OWNERSHIP", error)
                    .with_raw_reply(author_reply.content.clone())
            })?;
        if let Some(pending) = parts.iter().find(|part| part.pending_reason.is_some()) {
            return Ok(VideoOptimizeResult {
                draft: None,
                rejected: None,
                final_prompt: None,
                mother: Some(DocumentDraftV1 {
                    shots: parts.iter().filter_map(|part| part.draft.clone()).collect(),
                }),
                pending: Some(VideoOptimizePending {
                    reason: pending
                        .pending_reason
                        .clone()
                        .unwrap_or_else(|| "MODEL_OUTPUT_UNREADABLE".into()),
                    issue_codes: parts
                        .iter()
                        .flat_map(|part| part.issues.iter().map(|issue| issue.code.clone()))
                        .collect(),
                }),
            source_map: Vec::new(),
            diagnostics: Vec::new(),
            mother_review_raw: None,
            expression_review_raw: None,            expression_review_resolved: None,
            });
        }
        let mother = DocumentDraftV1 {
            shots: parts.into_iter().filter_map(|part| part.draft).collect(),
        };
        // 3. optional mother review. The official draft is the authored mother;
        // review output is not a DocumentDraftV1 and must not replace it.
        if request.templates.contains_key(STAGE_CONTENT_REVIEW) {
            let authored = serde_json::to_value(&mother).map_err(|error| {
                OptimizeError::new(
                    "VIDEO_PROMPT_MOTHER_JSON_INVALID",
                    format!("authored mother cannot be encoded: {error}"),
                )
            })?;
            let mut input = request
                .mother_review_base
                .clone()
                .unwrap_or_else(|| json!({}));
            if let Some(object) = input.as_object_mut() {
                object.insert("draft".into(), authored);
                if let Some(change) = &request.change_request {
                    object
                        .entry("changeRequest".to_owned())
                        .or_insert(json!(change));
                }
            }
            let review_schema = stage_schema::mother_review_schema(&input)?;
            let messages = render_stage(
                &request.templates,
                STAGE_CONTENT_REVIEW,
                input,
                &review_schema,
            )?;
            mother_review_raw = Some(
                complete_stage(completer, STAGE_CONTENT_REVIEW, messages, &review_schema)?.content,
            );
        }
        mother
    };
    // 4. expression + compile. Review uses the same frozen target and inventory.
    // Missing or conflicting rows stop here, before the format model call.
    let references = reference_items_for_frozen_target(&target, &request.reference_inventory)?;
    let numbers = reference_numbers(&references);
    let expression_template = request
        .templates
        .get(STAGE_EXPRESSION)
        .cloned()
        .ok_or_else(|| {
            OptimizeError::new(
                error_codes::INTERNAL_PROMPT_CONFIG,
                "run_video_optimize requires a video-expression template",
            )
        })?;
    let expression_request = VideoExpressionRequest {
        profile_id: target.profile_id.clone(),
        profile_version: target.profile_version,
        input_mode: target.input_mode.clone(),
        format_method: request.format_method.clone(),
        content_method: request.content_method.clone(),
        mother: mother.clone(),
        references,
        capability_snapshot: request.target_snapshot.clone(),
        duration_seconds: target.duration_seconds,
        expression_language: expression_language_wire(&target.expression_language).to_owned(),
        template: expression_template,
        images: Vec::new(),
        expansion_policy: request.expansion_policy.clone(),
        negative_context: request.negative_context.clone(),
        method: Some(request.method.clone()),
        identity_locks: request.identity_locks.clone(),
    };
    let result = run_numbered_video_expression(&expression_request, &numbers, completer)?;
    let review: Result<Option<String>, OptimizeError> = (|| {
        if request.templates.contains_key(STAGE_EXPRESSION_REVIEW) {
            if let Some(draft) = result.draft.as_ref() {
                let review_request = &expression_request;
                let closed = ProjectionClosedSet::new(&review_request.mother, &target)
                    .with_expansion_policy(review_request.expansion_policy.clone())
                    .with_negative_context(review_request.negative_context.clone());
                let schema = review_schema_for(&numbers.model_closed_set(&closed), draft.lines.len());
                let fingerprint = semantic_fingerprint(&review_request.mother);
                let mut payload = match numbered_expression_payload(review_request, &target, &fingerprint, &numbers) {
                    Ok(payload) => payload,
                    Err(error) => return Err(format_review_prepare_error(error)),
                };
                if let Some(object) = payload.as_object_mut() {
                    let mut model_draft = draft.clone();
                    map_draft_references(&mut model_draft, |id, path| {
                        *id = numbers.number(id).ok_or_else(|| {
                            OptimizeError::new(error_codes::INTERNAL_PROMPT_CONFIG, path)
                        })?.to_owned();
                        Ok(())
                    }).map_err(format_review_prepare_error)?;
                    let lines: Vec<Value> = model_draft
                        .lines
                        .iter()
                        .enumerate()
                        .map(|(index, line)| {
                            json!({
                                "lineIndex": index,
                                "shotId": line.shot_id,
                                "factIds": line.fact_ids,
                                "quoteIds": line.quote_ids,
                                "referenceUseIds": line.reference_use_ids,
                                "text": line.text,
                            })
                        })
                        .collect();
                    object.insert("lines".into(), Value::Array(lines));
                    object.insert("proposedFacts".into(), json!(draft.proposed_facts));
                    object.insert("negative".into(), json!(draft.negative));
                    object.insert(
                        "speakerVoiceBindings".into(),
                        json!(model_draft.speaker_voice_bindings),
                    );
                    let preview = match compiled_review_preview(review_request, draft) {
                        Ok(preview) => preview,
                        Err(error) => return Err(format_review_prepare_error(error)),
                    };
                    object.insert("compiledPreview".into(), preview);
                }
                let messages = match render_stage(
                    &request.templates,
                    STAGE_EXPRESSION_REVIEW,
                    payload,
                    &schema,
                ) {
                    Ok(messages) => messages,
                    Err(error) => return Err(format_review_prepare_error(error)),
                };
                return complete_stage(completer, STAGE_EXPRESSION_REVIEW, messages, &schema)
                    .map(|reply| Some(reply.content));
            }
        }
        Ok(None)
    })();
    let expression_review_raw = match review {
        Ok(raw) => raw,
        Err(mut error) => {
            error.compiled_video_candidate = result.final_prompt;
            return Err(error);
        }
    };
    let mut optimize = expression_result(mother, result);
    optimize.mother_review_raw = mother_review_raw;
    optimize.expression_review_resolved = expression_review_raw.as_deref()
        .map(|raw| resolve_review_references(raw, &numbers));
    optimize.expression_review_raw = expression_review_raw;
    Ok(optimize)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::OptimizeError;
    use crate::text_port::{TextCall, TextCompleter, TextReply};
    use std::sync::Mutex;

    struct RecordingCompleter {
        labels: Mutex<Vec<String>>,
        replies: Mutex<Vec<TextReply>>,
    }

    impl TextCompleter for RecordingCompleter {
        fn complete(&mut self, call: TextCall) -> Result<TextReply, OptimizeError> {
            self.labels.lock().unwrap().push(call.step_label.clone());
            let mut replies = self.replies.lock().unwrap();
            if replies.is_empty() {
                return Err(OptimizeError::new("TEST", "no scripted reply"));
            }
            Ok(replies.remove(0))
        }
    }

    #[test]
    fn compress_video_prompt_skips_the_model_when_already_under_target() {
        let mut completer = RecordingCompleter {
            labels: Mutex::new(Vec::new()),
            replies: Mutex::new(Vec::new()),
        };
        let result = compress_video_prompt(
            VideoCompressRequest {
                source_text: "短稿".into(),
                target_max_chars: 10,
            },
            &mut completer,
        )
        .unwrap();
        assert_eq!(result.compressed_text, "短稿");
        assert!(completer.labels.lock().unwrap().is_empty());
    }

    #[test]
    fn compress_video_prompt_does_not_load_a_skill_or_enter_optimize() {
        let mut completer = RecordingCompleter {
            labels: Mutex::new(Vec::new()),
            replies: Mutex::new(vec![TextReply {
                content: "压缩后的运动描述".into(),
                finish_reason: Some("stop".into()),
                raw_json: None,
            }]),
        };
        let long = "镜头向前推进，".repeat(20);
        let result = compress_video_prompt(
            VideoCompressRequest {
                source_text: long,
                target_max_chars: 8,
            },
            &mut completer,
        )
        .unwrap();
        assert_eq!(result.compressed_text, "压缩后的运动描述");
        assert_eq!(
            completer.labels.lock().unwrap().as_slice(),
            ["model_prompt_prepare.compress.1"]
        );
    }
}
