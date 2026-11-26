//! 模块：domain::ports::video_prompt_projection
//! 作用：视频提示词「已审母版 → 目标格式候选」的中立投影 Port（VS08；架构 PLAN §5.1）。
//!
//! - 输入只有已审核母版（只读借用）、冻结参考用途、冻结身份锁行、目标能力快照、可选冻结方法与
//!   可选逐事实表达行；不接收 Db / Tauri / HTTP 客户端，也不接收厂牌、provider、工作流节点或
//!   展示名。
//! - 输出为候选文本、结构化来源映射、投影器身份与诊断；Port 没有母版或节点写入路径，写回仍由
//!   既有 canonical CAS owner 完成。
//! - 实现只在独立 `video_prompt_adapters/`，由应用装配层按 `profileId` 注册（VS11）；通用 Host
//!   只按目标快照里的 profileId 查表，不做厂牌条件分支。

use std::fmt;

use bgx_video_prompt_contract::document::DocumentDraftV1;
use bgx_video_prompt_contract::identity::IdentityLockLine;
use bgx_video_prompt_contract::inventory::ReferenceMediaInventoryItem;
use bgx_video_prompt_contract::projection::{
    VideoPromptFrozenMethod, VideoPromptProjectionDraft, VideoPromptTargetSnapshot,
};

/// Stable identity of one projector implementation. `profile_id` / `profile_version` equal the
/// target adapter capability declaration (`promptProjection.profileId` / `version`);
/// `compiler_revision` changes whenever the deterministic compile output changes and is part of
/// the projection fingerprint.
///
/// VS29 (CF-09 / CF-25 / CF-26): the identity also carries what the library binding rules and the
/// default method resolution need from the adapter side, so `agent_skills` never guesses them:
/// - `input_modes`: neutral mode ids (runtime-caps `implementedModeIds` ids) this profile version
///   compiles. The target capability still has to declare a mode before a run reaches the
///   projector; this list only says what the projector can compile.
/// - `default_resource_refs`: package resourceRefs the target's default method is bound to (e.g.
///   one shared package with one reference file per profile). Empty = SKILL.md body only.
/// - `mode_resource_refs`: optional per-mode subset of `default_resource_refs` (read through the
///   frozen binding with `read_bound_subset`, VS38). A mode without an entry reads the whole
///   `default_resource_refs` list.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProjectorIdentity {
    pub profile_id: String,
    pub profile_version: u64,
    pub compiler_revision: u32,
    pub input_modes: Vec<String>,
    pub default_resource_refs: Vec<String>,
    pub mode_resource_refs: Vec<ModeResourceRefs>,
}

/// Default resourceRefs subset one input mode reads (CF-25). Always a subset of
/// [`ProjectorIdentity::default_resource_refs`].
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ModeResourceRefs {
    pub input_mode: String,
    pub resource_refs: Vec<String>,
}

impl ProjectorIdentity {
    /// `true` when this projector compiles the neutral mode id.
    pub fn supports_input_mode(&self, input_mode: &str) -> bool {
        self.input_modes.iter().any(|m| m == input_mode)
    }

    /// Default resourceRefs one mode reads: its declared subset, else the full default list.
    pub fn resource_refs_for_mode(&self, input_mode: &str) -> &[String] {
        self.mode_resource_refs
            .iter()
            .find(|m| m.input_mode == input_mode)
            .map(|m| m.resource_refs.as_slice())
            .unwrap_or(self.default_resource_refs.as_slice())
    }

