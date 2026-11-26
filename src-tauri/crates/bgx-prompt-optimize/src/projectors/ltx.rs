//! LTX-2.5 deterministic paragraph compiler. Quotes come verbatim from the reviewed mother;
//! every authored byte retains its UTF-8 source span. Layout changes invalidate projection caches.

use std::collections::{BTreeMap, BTreeSet};

#[cfg(test)]
use bgx_video_prompt_contract::descriptor::INPUT_MODE_IDS;

use crate::projectors::port::{
    ProjectionCandidate, ProjectionCompileInput, ProjectionDiagnostic, ProjectionIdKind,
    ProjectionPortError, ProjectionSourceSpan, ProjectionSpanOrigin, ProjectorIdentity,
    LineOrganization, VideoPromptProjectionPort,
};
use bgx_video_prompt_contract::projection::{
    VideoPromptExpressionLanguage, VideoPromptTargetSnapshot,
};
use bgx_video_prompt_contract::document::{DocumentDraftV1, QuoteIntentV1};
use bgx_video_prompt_contract::identity::IdentityLockLine;
use bgx_video_prompt_contract::inventory::ReferenceMediaInventoryItem;

/// `profileId` of the LTX target.
pub const LTX_PROFILE_ID: &str = "ltx";
/// Version of the LTX target.
pub const LTX_PROFILE_VERSION: u64 = 1;
/// Package name of the LTX default format method.
#[cfg(test)]
pub const LTX_DEFAULT_METHOD_NAME: &str = "ltx-2-5-prompt-writing";
/// Raised whenever the deterministic output of this file changes (projection fingerprint).
const LTX_COMPILER_REVISION: u32 = 3;
/// LTX guides frames through conditioning; the paragraph layout is the same for every mode and
/// the mode-specific writing lives in `references/modes.md`.
pub const LTX_INPUT_MODES: [&str; 3] = ["text_to_video", "image_to_video", "first_last_frame"];

/// LTX quote punctuation follows the frozen expression language.
struct LtxSyntax {
    en: bool,
}

impl LtxSyntax {
    /// Gap between a line and a quote inserted right after it.
    fn inline_gap(&self) -> &'static str {
        if self.en {
            " "
        } else {
            ""
        }
    }

    fn quote_open(&self) -> &'static str {
        if self.en {
            "\""
        } else {
            "“"
        }
    }

    fn quote_close(&self) -> &'static str {
        if self.en {
            "\""
        } else {
            "”"
        }
    }
}

/// Candidate text under construction with its source map.
#[derive(Default)]
struct Compiled {
    text: String,
    source_map: Vec<ProjectionSourceSpan>,
    row_open: bool,
}

impl Compiled {
    fn span(&mut self, piece: &str, origin: ProjectionSpanOrigin) {
        if piece.is_empty() {
            return;
        }
        let start = self.text.len();
        self.text.push_str(piece);
        self.source_map.push(ProjectionSourceSpan {
            start,
            end: self.text.len(),
            origin,
        });
    }

    /// Chronological shot blocks share a single paragraph; transitions belong to expression lines.
    fn start_block(&mut self) {
        if !self.text.is_empty() {
            self.text.push(' ');
        }
        self.row_open = false;
    }

    fn start_row(&mut self) {
        if self.row_open {
            self.text.push(' ');
        }
        self.row_open = true;
    }
}

/// Closed id sets of one mother shot.
struct ShotIndex<'a> {
    position: usize,
    facts: BTreeSet<&'a str>,
    quotes: BTreeSet<&'a str>,
}

fn index_mother(document: &DocumentDraftV1) -> BTreeMap<&str, ShotIndex<'_>> {
    document
        .shots
        .iter()
        .enumerate()
        .map(|(position, shot)| {
            (
                shot.shot_id.as_str(),
                ShotIndex {
                    position,
                    facts: shot.facts.iter().map(|f| f.fact_id.as_str()).collect(),
                    quotes: shot.quotes.iter().map(|q| q.quote_id.as_str()).collect(),
                },
            )
        })
        .collect()
}

