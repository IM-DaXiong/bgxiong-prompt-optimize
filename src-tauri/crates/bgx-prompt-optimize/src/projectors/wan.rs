//! 模块：video_prompt_adapters::wan
//! 作用：万相（Wan）视频目标的确定性提示词投影器（VS35，架构 PLAN §5.1 / §5.2）。
//!
//! - 只编译不创作：输入是审阅过的母版与方法按 VS02 合同产出的逐事实表达行，输出候选文本与
//!   结构化来源映射。台词原文按 quoteId 从母版 `exactText` 原样插入，模型不能改写。
//! - Wan 专属语法只在本文件：参考标记 图N / 视频N / 音频N（模板取自 cite-token 合同，序号用冻结
//!   清单的 `ordinal_within_kind`，按媒体类别分别计数）、多镜头「镜头N」分段；首帧 / 尾帧是请求里的
//!   严格画面，正文不写标记（docs/api/wan3.0.md：`first_frame`/`last_frame` 与 `reference_*` 互斥，
//!   提示词里的 图N 只指 `reference_image`）。
//! - 模式只编译有证据的四个：text_to_video / image_to_video / first_last_frame / reference_to_video。
//!   multi_image_to_video（生产 Wan caps 声明不支持）、last_frame_only（文档没有单独尾帧用法）以及
//!   模式闭集外的编辑 / 延长一律显式拒绝，不新增模式 id。
//! - 参考生视频里参考音只能作音色参考；音轨复用、用途未声明都拒绝。参考视频按文档参考生示例作
//!   素材参考（视频N），不带音频用途。
//! - 只注册 `wan@1`。其他服务商即使在旧 provider 分组里与 Wan 同组，也不按别名套用本投影器。

use std::collections::{BTreeMap, BTreeSet};

use crate::projectors::port::{
    ProjectionCandidate, ProjectionCompileInput, ProjectionDiagnostic, ProjectionIdKind,
    ProjectionPortError, ProjectionSourceSpan, ProjectionSpanOrigin, ProjectorIdentity,
    LineOrganization, VideoPromptProjectionPort,
};
use crate::projectors::cite_tokens::{
    apply_cite_template, CITE_TPL_WAN_AUDIO, CITE_TPL_WAN_IMAGE, CITE_TPL_WAN_VIDEO,
};
use bgx_video_prompt_contract::projection::VideoPromptExpressionLanguage;
use bgx_video_prompt_contract::document::{DocumentDraftV1, QuoteIntentV1};
use bgx_video_prompt_contract::identity::IdentityLockLine;

/// `promptProjection.profileId` this projector compiles.
pub const WAN_PROFILE_ID: &str = "wan";
/// `promptProjection.version` this projector compiles.
pub const WAN_PROFILE_VERSION: u64 = 1;
/// Raised whenever the deterministic output of this file changes (projection fingerprint).
const WAN_COMPILER_REVISION: u32 = 1;

/// Input modes this projector compiles (runtime-caps mode ids), carried into the projector
/// identity (VS29, CF-26); the target capability still has to declare the mode.
pub const WAN_INPUT_MODES: [&str; 4] = [
    "text_to_video",
    "image_to_video",
    "first_last_frame",
    "reference_to_video",
];

const MEDIA_FIRST_FRAME: &str = "first_frame";
const MEDIA_LAST_FRAME: &str = "last_frame";
const MEDIA_IMAGE: &str = "reference_image";
const MEDIA_VIDEO: &str = "reference_video";
const MEDIA_AUDIO: &str = "reference_audio";
const AUDIO_VOICE_TIMBRE: &str = "voice_timbre";

/// Input modes with Wan evidence (runtime-caps mode ids).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WanMode {
    Text,
    FirstFrame,
    FirstLastFrame,
    Reference,
}

impl WanMode {
    fn parse(id: &str) -> Option<Self> {
        match id {
            "text_to_video" => Some(WanMode::Text),
            "image_to_video" => Some(WanMode::FirstFrame),
            "first_last_frame" => Some(WanMode::FirstLastFrame),
            "reference_to_video" => Some(WanMode::Reference),
            _ => None,
        }
    }
}