    /// Structural rules of the adapter declaration: modes non-empty, unique and in the neutral
    /// closed set; every per-mode subset names a declared mode once and only refs of
    /// `default_resource_refs`. Checked when the projector directory is built.
    pub fn declaration_is_consistent(&self, neutral_modes: &[&str]) -> bool {
        let modes_ok = !self.input_modes.is_empty()
            && self.input_modes.iter().enumerate().all(|(i, m)| {
                neutral_modes.contains(&m.as_str()) && !self.input_modes[..i].contains(m)
            });
        let subsets_ok = self.mode_resource_refs.iter().enumerate().all(|(i, s)| {
            self.supports_input_mode(&s.input_mode)
                && !self.mode_resource_refs[..i]
                    .iter()
                    .any(|o| o.input_mode == s.input_mode)
                && s.resource_refs
                    .iter()
                    .all(|r| self.default_resource_refs.contains(r))
        });
        modes_ok && subsets_ok
    }
}

/// Reviewed canonical document handed to a projector. Read-only: the projector can neither
/// author new facts nor write the document back.
#[derive(Clone, Copy)]
pub struct ReviewedMother<'a> {
    pub document: &'a DocumentDraftV1,
    /// Semantic fingerprint (sha256 hex) of `document` as issued by Rust.
    pub canonical_fingerprint: &'a str,
}

/// Everything a projector may read for one compile. All fields are frozen by the service before
/// the call; a projector never re-reads the library, the node or the target catalog.
#[derive(Clone, Copy)]
pub struct ProjectionCompileInput<'a> {
    pub mother: ReviewedMother<'a>,
    /// Frozen reference inventory of this run; `item_id` is the reference use id that
    /// `target.reference_use_ids` and expression lines refer to.
    pub references: &'a [ReferenceMediaInventoryItem],
    /// Frozen identity lock lines derived from the reviewed document and its bindings.
    pub identity_locks: &'a [IdentityLockLine],
    /// Service-frozen target capability snapshot (profile, version, mode, duration, caps digest,
    /// expression language, allowed reference uses).
    pub target: &'a VideoPromptTargetSnapshot,
    /// Frozen format method, when a method was selected and verified for this run.
    pub method: Option<&'a VideoPromptFrozenMethod>,
    /// Structurally validated per-fact expression lines from the method run (VS38/VS39);
    /// `None` for a pure deterministic compile without a model call.
    pub expression: Option<&'a VideoPromptProjectionDraft>,
}

/// What produced one span of the candidate text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProjectionSpanOrigin {
    /// Text of expression line `line_index` (covers the listed mother facts).
    ExpressionLine {
        line_index: usize,
        shot_id: String,
        fact_ids: Vec<String>,
    },
    /// A quote inserted verbatim from the mother by the projector.
    Quote { shot_id: String, quote_id: String },
    /// A reference tag rendered for one frozen reference use.
    ReferenceUse { reference_use_id: String },
    /// An identity lock line rendered by the projector.
    IdentityLock { shot_id: String, binding_id: String },
    /// Target-owned syntax (section labels, separators, time codes).
    TargetSyntax,
}

/// Byte range `[start, end)` of the candidate text and its origin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectionSourceSpan {
    pub start: usize,
    pub end: usize,
    pub origin: ProjectionSpanOrigin,
}

/// Non-blocking, structured note about the compile. Never a free-text verdict on model output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProjectionDiagnostic {
    /// A frozen reference use that the target accepts was not rendered by any line.
    ReferenceUseUnused { reference_use_id: String },
    /// A mother fact that no expression line or compiled fact covered.
    FactNotCovered { shot_id: String, fact_id: String },
}

/// Compile result. The service compares it with the mother closed set, runs model review and
/// decides candidate / writeback; the projector only reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectionCandidate {
    pub text: String,
    pub source_map: Vec<ProjectionSourceSpan>,
    pub projector: ProjectorIdentity,
    pub diagnostics: Vec<ProjectionDiagnostic>,
}

