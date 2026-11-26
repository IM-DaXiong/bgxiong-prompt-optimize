//! Projection contract validation helpers + validate_projection_draft (R14c).

use std::collections::BTreeSet;
use std::fmt;
use std::sync::OnceLock;

use regex::Regex;
use serde_json::Value;

use crate::closed_set::ProjectionClosedSet;
use crate::projection::VideoPromptProjectionDraft;

const PROJECTION_SCHEMA: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../packages/app-contracts/v1/video-prompt-projection.schema.json"
));
const DRAFT_SCHEMA: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../packages/app-contracts/v1/video-prompt-projection-draft.schema.json"
));
const AGENT_SKILLS_CONTRACT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../packages/app-contracts/v1/agent-skills.json"
));

pub struct Limits {
    pub segment_node_id_max: usize,
    pub content_fingerprint_max: usize,
    pub binding_profile_id_max: usize,
    pub binding_profile_version_min: u64,
    pub frozen_resources_max: usize,
    pub target_profile_id_max: usize,
    pub target_profile_version_min: u64,
    pub target_input_mode_max: usize,
    pub target_reference_uses_max: usize,
    pub target_reference_use_id_max: usize,
    pub local_id_max: usize,
    pub line_fact_ids_min: usize,
    pub line_fact_ids_max: usize,
    pub line_quote_ids_max: usize,
    pub line_reference_use_ids_max: usize,
    pub conflict_fact_ids_max: usize,
    pub conflict_reference_use_ids_max: usize,
    pub draft_lines_min: usize,
    pub draft_lines_max: usize,
    pub draft_conflicts_max: usize,
    pub draft_speaker_voice_bindings_max: usize,
    pub finding_fact_ids_max: usize,
    pub finding_quote_ids_max: usize,
    pub finding_reference_use_ids_max: usize,
    pub review_findings_max: usize,
    pub activation_id: Regex,
    pub sha256_hex: Regex,
}

fn bound_u64(doc: &Value, pointer: &str) -> u64 {
    doc.pointer(pointer)
        .and_then(Value::as_u64)
        .unwrap_or_else(|| panic!("video prompt projection contract missing bound {pointer}"))
}

fn bound(doc: &Value, pointer: &str) -> usize {
    usize::try_from(bound_u64(doc, pointer))
        .unwrap_or_else(|_| panic!("video prompt projection contract bound {pointer} overflows"))
}

fn pattern(doc: &Value, pointer: &str) -> Regex {
    let source = doc
        .pointer(pointer)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("agent-skills contract missing pattern {pointer}"));
    Regex::new(source).unwrap_or_else(|e| panic!("agent-skills contract pattern {pointer}: {e}"))
}

