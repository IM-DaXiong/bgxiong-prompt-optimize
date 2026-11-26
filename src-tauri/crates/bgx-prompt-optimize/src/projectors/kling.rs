//! 模块：video_prompt_adapters::kling
//! 作用：可灵（Kling）视频目标的确定性提示词投影器（VS34，架构 PLAN §5.1 / §5.2）。
//!
//! - 只编译不创作：输入是审阅过的母版与方法（默认标准包 `kling-prompt-writing`，经冻结方法提供）按
//!   VS02 合同产出的逐事实表达行，输出候选文本与结构化来源映射。台词原文按 quoteId 从母版
//!   `exactText` 原样插入，模型不能改写。本文件不读包正文，也不恢复 cfg(test) 的旧 optimizer。
//! - 三个显式目标共用同一个纯编译函数，方言只由 `profileId@version` 决定，从不按模型名、
//!   provider 或「名字含 omni」判断（C-21-1 / CF-11）：
//!   - `kling.classic@1` 经典版：参考标记 `@image_N`（cite 模板 `kling.classic@1`），只接受参考图片；
//!     模式 text_to_video / image_to_video / first_last_frame / multi_image_to_video。
//!   - `kling.omni@1` Omni 旧接口（docs/api/可灵AI-Omni-Video.md，`<<<image_1>>>` 写法）：
//!     `<<<image_N>>>` / `<<<video_N>>>`（cite 模板 `kling.omni@1`）。
//!   - `kling.omni@2` Omni 新接口（docs/api/可灵AI-Kling 3.0 & 3.0 Omni.md，「@image_1, @video_1」）：
//!     `@image_N` / `@video_N`（cite 模板 `kling.omni@2` 独立成条，不借经典版模板，CF-20 / CF-47）。
//!   - 两个 Omni 版本的模式：text_to_video / image_to_video / first_last_frame /
//!     multi_image_to_video（只接受参考图片）/ reference_to_video（参考图片 + 特征参考视频）。
//!
//!   哪个模型登记成哪个 profile 由 VS29 按目标能力声明（不在本文件）。
//! - 首帧 / 尾帧是请求里的严格画面，正文不写标记。仅尾帧（last_frame_only）三个目标都不编译：
//!   新接口文档写明「暂不支持仅尾帧」，旧接口与经典版也没有单独尾帧用法。
//! - 参考音：可灵各接口都没有参考音频输入类型，一律 ReferenceUseUnsupported；cite 模板也不给可灵
//!   参考音槽。参考视频只作特征参考（运镜、风格、接续镜头）；待编辑视频属于编辑，编辑 / 延长
//!   不在模式闭集（CF-10），显式 InputModeUnsupported，不新增模式 id。
//! - 主体库 element 不在引用来源闭集，本文件不编译 element 引用（C-21-3）。
//! - 多镜头：官方「镜头 n, m, words;」句式受 `settings.multi_shot` 与分镜时长约束，冻结目标快照没有
//!   这两项，所以这里只按母版镜头写「镜头N」分段，不生成官方多镜头句式（C-21-2，留给能力合同）。
//! - 台词用引号（中文“”、英文""），不用其他目标的花括号写法。
//! - 方法资源（N-1）：经典版只读 `references/classic.md`，Omni 版只读 `references/omni.md`；本文件
//!   导出 [`KlingTarget::default_resource_ref`]，由 VS29 写进投影器身份（CF-09 / CF-26）。
//! - 引用标记模板（VS29，CF-21）：`compile` 与 `reference_token` 共用 [`KlingTarget::tag_template`]，
//!   身份锁定行的 token 由此给出，调用方不自己拼可灵标记。

use std::collections::{BTreeMap, BTreeSet};

use crate::projectors::port::{
    ProjectionCandidate, ProjectionCompileInput, ProjectionDiagnostic, ProjectionIdKind,
    ProjectionPortError, ProjectionSourceSpan, ProjectionSpanOrigin, ProjectorIdentity,
    LineOrganization, VideoPromptProjectionPort,
};
use crate::projectors::cite_tokens::{
    apply_cite_template, CITE_TPL_KLING_CLASSIC_IMAGE, CITE_TPL_KLING_OMNI_IMAGE,
    CITE_TPL_KLING_OMNI_V2_IMAGE, CITE_TPL_KLING_OMNI_V2_VIDEO, CITE_TPL_KLING_OMNI_VIDEO,
};
use bgx_video_prompt_contract::projection::VideoPromptExpressionLanguage;
use bgx_video_prompt_contract::document::{DocumentDraftV1, QuoteIntentV1};
use bgx_video_prompt_contract::identity::IdentityLockLine;