/// Closed set of reasons a projector refuses to compile. Each one is an explicit failure: the
/// caller keeps the current text and never retries with another projector or default method.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProjectionPortError {
    /// `target.profile_id` / `profile_version` is not the identity this projector compiles.
    ProfileMismatch {
        expected: Box<ProjectorIdentity>,
        profile_id: String,
        profile_version: u64,
    },
    /// The target input mode is not one this projector compiles.
    InputModeUnsupported { input_mode: String },
    /// This target needs method expression lines and none were supplied.
    ExpressionRequired,
    /// An id in the input is outside the frozen closed set it must belong to.
    UnknownId { kind: ProjectionIdKind, id: String },
    /// A frozen reference use cannot be expressed by this target.
    ReferenceUseUnsupported { reference_use_id: String },
    /// The target input mode writes a time anchor and the frozen target has no duration.
    TargetDurationRequired { input_mode: String },
    /// The target input mode needs a frozen reference in this role (inventory media kind).
    ReferenceRoleMissing { input_mode: String, role: String },
    /// A copied soundtrack needs exactly one visible performer; the frozen identity locks (or,
    /// without locks, the frozen pictures) name `count` candidates (CF-43).
    VocalPerformerAmbiguous { count: usize },
}

/// Which closed set an unknown id was checked against.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectionIdKind {
    Shot,
    Fact,
    Quote,
    ReferenceUse,
    Speaker,
    Subject,
}

impl ProjectionIdKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ProjectionIdKind::Shot => "shot",
            ProjectionIdKind::Fact => "fact",
            ProjectionIdKind::Quote => "quote",
            ProjectionIdKind::ReferenceUse => "referenceUse",
            ProjectionIdKind::Speaker => "speaker",
            ProjectionIdKind::Subject => "subject",
        }
    }
}

impl ProjectionPortError {
    /// Stable machine code for task records and receipts.
    pub fn code(&self) -> &'static str {
        match self {
            ProjectionPortError::ProfileMismatch { .. } => {
                "VIDEO_PROMPT_PROJECTION_PROFILE_MISMATCH"
            }
            ProjectionPortError::InputModeUnsupported { .. } => {
                "VIDEO_PROMPT_PROJECTION_INPUT_MODE_UNSUPPORTED"
            }
            ProjectionPortError::ExpressionRequired => {
                "VIDEO_PROMPT_PROJECTION_EXPRESSION_REQUIRED"
            }
            ProjectionPortError::UnknownId { .. } => "VIDEO_PROMPT_PROJECTION_UNKNOWN_ID",
            ProjectionPortError::ReferenceUseUnsupported { .. } => {
                "VIDEO_PROMPT_PROJECTION_REFERENCE_USE_UNSUPPORTED"
            }
            ProjectionPortError::TargetDurationRequired { .. } => {
                "VIDEO_PROMPT_PROJECTION_TARGET_DURATION_REQUIRED"
            }
            ProjectionPortError::ReferenceRoleMissing { .. } => {
                "VIDEO_PROMPT_PROJECTION_REFERENCE_ROLE_MISSING"
            }
            ProjectionPortError::VocalPerformerAmbiguous { .. } => {
                "VIDEO_PROMPT_PROJECTION_VOCAL_PERFORMER_AMBIGUOUS"
            }
        }
    }
}

impl fmt::Display for ProjectionPortError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProjectionPortError::ProfileMismatch {
                expected,
                profile_id,
                profile_version,
            } => write!(
                f,
                "{}: projector {}@{} cannot compile target {}@{}",
                self.code(),
                expected.profile_id,
                expected.profile_version,
                profile_id,
                profile_version
            ),
            ProjectionPortError::InputModeUnsupported { input_mode } => {
                write!(f, "{}: input mode {input_mode}", self.code())
            }
            ProjectionPortError::ExpressionRequired => f.write_str(self.code()),
            ProjectionPortError::UnknownId { kind, id } => {
                write!(f, "{}: {} id {id}", self.code(), kind.as_str())
            }
            ProjectionPortError::ReferenceUseUnsupported { reference_use_id } => {
                write!(f, "{}: reference use {reference_use_id}", self.code())
            }
            ProjectionPortError::TargetDurationRequired { input_mode } => {
                write!(f, "{}: input mode {input_mode}", self.code())
            }
            ProjectionPortError::ReferenceRoleMissing { input_mode, role } => {
                write!(f, "{}: input mode {input_mode} role {role}", self.code())
            }
            ProjectionPortError::VocalPerformerAmbiguous { count } => {
                write!(f, "{}: {count} performer candidates", self.code())
            }
        }
    }
}