pub fn limits() -> &'static Limits {
    static LIMITS: OnceLock<Limits> = OnceLock::new();
    LIMITS.get_or_init(|| {
        let p: Value = serde_json::from_str(PROJECTION_SCHEMA)
            .expect("validated video-prompt-projection.schema.json");
        let d: Value = serde_json::from_str(DRAFT_SCHEMA)
            .expect("validated video-prompt-projection-draft.schema.json");
        let a: Value =
            serde_json::from_str(AGENT_SKILLS_CONTRACT).expect("validated agent-skills.json");
        let cref = "/definitions/canonicalDocumentRef/properties";
        let method = "/definitions/frozenMethod/properties";
        let target = "/definitions/targetSnapshot/properties";
        let line = "/definitions/expressionLine/properties";
        let conflict = "/definitions/projectionConflict/properties";
        let draft = "/definitions/projectionDraft/properties";
        let finding = "/definitions/reviewFinding/properties";
        Limits {
            segment_node_id_max: bound(&p, &format!("{cref}/segmentNodeId/maxLength")),
            content_fingerprint_max: bound(&p, &format!("{cref}/contentFingerprint/maxLength")),
            binding_profile_id_max: bound(&p, &format!("{method}/bindingProfileId/maxLength")),
            binding_profile_version_min: bound_u64(
                &p,
                &format!("{method}/bindingProfileVersion/minimum"),
            ),
            frozen_resources_max: bound(&p, &format!("{method}/resources/maxItems")),
            target_profile_id_max: bound(&p, &format!("{target}/profileId/maxLength")),
            target_profile_version_min: bound_u64(&p, &format!("{target}/profileVersion/minimum")),
            target_input_mode_max: bound(&p, &format!("{target}/inputMode/maxLength")),
            target_reference_uses_max: bound(&p, &format!("{target}/referenceUseIds/maxItems")),
            target_reference_use_id_max: bound(
                &p,
                &format!("{target}/referenceUseIds/items/maxLength"),
            ),
            local_id_max: bound(&d, "/definitions/localId/maxLength"),
            line_fact_ids_min: bound(&d, &format!("{line}/factIds/minItems")),
            line_fact_ids_max: bound(&d, &format!("{line}/factIds/maxItems")),
            line_quote_ids_max: bound(&d, &format!("{line}/quoteIds/maxItems")),
            line_reference_use_ids_max: bound(&d, &format!("{line}/referenceUseIds/maxItems")),
            conflict_fact_ids_max: bound(&d, &format!("{conflict}/factIds/maxItems")),
            conflict_reference_use_ids_max: bound(
                &d,
                &format!("{conflict}/referenceUseIds/maxItems"),
            ),
            draft_lines_min: bound(&d, &format!("{draft}/lines/minItems")),
            draft_lines_max: bound(&d, &format!("{draft}/lines/maxItems")),
            draft_conflicts_max: bound(&d, &format!("{draft}/conflicts/maxItems")),
            draft_speaker_voice_bindings_max: bound(
                &d,
                &format!("{draft}/speakerVoiceBindings/maxItems"),
            ),
            finding_fact_ids_max: bound(&d, &format!("{finding}/factIds/maxItems")),
            finding_quote_ids_max: bound(&d, &format!("{finding}/quoteIds/maxItems")),
            finding_reference_use_ids_max: bound(
                &d,
                &format!("{finding}/referenceUseIds/maxItems"),
            ),
            review_findings_max: bound(
                &d,
                "/definitions/projectionReviewResult/properties/findings/maxItems",
            ),
            activation_id: pattern(&a, "/definitions/activationId/pattern"),
            sha256_hex: pattern(&a, "/definitions/sha256Hex/pattern"),
        }
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProjectionContractError {
    FieldNotAllowed {
        mode: &'static str,
        field: &'static str,
    },
    FieldMissing {
        mode: &'static str,
        field: &'static str,
    },
    ProjectExistingRequiresSegment,
    InvalidValue { path: String, reason: &'static str },
    CountOutOfRange {
        path: String,
        min: usize,
        max: usize,
        actual: usize,
    },
    NotInClosedSet { path: String, id: String },
}

impl ProjectionContractError {
    pub fn code(&self) -> &'static str {
        match self {
            ProjectionContractError::FieldNotAllowed { .. } => {
                "VIDEO_PROMPT_PROJECTION_FIELD_NOT_ALLOWED"
            }
            ProjectionContractError::FieldMissing { .. } => "VIDEO_PROMPT_PROJECTION_FIELD_MISSING",
            ProjectionContractError::ProjectExistingRequiresSegment => {
                "VIDEO_PROMPT_PROJECTION_PROJECT_EXISTING_REQUIRES_SEGMENT"
            }
            ProjectionContractError::InvalidValue { .. } => "VIDEO_PROMPT_PROJECTION_INVALID_VALUE",
            ProjectionContractError::CountOutOfRange { .. } => {
                "VIDEO_PROMPT_PROJECTION_COUNT_OUT_OF_RANGE"
            }
            ProjectionContractError::NotInClosedSet { .. } => {
                "VIDEO_PROMPT_PROJECTION_ID_NOT_IN_CLOSED_SET"
            }
        }
    }
}

impl fmt::Display for ProjectionContractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProjectionContractError::FieldNotAllowed { mode, field } => {
                write!(f, "{}: {field} is not allowed for mode {mode}", self.code())
            }
            ProjectionContractError::FieldMissing { mode, field } => {
                write!(f, "{}: {field} is required for mode {mode}", self.code())
            }
            ProjectionContractError::ProjectExistingRequiresSegment => f.write_str(self.code()),
            ProjectionContractError::InvalidValue { path, reason } => {
                write!(f, "{}: {path} ({reason})", self.code())
            }
            ProjectionContractError::CountOutOfRange {
                path,
                min,
                max,
                actual,
            } => write!(
                f,
                "{}: {path} has {actual}, allowed {min}..={max}",
                self.code()
            ),
            ProjectionContractError::NotInClosedSet { path, id } => {
                write!(f, "{}: {path} = {id}", self.code())
            }
        }
    }
}