fn unknown(kind: ProjectionIdKind, id: &str) -> ProjectionPortError {
    ProjectionPortError::UnknownId {
        kind,
        id: id.to_string(),
    }
}

/// Validate reference-use IDs against the frozen inventory; LTX renders no dedicated reference token.
fn frozen_uses<'a>(
    target: &'a VideoPromptTargetSnapshot,
    references: &[ReferenceMediaInventoryItem],
) -> Result<BTreeSet<&'a str>, ProjectionPortError> {
    let mut uses = BTreeSet::new();
    for use_id in &target.reference_use_ids {
        if !references.iter().any(|r| &r.item_id == use_id) {
            return Err(unknown(ProjectionIdKind::ReferenceUse, use_id));
        }
        uses.insert(use_id.as_str());
    }
    Ok(uses)
}

/// LTX owns its continuous paragraph layout; no Generic identity rewriting or title stripping.
pub struct LtxProjector;

impl VideoPromptProjectionPort for LtxProjector {
    fn identity(&self) -> ProjectorIdentity {
        ProjectorIdentity {
            profile_id: LTX_PROFILE_ID.to_string(),
            profile_version: LTX_PROFILE_VERSION,
            compiler_revision: LTX_COMPILER_REVISION,
            // Required mode guidance is read through the standard frozen resource binding.
            input_modes: LTX_INPUT_MODES.iter().map(|m| m.to_string()).collect(),
            default_resource_refs: vec!["references/modes.md".to_string()],
            mode_resource_refs: Vec::new(),
        }
    }

    /// LTX renders reference identity through ordinary prose, without dedicated tags.
    fn reference_token(&self, _media_kind: &str, _ordinal_within_kind: u32) -> Option<String> {
        None
    }

    fn line_organization(&self) -> LineOrganization {
        LineOrganization::Chronological
    }

