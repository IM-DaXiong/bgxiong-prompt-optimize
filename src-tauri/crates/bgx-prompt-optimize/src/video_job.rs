use serde::{Deserialize, Serialize};
use serde_json::Value;

#[path = "expression_schema.rs"]
mod expression_schema;
pub use expression_schema::{draft_schema_for, review_schema_for};

use bgx_video_prompt_contract::document::contract as semantic_contract;
use bgx_video_prompt_contract::identity::IdentityLockLine;
use bgx_video_prompt_contract::projection::{
    VideoPromptExpansionPolicy, VideoPromptExpressionLanguage, VideoPromptFrozenMethod,
    VideoPromptProjectionDraft, VideoPromptTargetSnapshot,
};
use bgx_video_prompt_contract::validate::validate_projection_draft;
use bgx_video_prompt_contract::{DocumentDraftV1, FrozenNegativeContext, ProjectionClosedSet};

use crate::image_job::SubmittedImage;
use crate::ReferenceNumbers;
use crate::skill_material::FrozenSkillBytes;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VideoReferenceItem {
    pub item_id: String,
    pub media_kind: String,
    pub ordinal_within_kind: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_usage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binding_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declared_identity: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VideoExpressionRequest {
    pub profile_id: String,
    pub profile_version: u64,
    pub input_mode: String,
    pub format_method: FrozenSkillBytes,
    #[serde(default)]
    pub content_method: Option<FrozenSkillBytes>,
    pub mother: DocumentDraftV1,
    #[serde(default)]
    pub references: Vec<VideoReferenceItem>,
    pub capability_snapshot: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_seconds: Option<f64>,
    pub expression_language: String,
    pub template: FrozenSkillBytes,
    #[serde(default)]
    pub images: Vec<SubmittedImage>,
    /// Host expression currently freezes and applies this policy to the closed set.
    /// Omit only when the caller truly has no expansion policy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expansion_policy: Option<VideoPromptExpansionPolicy>,
    /// Host expression currently freezes and applies this negative context to the closed set.
    /// Omit only when the caller truly has no negative context.
    /// The contract type is Serialize-only; host injects it in-process, it is not decoded from JSON.
    #[serde(default, skip_deserializing, skip_serializing_if = "Option::is_none")]
    pub negative_context: Option<FrozenNegativeContext>,
    /// Official frozen method from `VideoOptimizeRequest.method`. Standalone
    /// expression tests may omit it; production optimize must pass Some.
    #[serde(skip)]
    pub method: Option<VideoPromptFrozenMethod>,
    /// Official identity lock lines from the host preflight. Empty is honest
    /// only when that official preflight produced no lines.
    #[serde(skip)]
    pub identity_locks: Vec<IdentityLockLine>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VideoExpressionRejected {
    pub code: String,
    pub detail: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_reply: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VideoExpressionResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// Expression draft (validate_projection_draft). Not the mother DocumentDraftV1.
    pub draft: Option<VideoPromptProjectionDraft>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rejected: Option<VideoExpressionRejected>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_prompt: Option<String>,
    /// Official compile source map. Not a wire field; host maps it onto the reviewed candidate.
    #[serde(default, skip)]
    pub source_map: Vec<crate::projectors::port::ProjectionSourceSpan>,
    /// Official compile diagnostics. Not a wire field; host maps them onto the reviewed candidate.
    #[serde(default, skip)]
    pub diagnostics: Vec<crate::projectors::port::ProjectionDiagnostic>,
}

use crate::error::{error_codes, OptimizeError};
use crate::projectors::port::{ProjectionCompileInput, ReviewedMother};
use crate::projectors::{compiler_registry, production_registry};
use crate::skill_material::validate_resource_paths;
use crate::template::{render_request, unpack_rules};
use crate::text_port::{no_cancel, TextCall, TextCompleter, TextMessage};
use bgx_video_prompt_contract::inventory::ReferenceMediaInventoryItem;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub const DRAFT_JSON: &str = "PROJECTION_DRAFT_JSON";
pub const DRAFT_SHAPE: &str = "PROJECTION_DRAFT_SHAPE";
const EXPRESSION_TASK: &str = "video-expression";

fn language_from_wire(value: &str) -> Result<VideoPromptExpressionLanguage, OptimizeError> {
    match value.trim() {
        "zh" => Ok(VideoPromptExpressionLanguage::Zh),
        "en" => Ok(VideoPromptExpressionLanguage::En),
        other => Err(OptimizeError::new(
            error_codes::INTERNAL_PROMPT_CONFIG,
            format!("unsupported expressionLanguage {other}"),
        )),
    }
}

fn language_wire(language: &VideoPromptExpressionLanguage) -> &'static str {
    match language {
        VideoPromptExpressionLanguage::Zh => "zh",
        VideoPromptExpressionLanguage::En => "en",
    }
}

fn hex_sha256(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn caps_digest_for(capability_snapshot: &Value) -> String {
    if let Some(digest) = capability_snapshot
        .get("capsDigest")
        .and_then(Value::as_str)
    {
        if digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return digest.to_string();
        }
    }
    hex_sha256(&serde_json::to_vec(capability_snapshot).unwrap_or_else(|_| b"{}".to_vec()))
}

pub fn target_from_request(
    request: &VideoExpressionRequest,
) -> Result<VideoPromptTargetSnapshot, OptimizeError> {
    Ok(VideoPromptTargetSnapshot {
        profile_id: request.profile_id.clone(),
        profile_version: request.profile_version,
        input_mode: request.input_mode.clone(),
        duration_seconds: request.duration_seconds,
        caps_digest: caps_digest_for(&request.capability_snapshot),
        expression_language: language_from_wire(&request.expression_language)?,
        reference_use_ids: request
            .references
            .iter()
            .map(|item| item.item_id.clone())
            .collect(),
    })
}

fn check_method_resource_budget(method: &FrozenSkillBytes) -> Result<(), OptimizeError> {
    validate_resource_paths(method)?;
    let budget = semantic_contract().method_resource_max_chars;
    let mut used: u64 = method.body.chars().count() as u64;
    for bytes in method.resources.values() {
        used = used.saturating_add(String::from_utf8_lossy(bytes).chars().count() as u64);
    }
    if used > budget {
        return Err(OptimizeError::new(
            "PROJECTION_RESOURCE_BUDGET_EXCEEDED",
            format!(
                "method resources need {used} characters but the run budget is {budget}; they are not truncated"
            ),
        ));
    }
    Ok(())
}

/// The selected generation duration belongs to the renderer, not the writing task.
pub fn declared_capability_evidence(extensions: &Value, input_mode: &str) -> Value {
    let mut evidence = extensions.clone();
    if let Some(object) = evidence.as_object_mut() {
        object.remove("durationSeconds");
        if let Some(modes) = object.get_mut("inputModes").and_then(Value::as_array_mut) {
            modes.retain(|mode| mode.get("id").and_then(Value::as_str) == Some(input_mode));
        }
    }
    evidence
}

/// One frozen target plus the inventory rows those ids name.
/// Expression, review and compile all read this pair. Standalone expression
/// still builds its own `VideoExpressionRequest` and then calls `run_video_expression`.
pub fn reference_items_for_frozen_target(
    target: &VideoPromptTargetSnapshot,
    inventory: &[ReferenceMediaInventoryItem],
) -> Result<Vec<VideoReferenceItem>, OptimizeError> {
    let mut rows = Vec::with_capacity(target.reference_use_ids.len());
    for use_id in &target.reference_use_ids {
        let matched: Vec<&ReferenceMediaInventoryItem> = inventory
            .iter()
            .filter(|item| item.item_id == *use_id)
            .collect();
        let Some(first) = matched.first().copied() else {
            return Err(OptimizeError::new(
                error_codes::VIDEO_PROMPT_REFERENCE_ROW_MISSING,
                format!("参考素材 {use_id} 没有对应的素材行，格式整理还没有开始。请重新选择素材后再试。"),
            ));
        };
        if matched.iter().any(|item| *item != first) {
            return Err(OptimizeError::new(
                error_codes::VIDEO_PROMPT_REFERENCE_ROW_CONFLICT,
                format!("参考素材 {use_id} 有互相矛盾的素材行，格式整理还没有开始。请重新选择素材后再试。"),
            ));
        }
        rows.push(video_reference_from_inventory(first)?);
    }
    Ok(rows)
}

fn video_reference_from_inventory(
    item: &ReferenceMediaInventoryItem,
) -> Result<VideoReferenceItem, OptimizeError> {
    let declared_identity = match &item.declared_identity {
        Some(identity) => Some(serde_json::to_value(identity).map_err(|error| {
            OptimizeError::new(
                error_codes::INTERNAL_PROMPT_CONFIG,
                format!("参考素材 {} 的身份无法编码：{error}", item.item_id),
            )
        })?),
        None => None,
    };
    let label = {
        let trimmed = item.label.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    };
    Ok(VideoReferenceItem {
        item_id: item.item_id.clone(),
        media_kind: item.media_kind.clone(),
        ordinal_within_kind: item.ordinal_within_kind,
        audio_usage: item.audio_usage.clone(),
        label,
        binding_id: item.binding_id.clone(),
        declared_identity,
    })
}

pub(crate) fn inventory_item(item: &VideoReferenceItem) -> Result<ReferenceMediaInventoryItem, OptimizeError> {
    serde_json::from_value(json!({
        "token": item.item_id,
        "mediaKind": item.media_kind,
        "label": item.label.clone().unwrap_or_default(),
        "bindingId": item.binding_id,
        "mediaRelativePath": "",
        "audioUsage": item.audio_usage,
        "itemId": item.item_id,
        "declaredIdentity": item.declared_identity,
        "ordinalWithinKind": item.ordinal_within_kind,
    }))
    .map_err(|error| {
        OptimizeError::new(
            error_codes::INTERNAL_PROMPT_CONFIG,
            format!("reference inventory item {}: {error}", item.item_id),
        )
    })
}

/// Official `selected_quote_document`: only quotes referenced by expression lines
/// of the same shot are kept for automatic insertion. Inline speech stays in
/// `lines.text`; unselected metadata must not be appended as a duplicate.
pub fn selected_quote_document(
    mother: &DocumentDraftV1,
    expression: &VideoPromptProjectionDraft,
) -> DocumentDraftV1 {
    let mut selected = mother.clone();
    for shot in &mut selected.shots {
        let shot_id = &shot.shot_id;
        shot.quotes.retain(|quote| {
            expression.lines.iter().any(|line| {
                line.shot_id == *shot_id && line.quote_ids.iter().any(|id| id == &quote.quote_id)
            })
        });
    }
    selected
}

/// Official `semantic_fingerprint`: SHA-256 hex of `v{project_format_version}|{json}`.
pub fn semantic_fingerprint(draft: &DocumentDraftV1) -> String {
    let raw = serde_json::to_string(draft).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(format!("v{}|{raw}", semantic_contract().project_format_version).as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Official compile mother for expression review preview and final compile:
/// `expanded_document` then `selected_quote_document`, fingerprinted with
/// `semantic_fingerprint` of that compile document.
pub fn expression_compile_document(
    mother: &DocumentDraftV1,
    draft: &VideoPromptProjectionDraft,
    policy: Option<&VideoPromptExpansionPolicy>,
) -> Result<(DocumentDraftV1, String), OptimizeError> {
    let expanded = bgx_video_prompt_contract::expanded_document(mother, draft, policy).map_err(
        |error| OptimizeError::new(error.clone(), error),
    )?;
    let document = selected_quote_document(&expanded, draft);
    let fingerprint = semantic_fingerprint(&document);
    Ok((document, fingerprint))
}

/// User payload of the expression call: reviewed mother and frozen target facts only.
pub fn expression_payload(
    request: &VideoExpressionRequest,
    target: &VideoPromptTargetSnapshot,
    fingerprint: &str,
) -> Result<Value, OptimizeError> {
    let numbers = reference_numbers(&request.references);
    numbered_expression_payload(request, target, fingerprint, &numbers)
}

pub(crate) fn reference_numbers(references: &[VideoReferenceItem]) -> ReferenceNumbers {
    ReferenceNumbers::new(references.iter().map(|item| {
        (item.item_id.as_str(), item.media_kind.as_str(), item.ordinal_within_kind)
    }))
}

pub(crate) fn numbered_expression_payload(
    request: &VideoExpressionRequest,
    target: &VideoPromptTargetSnapshot,
    fingerprint: &str,
    numbers: &ReferenceNumbers,
) -> Result<Value, OptimizeError> {
    let document = &request.mother;
    if document.shots.iter().all(|shot| shot.facts.is_empty()) {
        return Err(OptimizeError::new(
            "VIDEO_PROMPT_PROJECTION_MOTHER_EMPTY",
            "已审核的提示词母版里没有可表达的内容。",
        ));
    }
    let shots: Vec<Value> = document
        .shots
        .iter()
        .map(|shot| {
            json!({
                "shotId": shot.shot_id,
                "facts": shot.facts.iter().map(|fact| json!({
                    "factId": fact.fact_id,
                    "kind": fact.kind,
                    "semanticSlot": fact.semantic_slot,
                    "polarity": fact.polarity,
                    "entityRefIds": fact.entity_ref_ids,
                    "sourceUnitIds": fact.source_unit_ids,
                    "text": fact.text,
                })).collect::<Vec<_>>(),
                "quotes": shot.quotes.iter().map(|quote| json!({
                    "quoteId": quote.quote_id,
                    "role": quote.role,
                    "exactTextReadOnly": quote.exact_text,
                    "sourceUnitId": quote.source_unit_id,
                    "speakerId": quote.speaker_id,
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let mut references: Vec<Value> = Vec::new();
    for use_id in &target.reference_use_ids {
        if !seen.insert(use_id.as_str()) {
            continue;
        }
        let item = request
            .references
            .iter()
            .find(|item| item.item_id == *use_id)
            .ok_or_else(|| {
                OptimizeError::new(
                    "VIDEO_PROMPT_PROJECTION_REFERENCE_USE_UNKNOWN",
                    format!("参考素材 {use_id} 不在这次冻结的素材清单里，请重新选择素材。"),
                )
            })?;
        references.push(json!({
            "referenceUseId": numbers.number(use_id).expect("selected reference row"),
            "mediaKind": item.media_kind,
            "ordinalWithinKind": item.ordinal_within_kind,
            "audioUsage": item.audio_usage,
            "label": item.label,
            "bindingId": item.binding_id.as_deref().map(|id| numbers.number(id).unwrap_or(id)),
            "declaredIdentity": item.declared_identity,
        }));
    }
    let format_resources: Vec<Value> = request
        .format_method
        .resources
        .iter()
        .map(|(resource_ref, bytes)| {
            json!({
                "resourceRef": resource_ref,
                "sha256": hex_sha256(bytes),
                "text": String::from_utf8_lossy(bytes),
            })
        })
        .collect();
    let mut capability = declared_capability_evidence(&request.capability_snapshot, &target.input_mode);
    if let Some(ids) = capability.get_mut("referenceUseIds") {
        *ids = json!(references.iter().map(|reference| &reference["referenceUseId"]).collect::<Vec<_>>());
    }
    let mut payload = json!({
        "profileId": target.profile_id,
        "profileVersion": target.profile_version,
        "capabilitySnapshot": capability,
        "identityLocks": [],
        "expressionLanguage": language_wire(&target.expression_language),
        "formatMethod": {
            "identity": {
                "stableId": request.format_method.stable_id,
                "packageDigest": request.format_method.package_digest
            },
            "body": request.format_method.body,
            "resources": format_resources,
        },
        "inputMode": target.input_mode,
        "canonicalFingerprint": fingerprint,
        "shots": shots,
        "references": references,
    });
    if let Some(contract) = numbers.model_contract() {
        payload["referenceNumberContract"] = json!(contract);
    }
    Ok(payload)
}

/// Structural check: JSON object, serde shape, then `validate_projection_draft`.
/// The host's bgx_creator_contract schema walk is not in this crate.
pub fn parse_expression_reply(
    raw: &str,
    closed: &ProjectionClosedSet,
) -> Result<VideoPromptProjectionDraft, VideoExpressionRejected> {
    parse_numbered_expression_reply(raw, closed, None)
}

fn parse_numbered_expression_reply(
    raw: &str,
    closed: &ProjectionClosedSet,
    numbers: Option<&ReferenceNumbers>,
) -> Result<VideoPromptProjectionDraft, VideoExpressionRejected> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(VideoExpressionRejected {
            code: DRAFT_JSON.into(),
            detail: "empty reply".into(),
            raw_reply: Some(raw.to_string()),
        });
    }
    let value: Value = match serde_json::from_str(trimmed) {
        Ok(value @ Value::Object(_)) => value,
        Ok(_) => {
            return Err(VideoExpressionRejected {
                code: DRAFT_JSON.into(),
                detail: "reply is not a JSON object".into(),
                raw_reply: Some(raw.to_string()),
            })
        }
        Err(error) => {
            return Err(VideoExpressionRejected {
                code: DRAFT_JSON.into(),
                detail: error.to_string(),
                raw_reply: Some(raw.to_string()),
            })
        }
    };
    let mut draft: VideoPromptProjectionDraft = match serde_json::from_value(value) {
        Ok(draft) => draft,
        Err(error) => {
            return Err(VideoExpressionRejected {
                code: DRAFT_SHAPE.into(),
                detail: error.to_string(),
                raw_reply: Some(raw.to_string()),
            })
        }
    };
    if let Some(numbers) = numbers {
        map_draft_references(&mut draft, |value, path| {
            numbers.restore(value, path).map_err(|error| VideoExpressionRejected {
                code: error_codes::VIDEO_PROMPT_MODEL_REFERENCE_INVALID.into(),
                detail: format!("{}: {}", error_codes::VIDEO_PROMPT_MODEL_REFERENCE_INVALID, error),
                raw_reply: Some(raw.to_string()),
            })
        })?;
    }
    if let Err(error) = validate_projection_draft(&draft, closed) {
        return Err(VideoExpressionRejected {
            code: error.code().to_string(),
            detail: error.to_string(),
            raw_reply: Some(raw.to_string()),
        });
    }
    Ok(draft)
}

pub(crate) fn map_draft_references<E>(
    draft: &mut VideoPromptProjectionDraft,
    mut map: impl FnMut(&mut String, String) -> Result<(), E>,
) -> Result<(), E> {
    for (index, line) in draft.lines.iter_mut().enumerate() {
        for (position, value) in line.reference_use_ids.iter_mut().enumerate() {
            map(value, format!("lines[{index}].referenceUseIds[{position}]"))?;
        }
    }
    for (index, conflict) in draft.conflicts.iter_mut().enumerate() {
        for (position, value) in conflict.reference_use_ids.iter_mut().enumerate() {
            map(value, format!("conflicts[{index}].referenceUseIds[{position}]"))?;
        }
    }
    for (index, binding) in draft.speaker_voice_bindings.iter_mut().enumerate() {
        map(&mut binding.voice_reference_use_id, format!("speakerVoiceBindings[{index}].voiceReferenceUseId"))?;
    }
    Ok(())
}

pub fn run_video_expression<C: TextCompleter>(
    request: VideoExpressionRequest,
    completer: &mut C,
) -> Result<VideoExpressionResult, OptimizeError> {
    let numbers = reference_numbers(&request.references);
    run_numbered_video_expression(&request, &numbers, completer)
}

pub(crate) fn run_numbered_video_expression<C: TextCompleter>(
    request: &VideoExpressionRequest,
    numbers: &ReferenceNumbers,
    completer: &mut C,
) -> Result<VideoExpressionResult, OptimizeError> {
    production_registry()?.require(&request.profile_id, request.profile_version)?;
    if !production_registry()?.supports_input_mode(
        &request.profile_id,
        request.profile_version,
        &request.input_mode,
    ) {
        return Err(OptimizeError::new(
            error_codes::VIDEO_PROMPT_INPUT_MODE_UNSUPPORTED,
            format!(
                "档案 {}@{} 不支持模式 {}",
                request.profile_id, request.profile_version, request.input_mode
            ),
        ));
    }
    let compilers =
        compiler_registry().map_err(|error| OptimizeError::new(error.code(), error.to_string()))?;
    let projector = compilers
        .projector(&request.profile_id, request.profile_version)
        .ok_or_else(|| {
            OptimizeError::new(
                error_codes::VIDEO_PROMPT_PROJECTOR_MISSING,
                format!(
                    "没有 {}@{} 的生产投影器，不会改用通用档案。",
                    request.profile_id, request.profile_version
                ),
            )
        })?;
    check_method_resource_budget(&request.format_method)?;
    let target = target_from_request(request)?;
    let fingerprint = semantic_fingerprint(&request.mother);
    let mut payload = numbered_expression_payload(request, &target, &fingerprint, numbers)?;
    let references = request.references.iter().map(inventory_item).collect::<Result<Vec<_>, _>>()?;
    let voice_reference_ids = projector.speaker_voice_reference_use_ids(&target, &references);
    let subject_ids = request.identity_locks.iter()
        .filter(|lock| !lock.token.is_empty())
        .map(|lock| lock.subject_id.clone())
        .collect();
    let closed = ProjectionClosedSet::new(&request.mother, &target)
        .with_expansion_policy(request.expansion_policy.clone())
        .with_negative_context(request.negative_context.clone())
        .with_speaker_voice_binding(voice_reference_ids, subject_ids);
    let model_closed = numbers.model_closed_set(&closed);
    payload["speakerVoiceBinding"] = model_closed.speaker_voice_binding_offer();
    let schema = expression_schema::draft_schema_for(&model_closed);
    let snapshot = unpack_rules(&request.template)?;
    // Official video-expression request.json uses taskContext + outputContract only.
    let mut inputs = std::collections::BTreeMap::new();
    inputs.insert("taskContext".into(), payload);
    inputs.insert("outputContract".into(), schema.clone());
    let rendered = match render_request(&snapshot, EXPRESSION_TASK, &inputs) {
        Ok(messages) => messages,
        Err(first) => {
            let task = snapshot.tasks.keys().next().cloned().ok_or(first)?;
            render_request(&snapshot, &task, &inputs)?
        }
    };
    let mut messages = rendered
        .into_iter()
        .map(|message| TextMessage {
            role: message.role,
            content: Value::String(message.content),
        })
        .collect();
    crate::reference_guidance::apply(&mut messages, snapshot.language);
    completer.check_cancel()?;
    let reply = completer.complete(TextCall {
        messages,
        json_mode: true,
        schema: Some(schema),
        step_label: EXPRESSION_TASK.to_owned(),
        check_cancel: no_cancel,
        image_count_policy: crate::text_port::TextImageCountPolicy::ModelCapability,
    })?;
    completer.check_cancel()?;
    match parse_numbered_expression_reply(&reply.content, &closed, Some(numbers)) {
        Err(rejected) => Ok(VideoExpressionResult {
            draft: None,
            rejected: Some(rejected),
            final_prompt: None,
            source_map: Vec::new(),
            diagnostics: Vec::new(),
        }),
        Ok(draft) => {
            let (compile_document, compile_fingerprint) = expression_compile_document(
                &request.mother,
                &draft,
                request.expansion_policy.as_ref(),
            )?;
            match projector.compile(&ProjectionCompileInput {
                mother: ReviewedMother {
                    document: &compile_document,
                    canonical_fingerprint: &compile_fingerprint,
                },
                references: &references,
                identity_locks: &request.identity_locks,
                target: &target,
                method: request.method.as_ref(),
                expression: Some(&draft),
            }) {
                Ok(candidate) => Ok(VideoExpressionResult {
                    draft: Some(draft),
                    rejected: None,
                    final_prompt: Some(candidate.text),
                    source_map: candidate.source_map,
                    diagnostics: candidate.diagnostics,
                }),
                Err(error) => Err(OptimizeError::new(error.code(), error.to_string())
                    .with_raw_reply(reply.content)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use sha2::{Digest, Sha256};

    #[test]
    fn expression_rejects_fact_ids_outside_the_closed_set() {
        let mother: DocumentDraftV1 = serde_json::from_value(json!({"shots":[
            {"shotId":"s1","facts":[{"factId":"f1","kind":"action","text":"She walks.","semanticSlot":"action_interaction","polarity":"positive","provenance":"user_text","sourceFactId":null,"entityRefIds":[],"sourceUnitIds":["u1"]}],"quotes":[]}
        ]}))
        .unwrap();
        let target = VideoPromptTargetSnapshot {
            profile_id: "generic".into(),
            profile_version: 1,
            input_mode: "text_to_video".into(),
            duration_seconds: None,
            caps_digest: "a".repeat(64),
            expression_language: VideoPromptExpressionLanguage::Zh,
            reference_use_ids: vec![],
        };
        let closed = ProjectionClosedSet::new(&mother, &target);
        let raw = r#"{"lines":[{"shotId":"s1","factIds":["not-a-fact"],"quoteIds":[],"referenceUseIds":[],"text":"x"}],"conflicts":[]}"#;
        let rejected = parse_expression_reply(raw, &closed).expect_err("closed-set miss");
        assert!(
            rejected.detail.contains("not-a-fact")
                || rejected.code.contains("CLOSED")
                || rejected.code.contains("ID")
        );
        assert_eq!(rejected.raw_reply.as_deref(), Some(raw));
    }


    #[test]
    fn selected_quote_document_keeps_only_referenced_quotes() {
        let mother: DocumentDraftV1 = serde_json::from_value(json!({
            "shots":[{
                "shotId":"s1",
                "facts":[{"factId":"f1","kind":"action","text":"She walks.","semanticSlot":"action_interaction","polarity":"positive","provenance":"user_text","sourceFactId":null,"entityRefIds":[],"sourceUnitIds":["u1"]}],
                "quotes":[
                    {"quoteId":"q1","exactText":"Hi","role":"vocalized_dialogue","speakerId":null,"sourceUnitId":"u1"},
                    {"quoteId":"q2","exactText":"Bye","role":"vocalized_dialogue","speakerId":null,"sourceUnitId":"u1"}
                ]
            }]
        })).unwrap();
        let draft: VideoPromptProjectionDraft = serde_json::from_value(json!({
            "lines":[{"shotId":"s1","factIds":["f1"],"quoteIds":["q1"],"referenceUseIds":[],"text":"x"}],
            "conflicts":[]
        })).unwrap();
        let selected = selected_quote_document(&mother, &draft);
        assert_eq!(selected.shots[0].quotes.len(), 1);
        assert_eq!(selected.shots[0].quotes[0].quote_id, "q1");
    }

    #[test]
    fn semantic_fingerprint_uses_official_project_format_prefix() {
        let mother: DocumentDraftV1 = serde_json::from_value(json!({"shots":[
            {"shotId":"s1","facts":[{"factId":"f1","kind":"action","text":"She walks.","semanticSlot":"action_interaction","polarity":"positive","provenance":"user_text","sourceFactId":null,"entityRefIds":[],"sourceUnitIds":["u1"]}],"quotes":[]}
        ]})).unwrap();
        let raw = serde_json::to_string(&mother).unwrap();
        let mut hasher = Sha256::new();
        hasher.update(format!("v{}|{raw}", semantic_contract().project_format_version).as_bytes());
        let expected: String = hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect();
        assert_eq!(semantic_fingerprint(&mother), expected);
        assert_ne!(
            semantic_fingerprint(&mother),
            hex_sha256(&serde_json::to_vec(&mother).unwrap())
        );
    }

}