impl std::error::Error for ProjectionContractError {}

pub type ContractResult = Result<(), ProjectionContractError>;

pub fn invalid(path: &str, reason: &'static str) -> ProjectionContractError {
    ProjectionContractError::InvalidValue {
        path: path.to_string(),
        reason,
    }
}

pub fn check_count(path: &str, actual: usize, min: usize, max: usize) -> ContractResult {
    if actual < min || actual > max {
        return Err(ProjectionContractError::CountOutOfRange {
            path: path.to_string(),
            min,
            max,
            actual,
        });
    }
    Ok(())
}

/// Model-authored prose has no application character ceiling.
pub fn check_nonempty_text(path: &str, value: &str) -> ContractResult {
    if value.is_empty() {
        return Err(invalid(path, "empty"));
    }
    Ok(())
}

pub fn check_text(path: &str, value: &str, max_chars: usize) -> ContractResult {
    check_nonempty_text(path, value)?;
    if value.chars().count() > max_chars {
        return Err(invalid(path, "tooLong"));
    }
    Ok(())
}

pub fn check_sha256_hex(path: &str, value: &str) -> ContractResult {
    if limits().sha256_hex.is_match(value) {
        Ok(())
    } else {
        Err(invalid(path, "sha256Hex"))
    }
}

pub fn check_min_u64(path: &str, value: u64, min: u64) -> ContractResult {
    if value < min {
        return Err(invalid(path, "belowMinimum"));
    }
    Ok(())
}

pub fn check_member(path: &str, id: &str, set: &BTreeSet<String>) -> ContractResult {
    check_text(path, id, limits().local_id_max)?;
    if !set.contains(id) {
        return Err(ProjectionContractError::NotInClosedSet {
            path: path.to_string(),
            id: id.to_string(),
        });
    }
    Ok(())
}

pub fn check_members(
    path: &str,
    ids: &[String],
    min: usize,
    max: usize,
    set: &BTreeSet<String>,
) -> ContractResult {
    check_count(path, ids.len(), min, max)?;
    for (index, id) in ids.iter().enumerate() {
        check_member(&format!("{path}[{index}]"), id, set)?;
    }
    Ok(())
}