    fn compile(
        &self,
        input: &ProjectionCompileInput<'_>,
    ) -> Result<ProjectionCandidate, ProjectionPortError> {
        let projector = self.identity();
        let target = input.target;
        crate::projectors::port::validate_projection_profile(
            &projector, input,
        )?;
        if !LTX_INPUT_MODES.contains(&target.input_mode.as_str()) {
            return Err(ProjectionPortError::InputModeUnsupported {
                input_mode: target.input_mode.clone(),
            });
        }
        let frozen = frozen_uses(target, input.references)?;
        // The method supplies natural transitions; this compiler owns chronological paragraph layout.
        let expression = input
            .expression
            .ok_or(ProjectionPortError::ExpressionRequired)?;
        let document = input.mother.document;
        let shots = index_mother(document);

        let mut lines_by_shot: Vec<Vec<usize>> = vec![Vec::new(); document.shots.len()];
        for (index, line) in expression.lines.iter().enumerate() {
            let shot = shots
                .get(line.shot_id.as_str())
                .ok_or_else(|| unknown(ProjectionIdKind::Shot, &line.shot_id))?;
            if let Some(id) = line
                .fact_ids
                .iter()
                .find(|id| !shot.facts.contains(id.as_str()))
            {
                return Err(unknown(ProjectionIdKind::Fact, id));
            }
            if let Some(id) = line
                .quote_ids
                .iter()
                .find(|id| !shot.quotes.contains(id.as_str()))
            {
                return Err(unknown(ProjectionIdKind::Quote, id));
            }
            if let Some(id) = line
                .reference_use_ids
                .iter()
                .find(|id| !frozen.contains(id.as_str()))
            {
                return Err(unknown(ProjectionIdKind::ReferenceUse, id));
            }
            // Mother shot order is authoritative; lines keep their relative order inside a shot.
            lines_by_shot[shot.position].push(index);
        }

        let mut locks_by_shot: Vec<Vec<&IdentityLockLine>> = vec![Vec::new(); document.shots.len()];
        for lock in input.identity_locks {
            let shot = shots
                .get(lock.shot_id.as_str())
                .ok_or_else(|| unknown(ProjectionIdKind::Shot, &lock.shot_id))?;
            // A lock line that names a target reference tag cannot be rendered by a target
            // without tags.
            if !lock.token.is_empty() {
                return Err(ProjectionPortError::ReferenceUseUnsupported {
                    reference_use_id: lock.binding_id.clone(),
                });
            }
            locks_by_shot[shot.position].push(lock);
        }

        let syntax = LtxSyntax {
            en: matches!(
                target.expression_language,
                VideoPromptExpressionLanguage::En
            ),
        };
        let mut out = Compiled::default();
        let mut covered: BTreeSet<(&str, &str)> = BTreeSet::new();
        let mut listed: BTreeSet<&str> = BTreeSet::new();
        for (position, shot) in document.shots.iter().enumerate() {
            let lines = &lines_by_shot[position];
            let locks = &locks_by_shot[position];
            if lines.is_empty() && locks.is_empty() && shot.quotes.is_empty() {
                continue;
            }
            let shot_id = shot.shot_id.as_str();
            out.start_block();
            for lock in locks {
                out.start_row();
                out.span(
                    &lock.text,
                    ProjectionSpanOrigin::IdentityLock {
                        shot_id: shot_id.to_string(),
                        binding_id: lock.binding_id.clone(),
                    },
                );
            }
            let mut quotes_done: BTreeSet<&str> = BTreeSet::new();
            for &index in lines {
                let line = &expression.lines[index];
                out.start_row();
                out.span(
                    &line.text,
                    ProjectionSpanOrigin::ExpressionLine {
                        line_index: index,
                        shot_id: line.shot_id.clone(),
                        fact_ids: line.fact_ids.clone(),
                    },
                );
                listed.extend(line.reference_use_ids.iter().map(String::as_str));
                covered.extend(line.fact_ids.iter().map(|id| (shot_id, id.as_str())));
                for quote_id in &line.quote_ids {
                    if !quotes_done.insert(quote_id.as_str()) {
                        continue;
                    }
                    if let Some(quote) = shot.quotes.iter().find(|q| &q.quote_id == quote_id) {
                        out.span(syntax.inline_gap(), ProjectionSpanOrigin::TargetSyntax);
                        push_quote(&mut out, &syntax, shot_id, quote);
                    }
                }
            }
            // Quotes are inserted from the mother even when no line listed them; never dropped.
            for quote in &shot.quotes {
                if quotes_done.insert(quote.quote_id.as_str()) {
                    out.start_row();
                    push_quote(&mut out, &syntax, shot_id, quote);
                }
            }
        }

        let mut diagnostics = Vec::new();
        for shot in &document.shots {
            for fact in &shot.facts {
                if !covered.contains(&(shot.shot_id.as_str(), fact.fact_id.as_str())) {
                    diagnostics.push(ProjectionDiagnostic::FactNotCovered {
                        shot_id: shot.shot_id.clone(),
                        fact_id: fact.fact_id.clone(),
                    });
                }
            }
        }
        let mut reported: BTreeSet<&str> = BTreeSet::new();
        for use_id in &target.reference_use_ids {
            if !listed.contains(use_id.as_str()) && reported.insert(use_id.as_str()) {
                diagnostics.push(ProjectionDiagnostic::ReferenceUseUnused {
                    reference_use_id: use_id.clone(),
                });
            }
        }

        Ok(ProjectionCandidate {
            text: out.text,
            source_map: out.source_map,
            projector,
            diagnostics,
        })
    }
}