/// `promptProjection.profileId` of the Kling classic target.
pub const KLING_CLASSIC_PROFILE_ID: &str = "kling.classic";
/// `promptProjection.version` of the Kling classic target.
pub const KLING_CLASSIC_PROFILE_VERSION: u64 = 1;
/// `promptProjection.profileId` of the Kling Omni targets.
pub const KLING_OMNI_PROFILE_ID: &str = "kling.omni";
/// Omni legacy interface (`<<<image_N>>>` dialect).
pub const KLING_OMNI_LEGACY_PROFILE_VERSION: u64 = 1;
/// Omni nextgen interface (`@image_N` dialect).
pub const KLING_OMNI_NEXTGEN_PROFILE_VERSION: u64 = 2;
/// Raised whenever the deterministic output of this file changes (projection fingerprint).
const KLING_COMPILER_REVISION: u32 = 1;

/// Method resource the classic target reads from `kling-prompt-writing`.
pub const KLING_CLASSIC_RESOURCE_REF: &str = "references/classic.md";
/// Method resource the Omni targets read from `kling-prompt-writing`.
pub const KLING_OMNI_RESOURCE_REF: &str = "references/omni.md";

/// Input modes the classic target compiles (runtime-caps mode ids).
pub const KLING_CLASSIC_INPUT_MODES: [&str; 4] = [
    "text_to_video",
    "image_to_video",
    "first_last_frame",
    "multi_image_to_video",
];
/// Input modes both Omni targets compile (runtime-caps mode ids).
pub const KLING_OMNI_INPUT_MODES: [&str; 5] = [
    "text_to_video",
    "image_to_video",
    "first_last_frame",
    "multi_image_to_video",
    "reference_to_video",
];

const MEDIA_FIRST_FRAME: &str = "first_frame";
const MEDIA_LAST_FRAME: &str = "last_frame";
const MEDIA_IMAGE: &str = "reference_image";
const MEDIA_VIDEO: &str = "reference_video";

/// One explicit Kling projection target. The dialect belongs to the target identity, never to a
/// model name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KlingTarget {
    Classic,
    OmniLegacy,
    OmniNextgen,
}

impl KlingTarget {
    /// Every Kling target, in registration order.
    #[cfg(test)]
    pub const ALL: [KlingTarget; 3] = [
        KlingTarget::Classic,
        KlingTarget::OmniLegacy,
        KlingTarget::OmniNextgen,
    ];

    /// `(promptProjection.profileId, version)` of this target.
    pub fn profile(self) -> (&'static str, u64) {
        match self {
            KlingTarget::Classic => (KLING_CLASSIC_PROFILE_ID, KLING_CLASSIC_PROFILE_VERSION),
            KlingTarget::OmniLegacy => (KLING_OMNI_PROFILE_ID, KLING_OMNI_LEGACY_PROFILE_VERSION),
            KlingTarget::OmniNextgen => (KLING_OMNI_PROFILE_ID, KLING_OMNI_NEXTGEN_PROFILE_VERSION),
        }
    }

    /// Input modes this target compiles, carried into the projector identity (VS29).
    pub fn input_modes(self) -> &'static [&'static str] {
        match self {
            KlingTarget::Classic => &KLING_CLASSIC_INPUT_MODES,
            KlingTarget::OmniLegacy | KlingTarget::OmniNextgen => &KLING_OMNI_INPUT_MODES,
        }
    }

    /// The single method resource this target reads (N-1), carried into the projector identity
    /// as the default resource ref (VS29).
    pub fn default_resource_ref(self) -> &'static str {
        match self {
            KlingTarget::Classic => KLING_CLASSIC_RESOURCE_REF,
            KlingTarget::OmniLegacy | KlingTarget::OmniNextgen => KLING_OMNI_RESOURCE_REF,
        }
    }

    fn image_template(self) -> &'static str {
        match self {
            KlingTarget::Classic => CITE_TPL_KLING_CLASSIC_IMAGE,
            KlingTarget::OmniLegacy => CITE_TPL_KLING_OMNI_IMAGE,
            KlingTarget::OmniNextgen => CITE_TPL_KLING_OMNI_V2_IMAGE,
        }
    }

    /// Feature reference video tag; the classic target takes no reference video.
    fn video_template(self) -> Option<&'static str> {
        match self {
            KlingTarget::Classic => None,
            KlingTarget::OmniNextgen => Some(CITE_TPL_KLING_OMNI_V2_VIDEO),
            KlingTarget::OmniLegacy => Some(CITE_TPL_KLING_OMNI_VIDEO),
        }
    }

    /// Marker template of one tagged media kind in this target's dialect (shared by `compile`
    /// and `reference_token`). Strict frames, reference audio (no Kling audio input) and any
    /// other kind carry no marker.
    fn tag_template(self, media_kind: &str) -> Option<&'static str> {
        match media_kind {
            MEDIA_IMAGE => Some(self.image_template()),
            MEDIA_VIDEO => self.video_template(),
            _ => None,
        }
    }
}