pub fn validate_projection_draft(
    draft: &VideoPromptProjectionDraft,
    closed: &ProjectionClosedSet,
) -> ContractResult {
    let expanded = closed.expanded(draft)?;
    let closed = &expanded;
    let l = limits();
    check_count(
        "lines",
        draft.lines.len(),
        l.draft_lines_min,
        l.draft_lines_max,
    )?;
    for (index, line) in draft.lines.iter().enumerate() {
        let base = format!("lines[{index}]");
        let shot = closed.shot(&format!("{base}.shotId"), &line.shot_id)?;
        check_members(
            &format!("{base}.factIds"),
            &line.fact_ids,
            l.line_fact_ids_min,
            l.line_fact_ids_max,
            &shot.facts,
        )?;
        check_members(
            &format!("{base}.quoteIds"),
            &line.quote_ids,
            0,
            l.line_quote_ids_max,
            &shot.quotes,
        )?;
        check_members(
            &format!("{base}.referenceUseIds"),
            &line.reference_use_ids,
            0,
            l.line_reference_use_ids_max,
            &closed.reference_use_ids,
        )?;
        check_nonempty_text(&format!("{base}.text"), &line.text)?;
    }
    check_count("conflicts", draft.conflicts.len(), 0, l.draft_conflicts_max)?;
    for (index, conflict) in draft.conflicts.iter().enumerate() {
        let base = format!("conflicts[{index}]");
        check_members(
            &format!("{base}.factIds"),
            &conflict.fact_ids,
            0,
            l.conflict_fact_ids_max,
            &closed.all_facts,
        )?;
        check_members(
            &format!("{base}.referenceUseIds"),
            &conflict.reference_use_ids,
            0,
            l.conflict_reference_use_ids_max,
            &closed.reference_use_ids,
        )?;
    }
    check_count(
        "speakerVoiceBindings",
        draft.speaker_voice_bindings.len(),
        0,
        l.draft_speaker_voice_bindings_max,
    )?;
    let mut seen_speakers = BTreeSet::new();
    let mut seen_voices = BTreeSet::new();
    let mut seen_subjects = BTreeSet::new();
    for (index, binding) in draft.speaker_voice_bindings.iter().enumerate() {
        let base = format!("speakerVoiceBindings[{index}]");
        check_member(
            &format!("{base}.speakerId"),
            &binding.speaker_id,
            &closed.speaker_ids,
        )?;
        if !seen_speakers.insert(binding.speaker_id.clone()) {
            return Err(invalid(&format!("{base}.speakerId"), "duplicate"));
        }
        check_member(
            &format!("{base}.voiceReferenceUseId"),
            &binding.voice_reference_use_id,
            &closed.voice_reference_use_ids,
        )?;
        if !seen_voices.insert(binding.voice_reference_use_id.clone()) {
            return Err(invalid(
                &format!("{base}.voiceReferenceUseId"),
                "duplicate",
            ));
        }
        if let Some(subject_id) = binding.subject_id.as_ref() {
            check_member(
                &format!("{base}.subjectId"),
                subject_id,
                &closed.speaker_subject_ids,
            )?;
            if !seen_subjects.insert(subject_id.clone()) {
                return Err(invalid(&format!("{base}.subjectId"), "duplicate"));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{DocumentDraftV1, PromptFactV1, QuoteIntentV1, ShotDraftV1};
    use crate::projection::{
        VideoPromptExpressionLanguage, VideoPromptExpressionLine, VideoPromptProjectionDraft,
        VideoPromptSpeakerVoiceBinding, VideoPromptTargetSnapshot,
    };
    use crate::closed_set::ProjectionClosedSet;

    #[test]
    fn limits_load_and_local_id_bound() {
        let l = limits();
        assert!(l.local_id_max > 0);
        assert!(l.draft_lines_max >= l.draft_lines_min);
        assert_eq!(l.draft_speaker_voice_bindings_max, 32);
        assert!(check_count("x", 1, 1, 3).is_ok());
        assert!(check_count("x", 0, 1, 3).is_err());
    }

    fn mother_with_speakers() -> DocumentDraftV1 {
        DocumentDraftV1 {
            shots: vec![ShotDraftV1 {
                shot_id: "S1".into(),
                facts: vec![PromptFactV1 {
                    fact_id: "f1".into(),
                    kind: "dialogue".into(),
                    text: "says".into(),
                    semantic_slot: "picture".into(),
                    polarity: None,
                    provenance: "user_text".into(),
                    source_fact_id: None,
                    entity_ref_ids: vec![],
                    source_unit_ids: vec![],
                }],
                quotes: vec![
                    QuoteIntentV1 {
                        quote_id: "q1".into(),
                        exact_text: "你好".into(),
                        role: "dialogue".into(),
                        speaker_id: Some("sp1".into()),
                        source_unit_id: None,
                    },
                    QuoteIntentV1 {
                        quote_id: "q2".into(),
                        exact_text: "再见".into(),
                        role: "dialogue".into(),
                        speaker_id: Some("sp2".into()),
                        source_unit_id: None,
                    },
                ],
            }],
        }
    }

    fn target_with_refs(refs: &[&str]) -> VideoPromptTargetSnapshot {
        VideoPromptTargetSnapshot {
            caps_digest: "a".repeat(64),
            duration_seconds: None,
            expression_language: VideoPromptExpressionLanguage::Zh,
            input_mode: "reference_to_video".into(),
            profile_id: "p".into(),
            profile_version: 1,
            reference_use_ids: refs.iter().map(|s| (*s).to_string()).collect(),
        }
    }

    fn draft_with_bindings(
        bindings: Vec<VideoPromptSpeakerVoiceBinding>,
    ) -> VideoPromptProjectionDraft {
        VideoPromptProjectionDraft {
            lines: vec![VideoPromptExpressionLine {
                shot_id: "S1".into(),
                fact_ids: vec!["f1".into()],
                quote_ids: vec!["q1".into()],
                reference_use_ids: vec![],
                text: "line".into(),
            }],
            conflicts: vec![],
            proposed_facts: None,
            negative: None,
            speaker_voice_bindings: bindings,
        }
    }

    #[test]
    fn speaker_voice_bindings_closed_set_and_uniqueness() {
        let mother = mother_with_speakers();
        let target = target_with_refs(&["use-voice", "use-voice2", "use-img"]);
        let closed = ProjectionClosedSet::new(&mother, &target)
            .with_speaker_voice_binding(
                vec!["use-voice".into(), "use-voice2".into(), "use-missing".into()],
                vec!["subj-a".into(), "subj-b".into()],
            );
        assert_eq!(closed.speaker_ids(), vec!["sp1".to_string(), "sp2".to_string()]);
        assert_eq!(
            closed.voice_reference_use_ids(),
            vec!["use-voice".to_string(), "use-voice2".to_string()]
        );
        assert_eq!(
            closed.speaker_subject_ids(),
            vec!["subj-a".to_string(), "subj-b".to_string()]
        );

        let ok = draft_with_bindings(vec![
            VideoPromptSpeakerVoiceBinding {
                speaker_id: "sp1".into(),
                voice_reference_use_id: "use-voice".into(),
                subject_id: Some("subj-a".into()),
            },
            VideoPromptSpeakerVoiceBinding {
                speaker_id: "sp2".into(),
                voice_reference_use_id: "use-voice2".into(),
                subject_id: None,
            },
        ]);
        assert!(validate_projection_draft(&ok, &closed).is_ok());

        // Model prose must survive validation and serde without truncation, including Unicode.
        let mut long = ok.clone();
        long.lines[0].text = "完整正文🙂\n".repeat(10_000);
        assert!(validate_projection_draft(&long, &closed).is_ok());
        let decoded: VideoPromptProjectionDraft =
            serde_json::from_str(&serde_json::to_string(&long).unwrap()).unwrap();
        assert_eq!(decoded.lines[0].text, long.lines[0].text);
        long.lines[0].text.clear();
        assert!(validate_projection_draft(&long, &closed).is_err());

        let closed_no_audio = ProjectionClosedSet::new(&mother, &target);
        let any_binding = draft_with_bindings(vec![VideoPromptSpeakerVoiceBinding {
            speaker_id: "sp1".into(),
            voice_reference_use_id: "use-voice".into(),
            subject_id: None,
        }]);
        let err = validate_projection_draft(&any_binding, &closed_no_audio).unwrap_err();
        assert!(matches!(
            err,
            ProjectionContractError::NotInClosedSet { .. }
        ));

        let dup_speaker = draft_with_bindings(vec![
            VideoPromptSpeakerVoiceBinding {
                speaker_id: "sp1".into(),
                voice_reference_use_id: "use-voice".into(),
                subject_id: None,
            },
            VideoPromptSpeakerVoiceBinding {
                speaker_id: "sp1".into(),
                voice_reference_use_id: "use-voice2".into(),
                subject_id: None,
            },
        ]);
        assert_eq!(
            validate_projection_draft(&dup_speaker, &closed).unwrap_err(),
            invalid("speakerVoiceBindings[1].speakerId", "duplicate")
        );

        let dup_voice = draft_with_bindings(vec![
            VideoPromptSpeakerVoiceBinding {
                speaker_id: "sp1".into(),
                voice_reference_use_id: "use-voice".into(),
                subject_id: None,
            },
            VideoPromptSpeakerVoiceBinding {
                speaker_id: "sp2".into(),
                voice_reference_use_id: "use-voice".into(),
                subject_id: None,
            },
        ]);
        assert_eq!(
            validate_projection_draft(&dup_voice, &closed).unwrap_err(),
            invalid("speakerVoiceBindings[1].voiceReferenceUseId", "duplicate")
        );

        let unknown_subject = draft_with_bindings(vec![VideoPromptSpeakerVoiceBinding {
            speaker_id: "sp1".into(),
            voice_reference_use_id: "use-voice".into(),
            subject_id: Some("subj-missing".into()),
        }]);
        let err = validate_projection_draft(&unknown_subject, &closed).unwrap_err();
        assert!(matches!(
            err,
            ProjectionContractError::NotInClosedSet { path, id }
                if path == "speakerVoiceBindings[0].subjectId" && id == "subj-missing"
        ));
    }
}
