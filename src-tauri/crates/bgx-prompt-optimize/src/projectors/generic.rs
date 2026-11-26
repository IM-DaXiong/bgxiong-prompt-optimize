//! 模块：video_prompt_adapters::generic
//! 作用：中立 Generic 目标的确定性提示词投影器（VS36，架构 PLAN §5.1 / §5.2）。
//!
//! - 明确的 Generic 目标：宿主在模型没有专属 `promptProjection`、且原生登记表没有该种类时冻结
//!   `generic@1`（用户自选中立方法时也用它）。目录只按 `profileId@version` 精确查找；已经写出的
//!   专属声明若找不到投影器，是显式不可用，绝不回落到这里，本投影器也拒绝编译其他 profile。
//! - 只编译不创作：输入是审阅过的母版与方法按 VS02 合同产出的逐事实表达行，输出候选文本与
//!   结构化来源映射。台词原文按 quoteId 从母版 `exactText` 原样插入，模型不能改写。
//! - 不产生任何平台语法：没有引用标记、段落标签、时间码。旧生产 generic 分支用
//!   `strip_section_labels` 去掉的「画面：」等段落标签，这里从来不生成；参考用途只由表达行的
//!   `referenceUseIds` 结构化记录，正文按方法要求用自然语言指认。
//! - 身份锁行（旧 `prefix_lock_lines`）放在所属镜头块最前。锁行若带目标引用标记（`token`
//!   非空），中立目标无法渲染，显式拒绝。
//! - 模式：中立模式闭集（runtime-caps `implementedModeIds` = `INPUT_MODE_IDS`）内都可编译，
//!   Generic 不随模式改写语法；闭集外的编辑 / 延长显式拒绝，不新增模式 id（CF-10）。

use std::collections::{BTreeMap, BTreeSet};

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

/// `profileId` of the explicit neutral target.
pub const GENERIC_PROFILE_ID: &str = "generic";
/// `version` of the explicit neutral target.
pub const GENERIC_PROFILE_VERSION: u64 = 1;
/// Package name of the neutral default method (`generic-video-prompt-writing`).
pub const GENERIC_DEFAULT_METHOD_NAME: &str = "generic-video-prompt-writing";
/// Raised whenever the deterministic output of this file changes (projection fingerprint).
const GENERIC_COMPILER_REVISION: u32 = 1;

/// Neutral target syntax for one expression language: shot separation and quote marks only.
struct GenericSyntax {
    en: bool,
}

impl GenericSyntax {
    fn shot_header(&self, number: usize) -> String {
        if self.en {
            format!("Shot {number}")
        } else {
            format!("镜头{number}")
        }
    }

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

    /// Shots are separated by one blank line.
    fn start_block(&mut self) {
        if !self.text.is_empty() {
            self.text.push_str("\n\n");
        }
        self.row_open = false;
    }