/// Input modes with Kling evidence (runtime-caps mode ids).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KlingMode {
    Text,
    FirstFrame,
    FirstLastFrame,
    MultiImage,
    Reference,
}

impl KlingMode {
    /// Parse a mode id for one target; a mode the target does not list is not compiled.
    fn parse(target: KlingTarget, id: &str) -> Option<Self> {
        if !target.input_modes().contains(&id) {
            return None;
        }
        match id {
            "text_to_video" => Some(KlingMode::Text),
            "image_to_video" => Some(KlingMode::FirstFrame),
            "first_last_frame" => Some(KlingMode::FirstLastFrame),
            "multi_image_to_video" => Some(KlingMode::MultiImage),
            "reference_to_video" => Some(KlingMode::Reference),
            _ => None,
        }
    }
}

/// How one frozen reference use appears in the Kling prompt.
#[derive(Clone, Debug, PartialEq, Eq)]
enum KlingUse {
    /// Strict frame carried by the request media; the prompt carries no tag for it.
    Frame,
    /// Prompt tag in the target dialect.
    Tag(String),
}

/// Target-owned syntax pieces for one expression language.
struct KlingSyntax {
    en: bool,
}

impl KlingSyntax {
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

    /// Opening quote mark; in English a quote that follows text on the same row is separated by
    /// one space.
    fn quote_open(&self, after_text: bool) -> &'static str {
        match (self.en, after_text) {
            (true, true) => " \"",
            (true, false) => "\"",
            (false, _) => "“",
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
    row_has_text: bool,
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
        self.row_has_text = true;
    }

    /// Shots are separated by one blank line.
    fn start_block(&mut self) {
        if !self.text.is_empty() {
            self.text.push_str("\n\n");
        }
        self.row_open = false;
        self.row_has_text = false;
    }

    fn start_row(&mut self) {
        if self.row_open {
            self.text.push('\n');
        }
        self.row_open = true;
        self.row_has_text = false;
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

/// Map every frozen reference use the target accepts to its Kling form; a use the target and
/// mode cannot express (reference audio, wrong media kind, a media row carrying an audio usage,
/// duplicate frame / ordinal, missing frozen ordinal) is an explicit refusal.
fn resolve_uses(
    input: &ProjectionCompileInput<'_>,
    target: KlingTarget,
    mode: KlingMode,
) -> Result<BTreeMap<String, KlingUse>, ProjectionPortError> {
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
        if item.audio_usage.is_some() {
            return Err(unsupported());
        }
        let kind = item.media_kind.as_str();
        let template = match (mode, kind) {
            (KlingMode::FirstFrame | KlingMode::FirstLastFrame, MEDIA_FIRST_FRAME)
            | (KlingMode::FirstLastFrame, MEDIA_LAST_FRAME) => {
                if !frames.insert(kind) {
                    return Err(unsupported());
                }
                None
            }
            (KlingMode::MultiImage, MEDIA_IMAGE)
            | (KlingMode::Reference, MEDIA_IMAGE | MEDIA_VIDEO) => {
                match target.tag_template(kind) {
                    Some(template) => Some(template),
                    None => return Err(unsupported()),
                }
            }
            _ => return Err(unsupported()),
        };
        let kling_use = match template {
            None => KlingUse::Frame,
            Some(template) => {
                let ordinal = item.ordinal_within_kind;
                if ordinal == 0 || !ordinals.insert((kind, ordinal)) {
                    return Err(unsupported());
                }
                KlingUse::Tag(apply_cite_template(template, ordinal))
            }
        };
        uses.insert(use_id.clone(), kling_use);
    }
    Ok(uses)
}

/// Deterministic Kling projector for one explicit target (`kling.classic@1`, `kling.omni@1`,
/// `kling.omni@2`).
pub struct KlingProjector {
    target: KlingTarget,
}

impl KlingProjector {
    pub const fn new(target: KlingTarget) -> Self {
        KlingProjector { target }
    }
}

impl VideoPromptProjectionPort for KlingProjector {
    fn identity(&self) -> ProjectorIdentity {
        let (profile_id, profile_version) = self.target.profile();
        ProjectorIdentity {
            profile_id: profile_id.to_string(),
            profile_version,
            compiler_revision: KLING_COMPILER_REVISION,
            // CF-26: classic = t2v/i2v/flf/multi-image + references/classic.md; both Omni
            // versions add reference_to_video and read references/omni.md (N-1). Which models
            // declare which modes is the target capability (bindings.rs, CF-20).
            input_modes: self
                .target
                .input_modes()
                .iter()
                .map(|m| m.to_string())
                .collect(),
            default_resource_refs: vec![self.target.default_resource_ref().to_string()],
            mode_resource_refs: Vec::new(),
        }
    }