/// How one frozen reference use appears in the Wan prompt.
#[derive(Clone, Debug, PartialEq, Eq)]
enum WanUse {
    /// Strict frame carried by the request media; the prompt carries no tag for it.
    Frame,
    /// Prompt tag (图N / 视频N / 音频N).
    Tag(String),
}

/// Target-owned syntax pieces for one expression language.
struct WanSyntax {
    en: bool,
}

impl WanSyntax {
    fn shot_header(&self, number: usize) -> String {
        if self.en {
            format!("Shot {number}")
        } else {
            format!("镜头{number}")
        }
    }

    fn tag_separator(&self) -> &'static str {
        if self.en {
            ", "
        } else {
            "、"
        }
    }

    fn tags_end(&self) -> &'static str {
        if self.en {
            ": "
        } else {
            "："
        }
    }

    fn quote_open(&self) -> &'static str {
        if self.en {
            " \""
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
    quotes: BTreeMap<&'a str, &'a QuoteIntentV1>,
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
                    quotes: shot
                        .quotes
                        .iter()
                        .map(|q| (q.quote_id.as_str(), q))
                        .collect(),
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

/// Wan marker template of one tagged media kind (cite-token contract). Shared by `compile` and
/// `reference_token`; strict frames and any other kind carry no marker.
fn tag_template(media_kind: &str) -> Option<&'static str> {
    match media_kind {
        MEDIA_IMAGE => Some(CITE_TPL_WAN_IMAGE),
        MEDIA_VIDEO => Some(CITE_TPL_WAN_VIDEO),
        MEDIA_AUDIO => Some(CITE_TPL_WAN_AUDIO),
        _ => None,
    }
}

/// Map every frozen reference use the target accepts to its Wan form; a use the mode cannot
/// express (wrong media kind, undeclared or soundtrack audio, duplicate frame / ordinal, missing
/// frozen ordinal) is an explicit refusal.
fn resolve_uses(
    input: &ProjectionCompileInput<'_>,
    mode: WanMode,
) -> Result<BTreeMap<String, WanUse>, ProjectionPortError> {
    let mut uses = BTreeMap::new();
    let mut frames: BTreeSet<&str> = BTreeSet::new();
    let mut ordinals: BTreeSet<(&str, u32)> = BTreeSet::new();
    for use_id in &input.target.reference_use_ids {
        if uses.contains_key(use_id) {
            continue;
        }
        let item = input
            .references
            .iter()
            .find(|r| &r.item_id == use_id)
            .ok_or_else(|| unknown(ProjectionIdKind::ReferenceUse, use_id))?;
        let unsupported = || ProjectionPortError::ReferenceUseUnsupported {
            reference_use_id: use_id.clone(),
        };
        let kind = item.media_kind.as_str();
        let wan_use = match (mode, kind) {
            (WanMode::FirstFrame | WanMode::FirstLastFrame, MEDIA_FIRST_FRAME)
            | (WanMode::FirstLastFrame, MEDIA_LAST_FRAME) => {
                if item.audio_usage.is_some() || !frames.insert(kind) {
                    return Err(unsupported());
                }
                WanUse::Frame
            }
            (WanMode::Reference, MEDIA_IMAGE | MEDIA_VIDEO | MEDIA_AUDIO) => {
                let usage_ok = if kind == MEDIA_AUDIO {
                    item.audio_usage.as_deref() == Some(AUDIO_VOICE_TIMBRE)
                } else {
                    item.audio_usage.is_none()
                };
                let ordinal = item.ordinal_within_kind;
                if !usage_ok || ordinal == 0 || !ordinals.insert((kind, ordinal)) {
                    return Err(unsupported());
                }
                let template = tag_template(kind).ok_or_else(unsupported)?;
                WanUse::Tag(apply_cite_template(template, ordinal))
            }
            _ => return Err(unsupported()),
        };
        uses.insert(use_id.clone(), wan_use);
    }
    Ok(uses)
}

/// Deterministic Wan projector registered as `wan@1`.
pub struct WanProjector;