impl std::error::Error for ProjectionPortError {}

/// How a target assembles expression lines into its final document (H-01). Every projector
/// declares it explicitly; there is no default, so a newly registered projector cannot inherit a
/// layout it does not compile.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LineOrganization {
    /// Lines are concatenated in order: sound and music belong on the line of the moment they
    /// happen, next to the action they accompany.
    Chronological,
    /// Lines are routed into target sections (e.g. picture / sound / music); a line whose facts
    /// all belong to one non-picture section leaves the picture section, so facts of different
    /// sections must be split into different lines.
    Sectioned,
}

impl LineOrganization {
    /// Stable wire / log value.
    pub fn as_str(self) -> &'static str {
        match self {
            LineOrganization::Chronological => "chronological",
            LineOrganization::Sectioned => "sectioned",
        }
    }
}

/// Neutral projection port. Implementations are deterministic compilers owned by independent
/// target adapters; they hold no database, window, network client or vendor enum.
pub trait VideoPromptProjectionPort: Send + Sync {
    /// Registry key (`profile_id`) and the version / compiler revision this projector compiles,
    /// with its implemented modes and default resourceRefs (VS29).
    fn identity(&self) -> ProjectorIdentity;

    /// The reference marker this target writes for the frozen reference of `media_kind`
    /// (inventory `media_kind`, e.g. `reference_image`) with `ordinal_within_kind` (1-based), built
    /// from the same template `compile` uses. `None` when this target writes no marker for that
    /// media kind (neutral targets, strict frames, kinds the target cannot tag) or for ordinal 0.
    /// Identity lock lines take their token from here (CF-21, VS43); no caller formats a vendor
    /// marker itself.
    fn reference_token(&self, media_kind: &str, ordinal_within_kind: u32) -> Option<String>;

    /// How this target's `compile` assembles expression lines (H-01). Expression and review
    /// prompts are generated from this declaration; no caller branches on `profile_id`.
    fn line_organization(&self) -> LineOrganization;

    /// Compile one candidate from frozen input. Must not author facts, drop facts silently or
    /// rewrite quotes; unsupported input is a [`ProjectionPortError`].
    fn compile(
        &self,
        input: &ProjectionCompileInput<'_>,
    ) -> Result<ProjectionCandidate, ProjectionPortError>;

    /// Reference uses of this run whose voice timbre a speaker may use. Empty when this target
    /// does not compile speaker voice bindings; the draft field is then closed.
    fn speaker_voice_reference_use_ids(
        &self,
        _target: &VideoPromptTargetSnapshot,
        _references: &[ReferenceMediaInventoryItem],
    ) -> Vec<String> {
        Vec::new()
    }
}

/// One structural profile check for every target projector, including supplied method binding.
pub fn validate_projection_profile(
    identity: &ProjectorIdentity,
    input: &ProjectionCompileInput<'_>,
) -> Result<(), ProjectionPortError> {
    let mismatch = |profile_id: &str, profile_version: u64| ProjectionPortError::ProfileMismatch {
        expected: Box::new(identity.clone()),
        profile_id: profile_id.into(),
        profile_version,
    };
    if input.target.profile_id != identity.profile_id
        || input.target.profile_version != identity.profile_version
    {
        return Err(mismatch(
            &input.target.profile_id,
            input.target.profile_version,
        ));
    }
    if let Some(method) = input.method {
        if method.binding_profile_id != identity.profile_id
            || method.binding_profile_version != identity.profile_version
        {
            return Err(mismatch(
                &method.binding_profile_id,
                method.binding_profile_version,
            ));
        }
    }
    Ok(())
}