    fn reference_token(&self, media_kind: &str, ordinal_within_kind: u32) -> Option<String> {
        if ordinal_within_kind == 0 {
            return None;
        }
        self.target
            .tag_template(media_kind)
            .map(|template| apply_cite_template(template, ordinal_within_kind))
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
        let mode = KlingMode::parse(self.target, &target.input_mode).ok_or_else(|| {
            ProjectionPortError::InputModeUnsupported {
                input_mode: target.input_mode.clone(),
            }
        })?;
        let uses = resolve_uses(input, self.target, mode)?;
        // Reference tags are placed per expression line; there is no fact-level Kling compile.
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
            // renders in this target's dialect (frame modes render none).
            // An empty token is a neutral lock line (this target writes no tag for the
            // reference, CF-21): written as text, bound to no use.
            if !lock.token.is_empty() {
                let use_id = uses
                    .iter()
                    .find_map(|(id, form)| match form {
                        KlingUse::Tag(tag) if *tag == lock.token => Some(id.as_str()),
                        _ => None,
                    })
                    .ok_or_else(|| ProjectionPortError::ReferenceUseUnsupported {
                        reference_use_id: lock.binding_id.clone(),
                    })?;
                used.insert(use_id);
            }
            locks_by_shot[shot.position].push(lock);
        }