impl VideoPromptProjectionPort for WanProjector {
    fn identity(&self) -> ProjectorIdentity {
        ProjectorIdentity {
            profile_id: WAN_PROFILE_ID.to_string(),
            profile_version: WAN_PROFILE_VERSION,
            compiler_revision: WAN_COMPILER_REVISION,
            // CF-26: four evidenced modes, no default resource (SKILL.md body only).
            input_modes: WAN_INPUT_MODES.iter().map(|m| m.to_string()).collect(),
            default_resource_refs: vec!["references/modes.md".into()],
            mode_resource_refs: Vec::new(),
        }
    }

    fn reference_token(&self, media_kind: &str, ordinal_within_kind: u32) -> Option<String> {
        if ordinal_within_kind == 0 {
            return None;
        }
        tag_template(media_kind).map(|template| apply_cite_template(template, ordinal_within_kind))
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
        let mode = WanMode::parse(&target.input_mode).ok_or_else(|| {
            ProjectionPortError::InputModeUnsupported {
                input_mode: target.input_mode.clone(),
            }
        })?;
        let uses = resolve_uses(input, mode)?;
        // Reference tags are placed per expression line; there is no fact-level Wan compile.
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
                .find(|id| !shot.quotes.contains_key(id.as_str()))
            {
                return Err(unknown(ProjectionIdKind::Quote, id));
            }
            if let Some(id) = line
                .reference_use_ids
                .iter()
                .find(|id| !uses.contains_key(id.as_str()))
            {
                return Err(unknown(ProjectionIdKind::ReferenceUse, id));
            }
            // Mother shot order is authoritative; lines keep their relative order inside a shot.
            lines_by_shot[shot.position].push(index);
        }

        let mut used: BTreeSet<&str> = BTreeSet::new();
        let mut locks_by_shot: Vec<Vec<&IdentityLockLine>> = vec![Vec::new(); document.shots.len()];
        for lock in input.identity_locks {
            let shot = shots
                .get(lock.shot_id.as_str())
                .ok_or_else(|| unknown(ProjectionIdKind::Shot, &lock.shot_id))?;
            // A lock line names its subject with a reference tag; it must be a tag this compile
            // renders (frame modes render none).
            // An empty token is a neutral lock line (this target writes no tag for the
            // reference, CF-21): written as text, bound to no use.
            if !lock.token.is_empty() {
                let use_id = uses
                    .iter()
                    .find_map(|(id, form)| match form {
                        WanUse::Tag(tag) if *tag == lock.token => Some(id.as_str()),
                        _ => None,
                    })
                    .ok_or_else(|| ProjectionPortError::ReferenceUseUnsupported {
                        reference_use_id: lock.binding_id.clone(),
                    })?;
                used.insert(use_id);
            }
            locks_by_shot[shot.position].push(lock);
        }