fn push_quote(out: &mut Compiled, syntax: &LtxSyntax, shot_id: &str, quote: &QuoteIntentV1) {
    out.span(syntax.quote_open(), ProjectionSpanOrigin::TargetSyntax);
    out.span(
        &quote.exact_text,
        ProjectionSpanOrigin::Quote {
            shot_id: shot_id.to_string(),
            quote_id: quote.quote_id.clone(),
        },
    );
    out.span(syntax.quote_close(), ProjectionSpanOrigin::TargetSyntax);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ltx_identity_is_consistent_and_registered() {
        let identity = LtxProjector.identity();
        assert!(identity.declaration_is_consistent(INPUT_MODE_IDS));
        assert_eq!(identity.profile_id, LTX_PROFILE_ID);
        let registry = super::super::compiler_registry().expect("registry");
        assert!(registry.projector(LTX_PROFILE_ID, LTX_PROFILE_VERSION).is_some());
        assert!(registry.projector("generic", 1).is_some());
    }

    #[test]
    fn ltx_workflow_choice_names_the_ltx_skill() {
        let catalog: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../resources/video-prompt-adapters/workflow-projections.json"
        ))
        .unwrap();
        let choice = catalog["choices"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == "ltx@1")
            .expect("ltx@1 choice");
        assert_eq!(choice["descriptor"]["defaultMethodName"], LTX_DEFAULT_METHOD_NAME);
        let skill = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../resources/builtin-agent-skills")
            .join(LTX_DEFAULT_METHOD_NAME)
            .join("SKILL.md");
        assert!(skill.is_file());
    }

    #[test]
    fn continuous_paragraph_preserves_quote_bytes_and_source_offsets() {
        use crate::projectors::port::ReviewedMother;
        use bgx_video_prompt_contract::projection::{VideoPromptProjectionDraft, VideoPromptTargetSnapshot};
        use bgx_video_prompt_contract::document::DocumentDraftV1;
        use serde_json::json;
        let mother: DocumentDraftV1 = serde_json::from_value(json!({"shots":[
            {"shotId":"s1","facts":[{"factId":"f1","kind":"action","text":"She speaks.","semanticSlot":"action_interaction","polarity":"positive","provenance":"user_text","sourceFactId":null,"entityRefIds":[],"sourceUnitIds":["unit-f1"]}],"quotes":[{"quoteId":"q1","exactText":"你好，世界！","role":"dialogue","speakerId":null,"sourceUnitId":null}]},
            {"shotId":"s2","facts":[{"factId":"f2","kind":"action","text":"She turns.","semanticSlot":"action_interaction","polarity":"positive","provenance":"user_text","sourceFactId":null,"entityRefIds":[],"sourceUnitIds":["unit-f2"]}],"quotes":[]}
        ]})).unwrap();
        let expression: VideoPromptProjectionDraft = serde_json::from_value(json!({"lines":[
            {"shotId":"s2","factIds":["f2"],"quoteIds":[],"referenceUseIds":[],"text":"Then she turns."},
            {"shotId":"s1","factIds":["f1"],"quoteIds":["q1"],"referenceUseIds":[],"text":"她轻声说。"}
        ],"conflicts":[]})).unwrap();
        for mode in LTX_INPUT_MODES {
            let target: VideoPromptTargetSnapshot = serde_json::from_value(json!({
                "profileId":"ltx","profileVersion":1,"inputMode":mode,"durationSeconds":5.0,
                "capsDigest":"a".repeat(64),"expressionLanguage":"en","referenceUseIds":[]
            })).unwrap();
            let candidate = LtxProjector.compile(&ProjectionCompileInput {
                mother: ReviewedMother {document: &mother, canonical_fingerprint:"b"},
                references:&[], identity_locks:&[], target:&target, method:None, expression:Some(&expression),
            }).unwrap();
            assert_eq!(candidate.text, "她轻声说。 \"你好，世界！\" Then she turns.");
            let quote = candidate.source_map.iter().find(|span| matches!(span.origin, ProjectionSpanOrigin::Quote { .. })).unwrap();
            assert_eq!(&candidate.text[quote.start..quote.end], "你好，世界！");
            let mut end = 0;
            for span in &candidate.source_map {
                assert!(span.start >= end && span.end <= candidate.text.len());
                assert!(candidate.text.is_char_boundary(span.start) && candidate.text.is_char_boundary(span.end));
                end = span.end;
            }
        }
        assert_eq!(LtxProjector.identity().resource_refs_for_mode("image_to_video"), &["references/modes.md"]);
    }

}