        let syntax = KlingSyntax {
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
                    let Some(KlingUse::Tag(tag)) = uses.get(use_id.as_str()) else {
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
            let tagged = matches!(uses.get(use_id.as_str()), Some(KlingUse::Tag(_)));
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

fn push_quote(out: &mut Compiled, syntax: &KlingSyntax, shot_id: &str, quote: &QuoteIntentV1) {
    let after_text = out.row_has_text;
    out.span(
        syntax.quote_open(after_text),
        ProjectionSpanOrigin::TargetSyntax,
    );
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
    use std::path::Path;
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

    const AUDIO_VOICE_TIMBRE: &str = "voice_timbre";
    const AUDIO_SOUNDTRACK_COPY: &str = "soundtrack_copy";
    const MEDIA_AUDIO: &str = "reference_audio";

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
            reference("use-vid", MEDIA_VIDEO, 1, None),
            reference("use-img2", MEDIA_IMAGE, 2, None),
            reference("use-voice", MEDIA_AUDIO, 1, Some(AUDIO_VOICE_TIMBRE)),
            reference("use-track", MEDIA_AUDIO, 2, Some(AUDIO_SOUNDTRACK_COPY)),
            reference("use-first", MEDIA_FIRST_FRAME, 1, None),
            reference("use-last", MEDIA_LAST_FRAME, 1, None),
            reference("use-first-b", MEDIA_FIRST_FRAME, 2, None),
            reference("use-img0", MEDIA_IMAGE, 0, None),
            reference("use-vid-audio", MEDIA_VIDEO, 2, Some(AUDIO_VOICE_TIMBRE)),
            reference("use-img-dup", MEDIA_IMAGE, 1, None),
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

    fn kling_target(kling: KlingTarget, mode: &str, uses: &[&str]) -> VideoPromptTargetSnapshot {
        let (profile, version) = kling.profile();
        target(profile, version, mode, "zh", uses)
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

    fn image_draft() -> VideoPromptProjectionDraft {
        draft(json!([
            line("s1", &["f1"], &[], &["use-img"], "expression one"),
            line(
                "s1",
                &["f2"],
                &["q1"],
                &["use-img2", "use-img"],
                "expression two"
            ),
            line("s2", &["f3"], &[], &["use-img2"], "expression three"),
        ]))
    }

    fn reference_draft() -> VideoPromptProjectionDraft {
        draft(json!([
            line("s1", &["f1"], &[], &["use-img"], "expression one"),
            line(
                "s1",
                &["f2"],
                &["q1"],
                &["use-vid", "use-img"],
                "expression two"
            ),
            line("s2", &["f3"], &[], &["use-vid"], "expression three"),
        ]))
    }

    fn only_text_draft() -> VideoPromptProjectionDraft {
        draft(json!([
            line("s1", &["f1", "f2"], &["q1"], &[], "expression one"),
            line("s2", &["f3"], &[], &[], "expression three"),
        ]))
    }

    fn compile_with(
        kling: KlingTarget,
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
        KlingProjector::new(kling).compile(&input)
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

    fn tags(candidate: &ProjectionCandidate) -> Vec<(&str, &str)> {
        candidate
            .source_map
            .iter()
            .filter_map(|span| match &span.origin {
                ProjectionSpanOrigin::ReferenceUse { reference_use_id } => {
                    Some((reference_use_id.as_str(), slice(candidate, span)))
                }
                _ => None,
            })
            .collect()
    }

    /// Everything except target syntax and reference tags: expression lines, quotes, locks.
    fn content(candidate: &ProjectionCandidate) -> Vec<(ProjectionSpanOrigin, String)> {
        candidate
            .source_map
            .iter()
            .filter(|span| {
                !matches!(
                    span.origin,
                    ProjectionSpanOrigin::TargetSyntax | ProjectionSpanOrigin::ReferenceUse { .. }
                )
            })
            .map(|span| (span.origin.clone(), slice(candidate, span).to_string()))
            .collect()
    }

    fn tag(template: &str, ordinal: u32) -> String {
        apply_cite_template(template, ordinal)
    }

    #[test]
    fn kling_fixture_mother_is_a_reviewable_document() {
        assert_eq!(validate_draft(&mother(), false), Ok(()));
    }

    #[test]
    fn kling_each_target_renders_its_own_dialect_and_the_same_content() {
        let document = mother();
        let uses = ["use-img", "use-img2"];
        let expression = image_draft();
        let mut contents = Vec::new();
        for (kling, image_template) in [
            (KlingTarget::Classic, CITE_TPL_KLING_CLASSIC_IMAGE),
            (KlingTarget::OmniLegacy, CITE_TPL_KLING_OMNI_IMAGE),
            (KlingTarget::OmniNextgen, CITE_TPL_KLING_OMNI_V2_IMAGE),
        ] {
            let target = kling_target(kling, "multi_image_to_video", &uses);
            let candidate =
                compile_with(kling, &document, &target, &[], Some(&expression)).unwrap();
            assert_eq!(candidate.projector, KlingProjector::new(kling).identity());
            assert!(candidate.diagnostics.is_empty(), "{kling:?}");
            assert_fully_mapped(&candidate);
            let image1 = tag(image_template, 1);
            let image2 = tag(image_template, 2);
            assert_eq!(
                tags(&candidate),
                vec![
                    ("use-img", image1.as_str()),
                    ("use-img2", image2.as_str()),
                    ("use-img", image1.as_str()),
                    ("use-img2", image2.as_str()),
                ],
                "{kling:?}"
            );
            contents.push(content(&candidate));
        }
        // Same mother facts, quotes and expression lines on every target; only syntax differs.
        assert_eq!(contents[0], contents[1]);
        assert_eq!(contents[1], contents[2]);
        let lines: Vec<String> = contents[0]
            .iter()
            .filter(|(origin, _)| matches!(origin, ProjectionSpanOrigin::ExpressionLine { .. }))
            .map(|(_, text)| text.clone())
            .collect();
        let expected: Vec<String> = expression.lines.iter().map(|l| l.text.clone()).collect();
        assert_eq!(lines, expected);
        let quotes: Vec<&str> = contents[0]
            .iter()
            .filter(|(origin, _)| matches!(origin, ProjectionSpanOrigin::Quote { .. }))
            .map(|(_, text)| text.as_str())
            .collect();
        assert_eq!(
            quotes,
            vec![document.shots[0].quotes[0].exact_text.as_str()]
        );
    }

    #[test]
    fn kling_omni_versions_split_the_reference_dialect_for_video_and_image() {
        let document = mother();
        let uses = ["use-img", "use-vid"];
        let expression = reference_draft();
        let mut contents = Vec::new();
        for (kling, image_template, video_template) in [
            (
                KlingTarget::OmniLegacy,
                CITE_TPL_KLING_OMNI_IMAGE,
                CITE_TPL_KLING_OMNI_VIDEO,
            ),
            (
                KlingTarget::OmniNextgen,
                CITE_TPL_KLING_OMNI_V2_IMAGE,
                CITE_TPL_KLING_OMNI_V2_VIDEO,
            ),
        ] {
            let target = kling_target(kling, "reference_to_video", &uses);
            let candidate =
                compile_with(kling, &document, &target, &[], Some(&expression)).unwrap();
            assert_fully_mapped(&candidate);
            assert!(candidate.diagnostics.is_empty(), "{kling:?}");
            let image = tag(image_template, 1);
            let video = tag(video_template, 1);
            assert_eq!(
                tags(&candidate),
                vec![
                    ("use-img", image.as_str()),
                    ("use-vid", video.as_str()),
                    ("use-img", image.as_str()),
                    ("use-vid", video.as_str()),
                ],
                "{kling:?}"
            );
            contents.push(content(&candidate));
        }
        assert_eq!(contents[0], contents[1]);
        assert_ne!(
            tag(CITE_TPL_KLING_OMNI_IMAGE, 1),
            tag(CITE_TPL_KLING_OMNI_V2_IMAGE, 1)
        );
        // The classic target has no reference-to-video mode at all.
        let classic = kling_target(KlingTarget::Classic, "reference_to_video", &uses);
        assert_eq!(
            compile_with(
                KlingTarget::Classic,
                &document,
                &classic,
                &[],
                Some(&expression)
            ),
            Err(ProjectionPortError::InputModeUnsupported {
                input_mode: "reference_to_video".into()
            })
        );
    }

    #[test]
    fn kling_target_identities_are_distinct_and_never_compile_another_target() {
        let projectors: Vec<Arc<dyn VideoPromptProjectionPort>> = KlingTarget::ALL
            .iter()
            .map(|kling| {
                Arc::new(KlingProjector::new(*kling)) as Arc<dyn VideoPromptProjectionPort>
            })
            .collect();
        let registry = VideoPromptProjectorRegistry::new(projectors).unwrap();
        for kling in KlingTarget::ALL {
            let (profile, version) = kling.profile();
            assert!(registry.projector(profile, version).is_some(), "{kling:?}");
        }
        assert!(registry.projector(KLING_CLASSIC_PROFILE_ID, 2).is_none());
        assert!(registry.projector(KLING_OMNI_PROFILE_ID, 3).is_none());
        assert_eq!(registry.identities().len(), KlingTarget::ALL.len());

        // A projector refuses every other Kling target: no silent downgrade to classic and no
        // Omni dialect chosen by a shared profile id alone.
        let document = mother();
        let expression = only_text_draft();
        for compiler in KlingTarget::ALL {
            for declared in KlingTarget::ALL {
                if compiler == declared {
                    continue;
                }
                let target = kling_target(declared, "text_to_video", &[]);
                assert!(
                    matches!(
                        compile_with(compiler, &document, &target, &[], Some(&expression)),
                        Err(ProjectionPortError::ProfileMismatch { .. })
                    ),
                    "{compiler:?} compiled {declared:?}"
                );
            }
            let other = target("other.video", 1, "text_to_video", "zh", &[]);
            assert!(matches!(
                compile_with(compiler, &document, &other, &[], Some(&expression)),
                Err(ProjectionPortError::ProfileMismatch { .. })
            ));
        }
    }

    #[test]
    fn kling_modes_follow_each_target_and_frames_carry_no_tag() {
        let document = mother();
        let expression = only_text_draft();
        for kling in KlingTarget::ALL {
            for mode in kling.input_modes() {
                assert!(KlingMode::parse(kling, mode).is_some(), "{kling:?} {mode}");
            }
            for mode in ["last_frame_only", "video_edit", "video_extend", ""] {
                let target = kling_target(kling, mode, &[]);
                assert_eq!(
                    compile_with(kling, &document, &target, &[], Some(&expression)),
                    Err(ProjectionPortError::InputModeUnsupported {
                        input_mode: mode.to_string()
                    }),
                    "{kling:?}"
                );
            }
            let frame_cases: [(&str, &[&str]); 2] = [
                ("image_to_video", &["use-first"]),
                ("first_last_frame", &["use-first", "use-last"]),
            ];
            for (mode, uses) in frame_cases {
                let target = kling_target(kling, mode, uses);
                let candidate =
                    compile_with(kling, &document, &target, &[], Some(&expression)).unwrap();
                assert_fully_mapped(&candidate);
                assert!(candidate.diagnostics.is_empty(), "{kling:?} {mode}");
                assert!(tags(&candidate).is_empty(), "{kling:?} {mode}");
            }
        }
        assert!(KlingMode::parse(KlingTarget::Classic, "reference_to_video").is_none());
    }

    #[test]
    fn kling_reference_uses_follow_target_and_mode_rules() {
        let document = mother();
        let only_text = only_text_draft();
        let unsupported = |id: &str| -> Result<ProjectionCandidate, ProjectionPortError> {
            Err(ProjectionPortError::ReferenceUseUnsupported {
                reference_use_id: id.to_string(),
            })
        };
        let cases: [(KlingTarget, &str, &[&str], &str); 13] = [
            (
                KlingTarget::Classic,
                "text_to_video",
                &["use-img"],
                "use-img",
            ),
            (
                KlingTarget::Classic,
                "multi_image_to_video",
                &["use-img", "use-vid"],
                "use-vid",
            ),
            (
                KlingTarget::OmniNextgen,
                "multi_image_to_video",
                &["use-img", "use-vid"],
                "use-vid",
            ),
            (
                KlingTarget::OmniLegacy,
                "reference_to_video",
                &["use-img", "use-voice"],
                "use-voice",
            ),
            (
                KlingTarget::OmniNextgen,
                "reference_to_video",
                &["use-img", "use-track"],
                "use-track",
            ),
            (
                KlingTarget::OmniLegacy,
                "reference_to_video",
                &["use-vid-audio"],
                "use-vid-audio",
            ),
            (
                KlingTarget::OmniNextgen,
                "reference_to_video",
                &["use-first"],
                "use-first",
            ),
            (
                KlingTarget::OmniLegacy,
                "reference_to_video",
                &["use-img0"],
                "use-img0",
            ),
            (
                KlingTarget::Classic,
                "multi_image_to_video",
                &["use-img", "use-img-dup"],
                "use-img-dup",
            ),
            (
                KlingTarget::Classic,
                "image_to_video",
                &["use-first", "use-first-b"],
                "use-first-b",
            ),
            (
                KlingTarget::OmniLegacy,
                "image_to_video",
                &["use-last"],
                "use-last",
            ),
            (
                KlingTarget::OmniNextgen,
                "first_last_frame",
                &["use-first", "use-img"],
                "use-img",
            ),
            (
                KlingTarget::Classic,
                "image_to_video",
                &["use-voice"],
                "use-voice",
            ),
        ];
        for (kling, mode, uses, refused) in cases {
            let target = kling_target(kling, mode, uses);
            assert_eq!(
                compile_with(kling, &document, &target, &[], Some(&only_text)),
                unsupported(refused),
                "{kling:?} {mode} {uses:?}"
            );
        }
    }

    #[test]
    fn kling_requires_expression_lines_inside_the_frozen_closed_sets() {
        let document = mother();
        let kling = KlingTarget::OmniNextgen;
        let target = kling_target(kling, "reference_to_video", &["use-img"]);
        assert_eq!(
            compile_with(kling, &document, &target, &[], None),
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
                compile_with(kling, &document, &target, &[], Some(&expression)),
                Err(ProjectionPortError::UnknownId {
                    kind,
                    id: id.to_string()
                })
            );
        }
        let missing = kling_target(kling, "reference_to_video", &["use-not-frozen"]);
        assert_eq!(
            compile_with(kling, &document, &missing, &[], Some(&reference_draft())),
            Err(ProjectionPortError::UnknownId {
                kind: ProjectionIdKind::ReferenceUse,
                id: "use-not-frozen".to_string()
            })
        );
    }

    #[test]
    fn kling_reports_uncovered_facts_and_unused_tags_and_quotes_with_target_marks() {
        let document = mother();
        let kling = KlingTarget::Classic;
        let target = kling_target(kling, "multi_image_to_video", &["use-img", "use-img2"]);
        let partial = draft(json!([line(
            "s1",
            &["f1"],
            &[],
            &["use-img"],
            "expression one"
        )]));
        let candidate = compile_with(kling, &document, &target, &[], Some(&partial)).unwrap();
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
        // The unlisted quote still appears, on its own row, wrapped by the target quote marks.
        let spans = &candidate.source_map;
        let position = spans
            .iter()
            .position(|span| matches!(span.origin, ProjectionSpanOrigin::Quote { .. }))
            .unwrap();
        let syntax = KlingSyntax { en: false };
        assert_eq!(
            slice(&candidate, &spans[position]),
            document.shots[0].quotes[0].exact_text
        );
        assert_eq!(
            slice(&candidate, &spans[position - 1]),
            syntax.quote_open(false)
        );
        assert_eq!(
            slice(&candidate, &spans[position + 1]),
            syntax.quote_close()
        );
    }

    #[test]
    fn kling_target_syntax_is_closed_in_both_expression_languages() {
        let document = mother();
        let uses = ["use-img", "use-vid"];
        let kling = KlingTarget::OmniLegacy;
        for (language, en) in [("zh", false), ("en", true)] {
            let (profile, version) = kling.profile();
            let target = target(profile, version, "reference_to_video", language, &uses);
            let expression = reference_draft();
            let candidate =
                compile_with(kling, &document, &target, &[], Some(&expression)).unwrap();
            assert_fully_mapped(&candidate);
            let syntax = KlingSyntax { en };
            let allowed: BTreeSet<String> = [
                syntax.shot_header(1),
                syntax.shot_header(2),
                syntax.tag_separator().to_string(),
                syntax.tags_end().to_string(),
                syntax.quote_open(false).to_string(),
                syntax.quote_open(true).to_string(),
                syntax.quote_close().to_string(),
            ]
            .into_iter()
            .collect();
            let dialect: BTreeSet<String> = [
                tag(CITE_TPL_KLING_OMNI_IMAGE, 1),
                tag(CITE_TPL_KLING_OMNI_VIDEO, 1),
            ]
            .into_iter()
            .collect();
            for span in &candidate.source_map {
                let piece = slice(&candidate, span).to_string();
                match &span.origin {
                    ProjectionSpanOrigin::TargetSyntax => {
                        assert!(allowed.contains(&piece), "{language} {piece}")
                    }
                    ProjectionSpanOrigin::ReferenceUse { .. } => {
                        assert!(dialect.contains(&piece), "{language} {piece}")
                    }
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
    }

    #[test]
    fn kling_identity_locks_must_name_a_tag_in_the_target_dialect() {
        let document = mother();
        let kling = KlingTarget::OmniNextgen;
        let target = kling_target(kling, "multi_image_to_video", &["use-img", "use-img2"]);
        let image2 = tag(CITE_TPL_KLING_OMNI_V2_IMAGE, 2);
        let lock = IdentityLockLine {
            shot_id: "s2".into(),
            subject_id: "subject-a".into(),
            binding_id: "binding-a".into(),
            text: format!("lock text {image2}"),
            token: image2.clone(),
        };
        let expression = image_draft();
        let candidate = compile_with(
            kling,
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

        // The same lock written in the legacy Omni dialect does not name a tag of this target.
        let legacy_token = tag(CITE_TPL_KLING_OMNI_IMAGE, 2);
        let legacy_lock = IdentityLockLine {
            text: format!("lock text {legacy_token}"),
            token: legacy_token,
            ..lock.clone()
        };
        assert_eq!(
            compile_with(
                kling,
                &document,
                &target,
                std::slice::from_ref(&legacy_lock),
                Some(&expression)
            ),
            Err(ProjectionPortError::ReferenceUseUnsupported {
                reference_use_id: "binding-a".into()
            })
        );
        let frame_target = kling_target(kling, "image_to_video", &["use-first"]);
        assert_eq!(
            compile_with(
                kling,
                &document,
                &frame_target,
                std::slice::from_ref(&lock),
                Some(&only_text_draft())
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
            compile_with(kling, &document, &target, &[stray], Some(&expression)),
            Err(ProjectionPortError::UnknownId {
                kind: ProjectionIdKind::Shot,
                id: "s9".into()
            })
        );
    }

    #[test]
    fn kling_default_resource_refs_exist_in_the_method_package() {
        let package = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../resources/builtin-agent-skills/kling-prompt-writing");
        for kling in KlingTarget::ALL {
            assert!(
                package.join(kling.default_resource_ref()).is_file(),
                "{kling:?}"
            );
        }
        assert_ne!(
            KlingTarget::Classic.default_resource_ref(),
            KlingTarget::OmniLegacy.default_resource_ref()
        );
        assert_eq!(
            KlingTarget::OmniLegacy.default_resource_ref(),
            KlingTarget::OmniNextgen.default_resource_ref()
        );
    }
}