        let syntax = WanSyntax {
            en: matches!(
                target.expression_language,
                VideoPromptExpressionLanguage::En
            ),
        };
        let multi_shot = document.shots.len() > 1;
        let mut out = Compiled::default();
        let mut covered: BTreeSet<(&str, &str)> = BTreeSet::new();
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
                let mut tagged: BTreeSet<&str> = BTreeSet::new();
                for use_id in &line.reference_use_ids {
                    used.insert(use_id.as_str());
                    let Some(WanUse::Tag(tag)) = uses.get(use_id.as_str()) else {
                        continue;
                    };
                    if !tagged.insert(use_id.as_str()) {
                        continue;
                    }
                    if tagged.len() > 1 {
                        out.span(syntax.tag_separator(), ProjectionSpanOrigin::TargetSyntax);
                    }
                    out.span(
                        tag,
                        ProjectionSpanOrigin::ReferenceUse {
                            reference_use_id: use_id.clone(),
                        },
                    );
                }
                if !tagged.is_empty() {
                    out.span(syntax.tags_end(), ProjectionSpanOrigin::TargetSyntax);
                }
                out.span(
                    &line.text,
                    ProjectionSpanOrigin::ExpressionLine {
                        line_index: index,
                        shot_id: line.shot_id.clone(),
                        fact_ids: line.fact_ids.clone(),
                    },
                );
                for fact_id in &line.fact_ids {
                    covered.insert((shot_id, fact_id.as_str()));
                }
                for quote_id in &line.quote_ids {
                    if !quotes_done.insert(quote_id.as_str()) {
                        continue;
                    }
                    if let Some(quote) = shot.quotes.iter().find(|q| &q.quote_id == quote_id) {
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
            let tagged = matches!(uses.get(use_id.as_str()), Some(WanUse::Tag(_)));
            if tagged && !used.contains(use_id.as_str()) && reported.insert(use_id.as_str()) {
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

fn push_quote(out: &mut Compiled, syntax: &WanSyntax, shot_id: &str, quote: &QuoteIntentV1) {
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
    use std::sync::Arc;

    use serde_json::json;

    use super::*;
    use crate::projectors::port::ReviewedMother;
    use crate::projectors::VideoPromptProjectorRegistry;
    use bgx_video_prompt_contract::projection::{
        VideoPromptProjectionDraft, VideoPromptTargetSnapshot,
    };
    use bgx_video_prompt_contract::document::{
        validate_draft, PromptFactV1, ShotDraftV1,
    };
    use bgx_video_prompt_contract::inventory::ReferenceMediaInventoryItem;

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
            reference("use-img", MEDIA_IMAGE, 1, None),
            reference("use-voice", MEDIA_AUDIO, 1, Some(AUDIO_VOICE_TIMBRE)),
            reference("use-vid", MEDIA_VIDEO, 1, None),
            reference("use-img2", MEDIA_IMAGE, 2, None),
            reference("use-track", MEDIA_AUDIO, 2, Some("soundtrack_copy")),
            reference("use-first", MEDIA_FIRST_FRAME, 1, None),
            reference("use-last", MEDIA_LAST_FRAME, 1, None),
            reference("use-first-b", MEDIA_FIRST_FRAME, 2, None),
            reference("use-img0", MEDIA_IMAGE, 0, None),
            reference("use-vid-audio", MEDIA_VIDEO, 2, Some(AUDIO_VOICE_TIMBRE)),
            reference("use-audio-bare", MEDIA_AUDIO, 3, None),
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

    fn wan_target(mode: &str, uses: &[&str]) -> VideoPromptTargetSnapshot {
        target(WAN_PROFILE_ID, WAN_PROFILE_VERSION, mode, "zh", uses)
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

    fn reference_draft() -> VideoPromptProjectionDraft {
        draft(json!([
            line("s1", &["f1"], &[], &["use-img"], "expression one"),
            line(
                "s1",
                &["f2"],
                &["q1"],
                &["use-voice", "use-img"],
                "expression two"
            ),
            line("s2", &["f3"], &[], &["use-vid"], "expression three"),
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
        WanProjector.compile(&input)
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

    fn wan_tag(template: &str, ordinal: u32) -> String {
        apply_cite_template(template, ordinal)
    }

    #[test]
    fn wan_fixture_mother_is_a_reviewable_document() {
        assert_eq!(validate_draft(&mother(), false), Ok(()));
    }

    #[test]
    fn wan_compiles_lines_quotes_and_reference_tags_into_source_map() {
        let document = mother();
        let target = wan_target("reference_to_video", &["use-img", "use-voice", "use-vid"]);
        let expression = reference_draft();
        let candidate = compile_with(&document, &target, &[], Some(&expression)).unwrap();
        assert_eq!(candidate.projector, WanProjector.identity());
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

        let tags: Vec<(&str, &str)> = candidate
            .source_map
            .iter()
            .filter_map(|span| match &span.origin {
                ProjectionSpanOrigin::ReferenceUse { reference_use_id } => {
                    Some((reference_use_id.as_str(), slice(&candidate, span)))
                }
                _ => None,
            })
            .collect();
        let image = wan_tag(CITE_TPL_WAN_IMAGE, 1);
        let audio = wan_tag(CITE_TPL_WAN_AUDIO, 1);
        let video = wan_tag(CITE_TPL_WAN_VIDEO, 1);
        assert_eq!(
            tags,
            vec![
                ("use-img", image.as_str()),
                ("use-voice", audio.as_str()),
                ("use-img", image.as_str()),
                ("use-vid", video.as_str()),
            ]
        );
    }

    #[test]
    fn wan_target_syntax_is_closed_without_foreign_sections_or_tokens() {
        let document = mother();
        let target = wan_target("reference_to_video", &["use-img", "use-voice", "use-vid"]);
        let expression = reference_draft();
        let candidate = compile_with(&document, &target, &[], Some(&expression)).unwrap();
        assert_fully_mapped(&candidate);
        let syntax = WanSyntax { en: false };
        let allowed: BTreeSet<String> = [
            syntax.shot_header(1),
            syntax.shot_header(2),
            syntax.tag_separator().to_string(),
            syntax.tags_end().to_string(),
            syntax.quote_open().to_string(),
            syntax.quote_close().to_string(),
        ]
        .into_iter()
        .collect();
        let tags: BTreeSet<String> = [
            wan_tag(CITE_TPL_WAN_IMAGE, 1),
            wan_tag(CITE_TPL_WAN_AUDIO, 1),
            wan_tag(CITE_TPL_WAN_VIDEO, 1),
        ]
        .into_iter()
        .collect();
        for span in &candidate.source_map {
            let piece = slice(&candidate, span).to_string();
            match &span.origin {
                ProjectionSpanOrigin::TargetSyntax => assert!(allowed.contains(&piece)),
                ProjectionSpanOrigin::ReferenceUse { .. } => assert!(tags.contains(&piece)),
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
    fn wan_english_expression_uses_english_target_syntax() {
        let document = mother();
        let target = target(
            WAN_PROFILE_ID,
            WAN_PROFILE_VERSION,
            "reference_to_video",
            "en",
            &["use-img", "use-voice", "use-vid"],
        );
        let candidate = compile_with(&document, &target, &[], Some(&reference_draft())).unwrap();
        assert_fully_mapped(&candidate);
        let headers: Vec<&str> = candidate
            .source_map
            .iter()
            .filter(|span| span.origin == ProjectionSpanOrigin::TargetSyntax)
            .map(|span| slice(&candidate, span))
            .filter(|piece| piece.starts_with("Shot"))
            .collect();
        let syntax = WanSyntax { en: true };
        assert_eq!(headers, vec![syntax.shot_header(1), syntax.shot_header(2)]);
    }

    #[test]
    fn wan_rejects_modes_without_evidence_and_modes_outside_the_closed_set() {
        let document = mother();
        let expression = reference_draft();
        for mode in [
            "last_frame_only",
            "multi_image_to_video",
            "video_edit",
            "video_extend",
        ] {
            let target = wan_target(mode, &[]);
            assert_eq!(
                compile_with(&document, &target, &[], Some(&expression)),
                Err(ProjectionPortError::InputModeUnsupported {
                    input_mode: mode.to_string()
                })
            );
        }
    }

    #[test]
    fn wan_only_compiles_its_own_profile_and_version() {
        let document = mother();
        let expression = reference_draft();
        for (profile, version) in [("other.video", 1), (WAN_PROFILE_ID, 2)] {
            let target = target(profile, version, "text_to_video", "zh", &[]);
            assert!(matches!(
                compile_with(&document, &target, &[], Some(&expression)),
                Err(ProjectionPortError::ProfileMismatch { .. })
            ));
        }
        let registry = VideoPromptProjectorRegistry::new(vec![
            Arc::new(WanProjector) as Arc<dyn VideoPromptProjectionPort>
        ])
        .unwrap();
        assert!(registry
            .projector(WAN_PROFILE_ID, WAN_PROFILE_VERSION)
            .is_some());
        assert!(registry.projector(WAN_PROFILE_ID, 2).is_none());
        assert!(registry.projector("other.video", 1).is_none());
    }

    #[test]
    fn wan_reference_uses_follow_mode_and_audio_rules() {
        let document = mother();
        let unsupported = |id: &str| -> Result<ProjectionCandidate, ProjectionPortError> {
            Err(ProjectionPortError::ReferenceUseUnsupported {
                reference_use_id: id.to_string(),
            })
        };
        let only_text = draft(json!([
            line("s1", &["f1", "f2"], &["q1"], &[], "expression one"),
            line("s2", &["f3"], &[], &[], "expression three"),
        ]));
        let cases: [(&str, &[&str], &str); 8] = [
            ("text_to_video", &["use-img"], "use-img"),
            ("reference_to_video", &["use-track"], "use-track"),
            ("reference_to_video", &["use-audio-bare"], "use-audio-bare"),
            ("reference_to_video", &["use-first"], "use-first"),
            ("reference_to_video", &["use-img0"], "use-img0"),
            ("reference_to_video", &["use-vid-audio"], "use-vid-audio"),
            (
                "image_to_video",
                &["use-first", "use-first-b"],
                "use-first-b",
            ),
            ("image_to_video", &["use-last"], "use-last"),
        ];
        for (mode, uses, refused) in cases {
            let target = wan_target(mode, uses);
            assert_eq!(
                compile_with(&document, &target, &[], Some(&only_text)),
                unsupported(refused),
                "{mode} {uses:?}"
            );
        }

        let frames = draft(json!([
            line(
                "s1",
                &["f1", "f2"],
                &["q1"],
                &["use-first"],
                "expression one"
            ),
            line("s2", &["f3"], &[], &["use-last"], "expression three"),
        ]));
        let target = wan_target("first_last_frame", &["use-first", "use-last"]);
        let candidate = compile_with(&document, &target, &[], Some(&frames)).unwrap();
        assert_fully_mapped(&candidate);
        assert!(candidate.diagnostics.is_empty());
        assert!(!candidate
            .source_map
            .iter()
            .any(|span| matches!(span.origin, ProjectionSpanOrigin::ReferenceUse { .. })));
    }

    #[test]
    fn wan_requires_expression_lines_inside_the_frozen_closed_sets() {
        let document = mother();
        let target = wan_target("reference_to_video", &["use-img"]);
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
                line("s1", &["f1"], &[], &["use-vid"], "x"),
                ProjectionIdKind::ReferenceUse,
                "use-vid",
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
        let missing = wan_target("reference_to_video", &["use-not-frozen"]);
        assert_eq!(
            compile_with(&document, &missing, &[], Some(&reference_draft())),
            Err(ProjectionPortError::UnknownId {
                kind: ProjectionIdKind::ReferenceUse,
                id: "use-not-frozen".to_string()
            })
        );
    }

    #[test]
    fn wan_reports_uncovered_facts_and_unused_tags_and_still_inserts_every_quote() {
        let document = mother();
        let target = wan_target("reference_to_video", &["use-img", "use-img2"]);
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
                    reference_use_id: "use-img2".into()
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
    fn wan_identity_locks_must_name_a_rendered_tag() {
        let document = mother();
        let target = wan_target(
            "reference_to_video",
            &["use-img", "use-voice", "use-vid", "use-img2"],
        );
        let image2 = wan_tag(CITE_TPL_WAN_IMAGE, 2);
        let lock = IdentityLockLine {
            shot_id: "s2".into(),
            subject_id: "subject-a".into(),
            binding_id: "binding-a".into(),
            text: format!("lock text {image2}"),
            token: image2.clone(),
        };
        let expression = reference_draft();
        let candidate = compile_with(
            &document,
            &target,
            std::slice::from_ref(&lock),
            Some(&expression),
        )
        .unwrap();
        assert_fully_mapped(&candidate);
        let locks: Vec<(&ProjectionSpanOrigin, &str)> = candidate
            .source_map
            .iter()
            .filter(|span| matches!(span.origin, ProjectionSpanOrigin::IdentityLock { .. }))
            .map(|span| (&span.origin, slice(&candidate, span)))
            .collect();
        assert_eq!(
            locks,
            vec![(
                &ProjectionSpanOrigin::IdentityLock {
                    shot_id: "s2".into(),
                    binding_id: "binding-a".into()
                },
                lock.text.as_str()
            )]
        );
        // The lock uses the second image, so it is not reported unused.
        assert!(candidate.diagnostics.is_empty());

        let frame_target = wan_target("image_to_video", &["use-first"]);
        let frame_lines = draft(json!([
            line(
                "s1",
                &["f1", "f2"],
                &["q1"],
                &["use-first"],
                "expression one"
            ),
            line("s2", &["f3"], &[], &[], "expression three"),
        ]));
        assert_eq!(
            compile_with(
                &document,
                &frame_target,
                std::slice::from_ref(&lock),
                Some(&frame_lines)
            ),
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