    fn start_row(&mut self) {
        if self.row_open {
            self.text.push('\n');
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

/// Frozen reference uses of the target. Each must be a row of the frozen inventory; the neutral
/// target renders no tag for any of them, so media kind and audio usage do not change the text.
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

/// Deterministic neutral projector registered as `generic@1`.
pub struct GenericProjector;

impl VideoPromptProjectionPort for GenericProjector {
    fn identity(&self) -> ProjectorIdentity {
        ProjectorIdentity {
            profile_id: GENERIC_PROFILE_ID.to_string(),
            profile_version: GENERIC_PROFILE_VERSION,
            compiler_revision: GENERIC_COMPILER_REVISION,
            // CF-26: every neutral mode, no default resource (SKILL.md body only).
            input_modes: INPUT_MODE_IDS.iter().map(|m| m.to_string()).collect(),
            default_resource_refs: Vec::new(),
            mode_resource_refs: Vec::new(),
        }
    }

    /// The neutral target writes no reference marker for any media kind (CF-21: identity lock
    /// lines use the token-less neutral template).
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
        if !INPUT_MODE_IDS.contains(&target.input_mode.as_str()) {
            return Err(ProjectionPortError::InputModeUnsupported {
                input_mode: target.input_mode.clone(),
            });
        }
        let frozen = frozen_uses(target, input.references)?;
        // The neutral method writes the lines; there is no fact-level compile that would need a
        // second copy of the canonical renderer.
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

        let syntax = GenericSyntax {
            en: matches!(
                target.expression_language,
                VideoPromptExpressionLanguage::En
            ),
        };
        let multi_shot = document.shots.len() > 1;
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
            if multi_shot {
                out.start_row();
                out.span(
                    &syntax.shot_header(position + 1),
                    ProjectionSpanOrigin::TargetSyntax,
                );
            }
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

fn push_quote(out: &mut Compiled, syntax: &GenericSyntax, shot_id: &str, quote: &QuoteIntentV1) {
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
    use serde_json::json;

    use super::*;
    use crate::projectors::port::ReviewedMother;
    use crate::projectors::compiler_registry;
    use crate::projectors::wan::{WAN_PROFILE_ID, WAN_PROFILE_VERSION};
    use bgx_video_prompt_contract::projection::VideoPromptProjectionDraft;
    use bgx_video_prompt_contract::document::{
        contract, validate_draft, PromptFactV1, ShotDraftV1,
    };

    const ALL_USES: [&str; 4] = ["use-img", "use-voice", "use-track", "use-first"];

    fn fact(id: &str, text: &str) -> PromptFactV1 {
        PromptFactV1 {
            fact_id: id.into(),
            kind: "action".into(),
            text: text.into(),
            semantic_slot: "action_interaction".into(),
            polarity: Some("positive".into()),
            provenance: "user_text".into(),
            source_fact_id: None,
            entity_ref_ids: Vec::new(),
            source_unit_ids: vec![format!("unit-{id}")],
        }
    }

    fn mother() -> DocumentDraftV1 {
        DocumentDraftV1 {
            shots: vec![
                ShotDraftV1 {
                    shot_id: "s1".into(),
                    facts: vec![fact("f1", "mother fact one"), fact("f2", "mother fact two")],
                    quotes: vec![QuoteIntentV1 {
                        quote_id: "q1".into(),
                        exact_text: "Mother quote, verbatim!".into(),
                        role: "dialogue".into(),
                        speaker_id: None,
                        source_unit_id: Some("unit-q1".into()),
                    }],
                },
                ShotDraftV1 {
                    shot_id: "s2".into(),
                    facts: vec![fact("f3", "mother fact three")],
                    quotes: Vec::new(),
                },
            ],
        }
    }

    fn reference(
        id: &str,
        kind: &str,
        ordinal: u32,
        audio: Option<&str>,
    ) -> ReferenceMediaInventoryItem {
        ReferenceMediaInventoryItem {
            item_id: id.into(),
            media_kind: kind.into(),
            ordinal_within_kind: ordinal,
            audio_usage: audio.map(str::to_string),
            ..Default::default()
        }
    }

    fn references() -> Vec<ReferenceMediaInventoryItem> {
        vec![
            reference("use-img", "reference_image", 1, None),
            reference("use-voice", "reference_audio", 1, Some("voice_timbre")),
            reference("use-track", "reference_audio", 2, Some("soundtrack_copy")),
            reference("use-first", "first_frame", 1, None),
        ]
    }

    fn target(
        profile: &str,
        version: u64,
        mode: &str,
        language: &str,
        uses: &[&str],
    ) -> VideoPromptTargetSnapshot {
        serde_json::from_value(json!({
            "profileId": profile,
            "profileVersion": version,
            "inputMode": mode,
            "durationSeconds": 5.0,
            "capsDigest": "a".repeat(64),
            "expressionLanguage": language,
            "referenceUseIds": uses,
        }))
        .unwrap()
    }

    fn generic_target(mode: &str, uses: &[&str]) -> VideoPromptTargetSnapshot {
        target(
            GENERIC_PROFILE_ID,
            GENERIC_PROFILE_VERSION,
            mode,
            "zh",
            uses,
        )
    }

    fn draft(lines: serde_json::Value) -> VideoPromptProjectionDraft {
        serde_json::from_value(json!({ "lines": lines, "conflicts": [] })).unwrap()
    }

    fn line(
        shot: &str,
        facts: &[&str],
        quotes: &[&str],
        refs: &[&str],
        text: &str,
    ) -> serde_json::Value {
        json!({
            "shotId": shot,
            "factIds": facts,
            "quoteIds": quotes,
            "referenceUseIds": refs,
            "text": text,
        })
    }

    fn full_draft() -> VideoPromptProjectionDraft {
        draft(json!([
            line("s1", &["f1"], &[], &["use-img"], "expression one"),
            line(
                "s1",
                &["f2"],
                &["q1"],
                &["use-voice", "use-track"],
                "expression two"
            ),
            line("s2", &["f3"], &[], &["use-first"], "expression three"),
        ]))
    }

    fn compile_with(
        document: &DocumentDraftV1,
        target: &VideoPromptTargetSnapshot,
        locks: &[IdentityLockLine],
        expression: Option<&VideoPromptProjectionDraft>,
    ) -> Result<ProjectionCandidate, ProjectionPortError> {
        let refs = references();
        let input = ProjectionCompileInput {
            mother: ReviewedMother {
                document,
                canonical_fingerprint: "b",
            },
            references: &refs,
            identity_locks: locks,
            target,
            method: None,
            expression,
        };
        GenericProjector.compile(&input)
    }

    fn slice<'a>(candidate: &'a ProjectionCandidate, span: &ProjectionSourceSpan) -> &'a str {
        &candidate.text[span.start..span.end]
    }

    /// Spans are ordered, disjoint and cover every byte except row / block newlines: nothing in
    /// the candidate comes from outside the source map.
    fn assert_fully_mapped(candidate: &ProjectionCandidate) {
        let mut cursor = 0;
        for span in &candidate.source_map {
            assert!(span.start >= cursor && span.end > span.start);
            assert!(span.end <= candidate.text.len());
            assert!(candidate.text[cursor..span.start]
                .chars()
                .all(|c| c == '\n'));
            cursor = span.end;
        }
        assert!(candidate.text[cursor..].chars().all(|c| c == '\n'));
    }

    fn target_syntax(candidate: &ProjectionCandidate) -> Vec<&str> {
        candidate
            .source_map
            .iter()
            .filter(|span| span.origin == ProjectionSpanOrigin::TargetSyntax)
            .map(|span| slice(candidate, span))
            .collect()
    }

    #[test]
    fn generic_fixture_mother_is_a_reviewable_document() {
        assert_eq!(validate_draft(&mother(), false), Ok(()));
    }

    #[test]
    fn generic_compiles_lines_and_verbatim_quotes_without_reference_tags() {
        let document = mother();
        let target = generic_target("reference_to_video", &ALL_USES);
        let expression = full_draft();
        let candidate = compile_with(&document, &target, &[], Some(&expression)).unwrap();
        assert_eq!(candidate.projector, GenericProjector.identity());
        assert!(candidate.diagnostics.is_empty());
        assert_fully_mapped(&candidate);

        let lines: Vec<(usize, &str)> = candidate
            .source_map
            .iter()
            .filter_map(|span| match &span.origin {
                ProjectionSpanOrigin::ExpressionLine { line_index, .. } => {
                    Some((*line_index, slice(&candidate, span)))
                }
                _ => None,
            })
            .collect();
        let expected: Vec<(usize, &str)> = expression
            .lines
            .iter()
            .enumerate()
            .map(|(index, line)| (index, line.text.as_str()))
            .collect();
        assert_eq!(lines, expected);

        let quotes: Vec<(&str, &str)> = candidate
            .source_map
            .iter()
            .filter_map(|span| match &span.origin {
                ProjectionSpanOrigin::Quote { quote_id, .. } => {
                    Some((quote_id.as_str(), slice(&candidate, span)))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            quotes,
            vec![("q1", document.shots[0].quotes[0].exact_text.as_str())]
        );

        // Reference uses are recorded by the lines only; the neutral text carries no tag.
        assert!(!candidate
            .source_map
            .iter()
            .any(|span| matches!(span.origin, ProjectionSpanOrigin::ReferenceUse { .. })));
    }

    #[test]
    fn generic_target_syntax_is_closed_and_has_no_section_labels() {
        let document = mother();
        let target = generic_target("reference_to_video", &ALL_USES);
        let expression = full_draft();
        let candidate = compile_with(&document, &target, &[], Some(&expression)).unwrap();
        assert_fully_mapped(&candidate);
        let syntax = GenericSyntax { en: false };
        let allowed: BTreeSet<String> = [
            syntax.shot_header(1),
            syntax.shot_header(2),
            syntax.inline_gap().to_string(),
            syntax.quote_open().to_string(),
            syntax.quote_close().to_string(),
        ]
        .into_iter()
        .collect();
        // The canonical project format labels (画面： / Picture: …) are never target syntax here.
        let c = contract();
        for label in c.labels_zh.values().chain(c.labels_en.values()) {
            for piece in &allowed {
                assert!(
                    !piece.starts_with(&format!("{label}："))
                        && !piece.starts_with(&format!("{label}:")),
                    "{piece}"
                );
            }
        }
        for span in &candidate.source_map {
            let piece = slice(&candidate, span);
            match &span.origin {
                ProjectionSpanOrigin::TargetSyntax => assert!(allowed.contains(piece)),
                ProjectionSpanOrigin::ExpressionLine { line_index, .. } => {
                    assert_eq!(piece, expression.lines[*line_index].text)
                }
                ProjectionSpanOrigin::Quote { .. } => {
                    assert_eq!(piece, document.shots[0].quotes[0].exact_text)
                }
                other => panic!("unexpected origin {other:?}"),
            }
        }
    }

    #[test]
    fn generic_target_syntax_follows_expression_language() {
        let document = mother();
        let expression = full_draft();
        for (language, en) in [("zh", false), ("en", true)] {
            let target = target(
                GENERIC_PROFILE_ID,
                GENERIC_PROFILE_VERSION,
                "text_to_video",
                language,
                &ALL_USES,
            );
            let candidate = compile_with(&document, &target, &[], Some(&expression)).unwrap();
            assert_fully_mapped(&candidate);
            let syntax = GenericSyntax { en };
            let header_one = syntax.shot_header(1);
            let header_two = syntax.shot_header(2);
            let mut expected: Vec<&str> = vec![header_one.as_str()];
            if !syntax.inline_gap().is_empty() {
                expected.push(syntax.inline_gap());
            }
            expected.extend([
                syntax.quote_open(),
                syntax.quote_close(),
                header_two.as_str(),
            ]);
            assert_eq!(target_syntax(&candidate), expected, "{language}");
        }
    }

    #[test]
    fn generic_compiles_every_neutral_mode_and_rejects_modes_outside_the_closed_set() {
        let document = mother();
        let expression = full_draft();
        for mode in INPUT_MODE_IDS {
            let target = generic_target(mode, &ALL_USES);
            assert!(
                compile_with(&document, &target, &[], Some(&expression)).is_ok(),
                "{mode}"
            );
        }
        for mode in ["video_edit", "video_extend"] {
            let target = generic_target(mode, &ALL_USES);
            assert_eq!(
                compile_with(&document, &target, &[], Some(&expression)),
                Err(ProjectionPortError::InputModeUnsupported {
                    input_mode: mode.to_string()
                })
            );
        }
    }

    #[test]
    fn generic_only_compiles_its_own_profile_and_selected_targets_never_fall_back() {
        let document = mother();
        let expression = full_draft();
        for (profile, version) in [
            (WAN_PROFILE_ID, WAN_PROFILE_VERSION),
            (GENERIC_PROFILE_ID, 2),
            ("unknown.video", 1),
        ] {
            let target = target(profile, version, "text_to_video", "zh", &[]);
            assert!(matches!(
                compile_with(&document, &target, &[], Some(&expression)),
                Err(ProjectionPortError::ProfileMismatch { .. })
            ));
        }
        let registry = compiler_registry().unwrap();
        assert!(registry
            .projector(GENERIC_PROFILE_ID, GENERIC_PROFILE_VERSION)
            .is_some());
        assert!(registry.projector(GENERIC_PROFILE_ID, 2).is_none());
        assert!(registry.projector("unknown.video", 1).is_none());
        assert!(registry.is_registered_profile(GENERIC_PROFILE_ID, 1));
        assert!(!registry.is_registered_profile("unknown.video", 1));
    }

    #[test]
    fn generic_requires_expression_lines_inside_the_frozen_closed_sets() {
        let document = mother();
        let target = generic_target("reference_to_video", &["use-img"]);
        assert_eq!(
            compile_with(&document, &target, &[], None),
            Err(ProjectionPortError::ExpressionRequired)
        );
        let cases = [
            (
                line("s9", &["f1"], &[], &[], "x"),
                ProjectionIdKind::Shot,
                "s9",
            ),
            (
                line("s1", &["f3"], &[], &[], "x"),
                ProjectionIdKind::Fact,
                "f3",
            ),
            (
                line("s2", &["f3"], &["q1"], &[], "x"),
                ProjectionIdKind::Quote,
                "q1",
            ),
            (
                line("s1", &["f1"], &[], &["use-voice"], "x"),
                ProjectionIdKind::ReferenceUse,
                "use-voice",
            ),
        ];
        for (bad, kind, id) in cases {
            let expression = draft(json!([bad]));
            assert_eq!(
                compile_with(&document, &target, &[], Some(&expression)),
                Err(ProjectionPortError::UnknownId {
                    kind,
                    id: id.to_string()
                })
            );
        }
        let missing = generic_target("reference_to_video", &["use-not-frozen"]);
        assert_eq!(
            compile_with(&document, &missing, &[], Some(&full_draft())),
            Err(ProjectionPortError::UnknownId {
                kind: ProjectionIdKind::ReferenceUse,
                id: "use-not-frozen".to_string()
            })
        );
    }

    #[test]
    fn generic_reports_uncovered_facts_and_unused_uses_and_still_inserts_every_quote() {
        let document = mother();
        let target = generic_target("reference_to_video", &["use-img", "use-voice"]);
        let partial = draft(json!([line(
            "s1",
            &["f1"],
            &[],
            &["use-img"],
            "expression one"
        )]));
        let candidate = compile_with(&document, &target, &[], Some(&partial)).unwrap();
        assert_fully_mapped(&candidate);
        assert_eq!(
            candidate.diagnostics,
            vec![
                ProjectionDiagnostic::FactNotCovered {
                    shot_id: "s1".into(),
                    fact_id: "f2".into()
                },
                ProjectionDiagnostic::FactNotCovered {
                    shot_id: "s2".into(),
                    fact_id: "f3".into()
                },
                ProjectionDiagnostic::ReferenceUseUnused {
                    reference_use_id: "use-voice".into()
                },
            ]
        );
        let quotes: Vec<&str> = candidate
            .source_map
            .iter()
            .filter(|span| matches!(span.origin, ProjectionSpanOrigin::Quote { .. }))
            .map(|span| slice(&candidate, span))
            .collect();
        assert_eq!(
            quotes,
            vec![document.shots[0].quotes[0].exact_text.as_str()]
        );
    }

    #[test]
    fn generic_lock_lines_lead_their_shot_and_tagged_locks_are_refused() {
        let document = mother();
        let target = generic_target("reference_to_video", &ALL_USES);
        let expression = full_draft();
        let lock = IdentityLockLine {
            shot_id: "s2".into(),
            subject_id: "subject-a".into(),
            binding_id: "binding-a".into(),
            text: "neutral lock text".into(),
            token: String::new(),
        };
        let candidate = compile_with(
            &document,
            &target,
            std::slice::from_ref(&lock),
            Some(&expression),
        )
        .unwrap();
        assert_fully_mapped(&candidate);
        let origins: Vec<&ProjectionSpanOrigin> = candidate
            .source_map
            .iter()
            .map(|span| &span.origin)
            .collect();
        let lock_at = origins
            .iter()
            .position(|origin| matches!(origin, ProjectionSpanOrigin::IdentityLock { .. }))
            .unwrap();
        assert_eq!(
            origins[lock_at],
            &ProjectionSpanOrigin::IdentityLock {
                shot_id: "s2".into(),
                binding_id: "binding-a".into()
            }
        );
        assert_eq!(slice(&candidate, &candidate.source_map[lock_at]), lock.text);
        // Shot header of s2, then the lock row, then the shot's first line.
        assert_eq!(
            slice(&candidate, &candidate.source_map[lock_at - 1]),
            GenericSyntax { en: false }.shot_header(2)
        );
        assert!(matches!(
            origins[lock_at + 1],
            ProjectionSpanOrigin::ExpressionLine { line_index: 2, .. }
        ));

        let tagged = IdentityLockLine {
            token: "tag-1".into(),
            ..lock.clone()
        };
        assert_eq!(
            compile_with(&document, &target, &[tagged], Some(&expression)),
            Err(ProjectionPortError::ReferenceUseUnsupported {
                reference_use_id: "binding-a".into()
            })
        );
        let stray = IdentityLockLine {
            shot_id: "s9".into(),
            ..lock.clone()
        };
        assert_eq!(
            compile_with(&document, &target, &[stray], Some(&expression)),
            Err(ProjectionPortError::UnknownId {
                kind: ProjectionIdKind::Shot,
                id: "s9".into()
            })
        );
    }
}
