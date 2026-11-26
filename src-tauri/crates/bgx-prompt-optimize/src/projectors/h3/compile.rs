//! 模块：video_prompt_adapters::h3::compile
//! 作用：H3（MiniMax h3-prompt-writing）目标的唯一确定性编译器（VS31，架构 PLAN §5.1 / §5.2）。
//!
//! - 三段（T2VA / I2VA / FL2VA / L2VA）与六段（Ref2VA）布局、段名、段分隔与空段写法只在本文件
//!   声明；旧润色装配（`h3_preserve_polish::assemble`）、来源账本与字段表都取这里的布局，
//!   不另存段名。
//! - 只编译不创作：事实进哪一段只看母版事实的类型化 kind 与表达行的参考用途（[`route_section`]），
//!   不搜文本关键词；摘要前缀由冻结参考用途推出，摘要 / 保留分析正文只来自母版事实的表达行；
//!   空段写合同约定的 `N/A` 或声场占位，不写固定分析占位。
//! - 台词原文按 quoteId 从母版 `exactText` 原样插入（人声走 `h3::audio` 兼容句式
//!   `(Sx) … <d>[语言] …</d>`，可见文字 / 非人声引文用英文双引号）；说话人编号取冻结母版引文的
//!   speakerId（CF-43），不从投影文本反推。
//! - 参考音（仅 Ref2VA）：音频行来自目标冻结参考与本文件解析出的 <Audio N>；音轨复用时唯一演唱者
//!   来自冻结身份锁（无锁时唯一画面参考），多于一个即 `VocalPerformerAmbiguous`；各段音频句式在
//!   编译时写入，不做投影后的文本改写（CF-43）。
//! - 参考标记 <Picture N> / <Video N> / <Audio N> 取 cite-token 合同模板；画面编号与对齐头来自
//!   [`super::mode::plan_mode`]，首帧 / 尾帧在 H3 里同样写 <Picture N>。
//! - 资源子集（CF-25）：非参考模式只读 `references/base-en.txt`，参考模式再读 `references/ref-en.txt`。

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::projectors::port::{
    LineOrganization, ModeResourceRefs, ProjectionCandidate, ProjectionCompileInput,
    ProjectionDiagnostic, ProjectionIdKind, ProjectionPortError, ProjectionSourceSpan,
    ProjectionSpanOrigin, ProjectorIdentity, VideoPromptProjectionPort,
};
use crate::projectors::h3_audio_usage::{
    parse_explicit_h3_reference_audio_usage, H3_REFERENCE_AUDIO_USAGE_SOUNDTRACK_COPY,
    H3_REFERENCE_AUDIO_USAGE_VOICE_TIMBRE,
};
use super::{H3_PROFILE_ID, H3_PROFILE_VERSION};
use crate::projectors::cite_tokens::{
    apply_cite_template, CITE_TPL_H3_AUDIO, CITE_TPL_H3_IMAGE, CITE_TPL_H3_VIDEO,
};
use bgx_video_prompt_contract::projection::{
    VideoPromptSpeakerVoiceBinding, VideoPromptTargetSnapshot,
};
use bgx_video_prompt_contract::document::{DocumentDraftV1, QuoteIntentV1};
use bgx_video_prompt_contract::identity::IdentityLockLine;
use bgx_video_prompt_contract::inventory::ReferenceMediaInventoryItem;

use super::audio::{
    lip_sync_sentence, music_sentence, performer_definition_line, retention_lines,
    soundscape_sentence, speaker_label, summary_sentence, use_exactly_sentence, vocal_frame,
    voice_timbre_bound_parts, voice_timbre_unbound_line, H3AudioRow, H3AudioUse,
    H3SoundtrackPerformer, H3VocalRole, H3_PERFORMER_SUBJECT, H3_UNNAMED_SPEAKER,
};
use super::mode::{plan_mode, H3Mode, H3ModeError, H3ModePlan, H3PictureRole, H3_INPUT_MODES};

/// Raised whenever the deterministic output of this file changes (projection fingerprint).
const H3_COMPILER_REVISION: u32 = 3;

/// Project-authored base compatibility guide (every mode).
pub const H3_RESOURCE_BASE: &str = "references/base-en.txt";
/// Project-authored reference-mode compatibility guide (Ref2VA modes only).
pub const H3_RESOURCE_REF: &str = "references/ref-en.txt";

/// Field names come from `packages/app-contracts/v1/minimax-h3-cir-field-keys.json` (also read by
/// the UI highlighter) through the build.rs generator, which rejects a malformed contract.
mod cir_field_keys {
    include!(concat!(
        env!("OUT_DIR"),
        "/app_contracts_minimax_h3_cir_field_keys.rs"
    ));
}

/// h3@1 three-field layout (T2VA / I2VA / FL2VA / L2VA), in contract order.
pub const H3_BASE_SECTION_KEYS: &[&str] = cir_field_keys::CIR_BASE_KEYS;

/// h3@1 six-field layout (Ref2VA), in contract order.
pub const H3_REF_SECTION_KEYS: &[&str] = cir_field_keys::CIR_REF_KEYS;

// The adapter owns which field plays which role; the contract owns names and order.
const KEY_BASE_PICTURE: &str = H3_BASE_SECTION_KEYS[0];
const KEY_SOUNDSCAPE: &str = H3_BASE_SECTION_KEYS[1];
const KEY_MUSIC: &str = H3_BASE_SECTION_KEYS[2];
const KEY_SUBJECT_DEFINITIONS: &str = H3_REF_SECTION_KEYS[0];
const KEY_SUMMARY: &str = H3_REF_SECTION_KEYS[1];
const KEY_RETENTION_ANALYSIS: &str = H3_REF_SECTION_KEYS[2];
const KEY_REF_PICTURE: &str = H3_REF_SECTION_KEYS[3];

#[cfg(test)]
/// Every field label the two layouts write (pasted-back projections strip them as format).
pub const H3_LAYOUT_LABELS: [&str; 7] = [
    KEY_BASE_PICTURE,
    KEY_SOUNDSCAPE,
    KEY_MUSIC,
    KEY_SUBJECT_DEFINITIONS,
    KEY_SUMMARY,
    KEY_RETENTION_ANALYSIS,
    KEY_REF_PICTURE,
];

/// Picture-description field of a layout (`integrated_multimodal_description` /
/// `detailed_description`).
pub(super) fn picture_field_key(reference_layout: bool) -> &'static str {
    if reference_layout {
        KEY_REF_PICTURE
    } else {
        KEY_BASE_PICTURE
    }
}

/// Contract soundscape body when the mother declares no sound (it is not silence).
pub const H3_SOUNDSCAPE_UNSPECIFIED: &str =
    "No additional sound source is specified. This placeholder does not mean the scene is silent.";

/// Contract body of a field the mother has nothing for.
pub const H3_SECTION_NOT_APPLICABLE: &str = "N/A";

/// Separator between rendered fields.
pub const H3_SECTION_SEPARATOR: &str = "\n\n";

const MEDIA_IMAGE: &str = "reference_image";
const MEDIA_VIDEO: &str = "reference_video";
const MEDIA_AUDIO: &str = "reference_audio";

/// `key: ` head of one rendered field.
pub fn section_head(key: &str) -> String {
    format!("{key}: ")
}

/// Wire form of an H3 mode (`FrozenPolishSourceV1.prompt_mode`).
#[cfg(test)]
pub fn mode_wire(mode: H3Mode) -> &'static str {
    match mode {
        H3Mode::T2VA => "T2VA",
        H3Mode::I2VA => "I2VA",
        H3Mode::FL2VA => "FL2VA",
        H3Mode::L2VA => "L2VA",
        H3Mode::Ref2VA => "Ref2VA",
    }
}

/// Inverse of [`mode_wire`]; anything else is not an H3 mode.
#[cfg(test)]
pub fn parse_mode_wire(raw: &str) -> Option<H3Mode> {
    match raw.trim() {
        "T2VA" => Some(H3Mode::T2VA),
        "I2VA" => Some(H3Mode::I2VA),
        "FL2VA" => Some(H3Mode::FL2VA),
        "L2VA" => Some(H3Mode::L2VA),
        "Ref2VA" => Some(H3Mode::Ref2VA),
        _ => None,
    }
}

/// Field one mode writes, in layout order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum H3Section {
    SubjectDefinitions,
    Summary,
    RetentionAnalysis,
    /// `integrated_multimodal_description` (base) / `detailed_description` (Ref2VA).
    Picture,
    OverallSoundscape,
    NonDiegeticMusic,
}

const BASE_LAYOUT: [H3Section; 3] = [
    H3Section::Picture,
    H3Section::OverallSoundscape,
    H3Section::NonDiegeticMusic,
];

const REF_LAYOUT: [H3Section; 6] = [
    H3Section::SubjectDefinitions,
    H3Section::Summary,
    H3Section::RetentionAnalysis,
    H3Section::Picture,
    H3Section::OverallSoundscape,
    H3Section::NonDiegeticMusic,
];

/// Field order of one mode.
pub fn layout(mode: H3Mode) -> &'static [H3Section] {
    if mode == H3Mode::Ref2VA {
        &REF_LAYOUT
    } else {
        &BASE_LAYOUT
    }
}

/// Contract label of one field in one mode.
pub fn section_key(section: H3Section, mode: H3Mode) -> &'static str {
    match section {
        H3Section::SubjectDefinitions => KEY_SUBJECT_DEFINITIONS,
        H3Section::Summary => KEY_SUMMARY,
        H3Section::RetentionAnalysis => KEY_RETENTION_ANALYSIS,
        H3Section::Picture => mode.picture_field_key(),
        H3Section::OverallSoundscape => KEY_SOUNDSCAPE,
        H3Section::NonDiegeticMusic => KEY_MUSIC,
    }
}

/// Closed fact-kind set of the legacy preserve-polish ledger (owned by the H3 adapter, VS31).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FactKind {
    Subject,
    Action,
    Camera,
    Time,
    Dialogue,
    Sound,
    Music,
    Style,
    Ban,
    Relation,
    VisibleText,
    Structure,
}

/// What a fact contributes to the H3 layout; derived only from typed kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum H3FactClass {
    Appearance,
    Continuity,
    Style,
    Sound,
    Music,
    Other,
}

impl H3FactClass {
    /// Mother fact `kind` (semantic contract closed set).
    pub fn of_semantic_kind(kind: &str) -> Self {
        match kind {
            "appearance" => H3FactClass::Appearance,
            "continuity" => H3FactClass::Continuity,
            "style" => H3FactClass::Style,
            "sound" => H3FactClass::Sound,
            "music" => H3FactClass::Music,
            _ => H3FactClass::Other,
        }
    }
}

/// The field one grouped sentence belongs to. Only whole groups of one class leave the picture
/// field; mixed groups, quotes and untyped groups stay in the timeline description.
pub fn route_section(
    mode: H3Mode,
    classes: &[H3FactClass],
    cites_reference: bool,
    carries_quote: bool,
) -> H3Section {
    let all = |class: H3FactClass| !classes.is_empty() && classes.iter().all(|c| *c == class);
    let reference_mode = mode == H3Mode::Ref2VA;
    if carries_quote {
        H3Section::Picture
    } else if all(H3FactClass::Sound) {
        H3Section::OverallSoundscape
    } else if all(H3FactClass::Music) {
        H3Section::NonDiegeticMusic
    } else if reference_mode && cites_reference && all(H3FactClass::Appearance) {
        H3Section::SubjectDefinitions
    } else if reference_mode && cites_reference && all(H3FactClass::Continuity) {
        H3Section::RetentionAnalysis
    } else if reference_mode && all(H3FactClass::Style) {
        H3Section::Summary
    } else {
        H3Section::Picture
    }
}

/// Summary task-type prefix from the frozen reference uses (`media_kind`, audio usage):
/// image / video guidance = reference generation, soundtrack copy = audio reuse, voice timbre =
/// audio reference. `None` when no use names a task type.
pub fn summary_task_types<'a>(
    uses: impl IntoIterator<Item = (&'a str, Option<&'a str>)>,
) -> Option<String> {
    let (mut generation, mut reuse, mut reference) = (false, false, false);
    for (media_kind, usage) in uses {
        match media_kind {
            MEDIA_IMAGE | MEDIA_VIDEO => generation = true,
            MEDIA_AUDIO => match usage.map(str::trim) {
                Some(H3_REFERENCE_AUDIO_USAGE_SOUNDTRACK_COPY) => reuse = true,
                Some(H3_REFERENCE_AUDIO_USAGE_VOICE_TIMBRE) => reference = true,
                _ => {}
            },
            _ => {}
        }
    }
    let types: Vec<&str> = [
        (generation, "reference generation"),
        (reuse, "audio reuse"),
        (reference, "audio reference"),
    ]
    .into_iter()
    .filter(|(on, _)| *on)
    .map(|(_, name)| name)
    .collect();
    (!types.is_empty()).then(|| format!("[{}]", types.join(" + ")))
}

/// Resources one input mode reads (CF-25 subset of the defaults).
fn mode_resource_refs(input_mode: &str) -> &'static [&'static str] {
    if H3Mode::from_input_mode(input_mode) == Some(H3Mode::Ref2VA) {
        &[H3_RESOURCE_BASE, H3_RESOURCE_REF]
    } else {
        &[H3_RESOURCE_BASE]
    }
}

/// H3 marker template of one taggable media kind (cite-token contract). Strict frames are
/// numbered by the mode plan, not by their ordinal, so they have no template here.
fn tag_template(media_kind: &str) -> Option<&'static str> {
    match media_kind {
        MEDIA_IMAGE => Some(CITE_TPL_H3_IMAGE),
        MEDIA_VIDEO => Some(CITE_TPL_H3_VIDEO),
        MEDIA_AUDIO => Some(CITE_TPL_H3_AUDIO),
        _ => None,
    }
}

fn explicit_audio_usage(item: &ReferenceMediaInventoryItem) -> Option<&'static str> {
    let raw = item
        .audio_usage
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())?;
    parse_explicit_h3_reference_audio_usage(Some(raw)).ok()
}

fn port_error(error: H3ModeError) -> ProjectionPortError {
    match error {
        H3ModeError::InputModeUnsupported { input_mode } => {
            ProjectionPortError::InputModeUnsupported { input_mode }
        }
        H3ModeError::DurationRequired { input_mode } => {
            ProjectionPortError::TargetDurationRequired { input_mode }
        }
        H3ModeError::PictureRoleMissing { input_mode, role } => {
            ProjectionPortError::ReferenceRoleMissing {
                input_mode,
                role: role.as_str().to_string(),
            }
        }
        H3ModeError::ReferenceUnsupported { reference_use_id } => {
            ProjectionPortError::ReferenceUseUnsupported { reference_use_id }
        }
    }
}

fn unknown(kind: ProjectionIdKind, id: &str) -> ProjectionPortError {
    ProjectionPortError::UnknownId {
        kind,
        id: id.to_string(),
    }
}

/// Marker of every frozen use: pictures from the mode plan; reference video / audio (Ref2VA only)
/// from their own ordinal. Audio needs an explicit usage, video carries none.
fn resolve_tokens(
    plan: &H3ModePlan,
    items: &[&ReferenceMediaInventoryItem],
) -> Result<BTreeMap<String, String>, ProjectionPortError> {
    let mut tokens: BTreeMap<String, String> = plan
        .pictures
        .iter()
        .map(|p| {
            (
                p.reference_use_id.clone(),
                apply_cite_template(CITE_TPL_H3_IMAGE, p.number),
            )
        })
        .collect();
    let mut ordinals: BTreeSet<(&str, u32)> = BTreeSet::new();
    for item in items {
        if tokens.contains_key(&item.item_id) {
            continue;
        }
        let unsupported = || ProjectionPortError::ReferenceUseUnsupported {
            reference_use_id: item.item_id.clone(),
        };
        let kind = item.media_kind.as_str();
        let usage_ok = match kind {
            MEDIA_AUDIO => explicit_audio_usage(item).is_some(),
            MEDIA_VIDEO => item.audio_usage.is_none(),
            _ => false,
        };
        if plan.mode != H3Mode::Ref2VA || !usage_ok {
            return Err(unsupported());
        }
        let template = tag_template(kind).ok_or_else(unsupported)?;
        let ordinal = item.ordinal_within_kind;
        if ordinal == 0 || !ordinals.insert((kind, ordinal)) {
            return Err(unsupported());
        }
        tokens.insert(item.item_id.clone(), apply_cite_template(template, ordinal));
    }
    Ok(tokens)
}

/// Frozen audio rows of the target (Ref2VA only; tokens already resolved). At most one soundtrack
/// copy: a second one is an unsupported use.
pub(super) fn audio_rows(
    items: &[&ReferenceMediaInventoryItem],
    tokens: &BTreeMap<String, String>,
) -> Result<Vec<H3AudioRow>, ProjectionPortError> {
    let mut rows: Vec<H3AudioRow> = Vec::new();
    for item in items.iter().filter(|i| i.media_kind == MEDIA_AUDIO) {
        let unsupported = || ProjectionPortError::ReferenceUseUnsupported {
            reference_use_id: item.item_id.clone(),
        };
        let usage_wire = explicit_audio_usage(item).ok_or_else(unsupported)?;
        let token = tokens.get(&item.item_id).ok_or_else(unsupported)?;
        let usage = if usage_wire == H3_REFERENCE_AUDIO_USAGE_SOUNDTRACK_COPY {
            H3AudioUse::SoundtrackCopy
        } else {
            H3AudioUse::VoiceTimbre
        };
        if usage.is_soundtrack_copy() && rows.iter().any(|r| r.usage.is_soundtrack_copy()) {
            return Err(unsupported());
        }
        rows.push(H3AudioRow {
            reference_use_id: item.item_id.clone(),
            ordinal: item.ordinal_within_kind,
            token: token.clone(),
            usage,
            usage_wire,
        });
    }
    Ok(rows)
}

/// The single visible performer when every audio reference is the copied soundtrack. With
/// identity locks it is the one locked subject (its lock token is the picture); without locks it
/// is the one frozen picture. Anything else is ambiguous (CF-43). Returns the performer and the
/// reference use id of its picture.
fn soundtrack_performer(
    rows: &[H3AudioRow],
    plan: &H3ModePlan,
    locks: &[IdentityLockLine],
    tokens: &BTreeMap<String, String>,
    speaker_id: &str,
) -> Result<Option<(H3SoundtrackPerformer, String)>, ProjectionPortError> {
    let Some(copy) = rows.iter().find(|r| r.usage.is_soundtrack_copy()) else {
        return Ok(None);
    };
    if rows.iter().any(|r| !r.usage.is_soundtrack_copy()) {
        return Ok(None);
    }
    let picture_token = if locks.is_empty() {
        if plan.pictures.len() != 1 {
            return Err(ProjectionPortError::VocalPerformerAmbiguous {
                count: plan.pictures.len(),
            });
        }
        tokens
            .get(&plan.pictures[0].reference_use_id)
            .cloned()
            .ok_or_else(|| {
                unknown(
                    ProjectionIdKind::ReferenceUse,
                    &plan.pictures[0].reference_use_id,
                )
            })?
    } else {
        let subjects: BTreeSet<&str> = locks.iter().map(|l| l.subject_id.as_str()).collect();
        if subjects.len() != 1 {
            return Err(ProjectionPortError::VocalPerformerAmbiguous {
                count: subjects.len(),
            });
        }
        locks[0].token.clone()
    };
    let picture_use = tokens
        .iter()
        .find_map(|(id, token)| (*token == picture_token).then(|| id.clone()))
        .ok_or_else(|| ProjectionPortError::ReferenceUseUnsupported {
            reference_use_id: locks
                .first()
                .map(|l| l.binding_id.clone())
                .unwrap_or_default(),
        })?;
    Ok(Some((
        H3SoundtrackPerformer {
            subject_token: H3_PERFORMER_SUBJECT.to_string(),
            speaker_id: speaker_label(speaker_id).to_string(),
            picture_token,
            audio_token: copy.token.clone(),
        },
        picture_use,
    )))
}

/// Text with its source map under construction.
#[derive(Default)]
struct Buffer {
    text: String,
    source_map: Vec<ProjectionSourceSpan>,
}

impl Buffer {
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

    fn syntax(&mut self, piece: &str) {
        self.span(piece, ProjectionSpanOrigin::TargetSyntax);
    }

    /// Separator before the next piece of a field body.
    fn separate(&mut self, separator: &str) {
        if !self.text.is_empty() {
            self.syntax(separator);
        }
    }

    fn append(&mut self, other: Buffer) {
        let offset = self.text.len();
        self.text.push_str(&other.text);
        self.source_map
            .extend(other.source_map.into_iter().map(|s| ProjectionSourceSpan {
                start: s.start + offset,
                end: s.end + offset,
                origin: s.origin,
            }));
    }
}

/// Closed id sets of one mother shot (fact id -> typed kind).
struct ShotIndex<'a> {
    position: usize,
    facts: BTreeMap<&'a str, &'a str>,
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
                    facts: shot
                        .facts
                        .iter()
                        .map(|f| (f.fact_id.as_str(), f.kind.as_str()))
                        .collect(),
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


/// Parse `speakerVoiceBindings` against frozen voice-timbre rows, mother speakers and identity
/// locks. Duplicate speaker / audio and non-timbre uses fail explicitly (T2-08).
type SpeakerVoiceBindings<'a> = (
    BTreeMap<String, String>,
    BTreeMap<String, (String, Option<&'a IdentityLockLine>)>,
);

fn resolve_speaker_voice_bindings<'a>(
    bindings: &'a [VideoPromptSpeakerVoiceBinding],
    audio: &[H3AudioRow],
    document: &DocumentDraftV1,
    locks: &'a [IdentityLockLine],
) -> Result<SpeakerVoiceBindings<'a>, ProjectionPortError> {
    let timbre_by_use: BTreeMap<&str, &H3AudioRow> = audio
        .iter()
        .filter(|r| matches!(r.usage, H3AudioUse::VoiceTimbre))
        .map(|r| (r.reference_use_id.as_str(), r))
        .collect();
    let mother_speakers: BTreeSet<&str> = document
        .shots
        .iter()
        .flat_map(|shot| shot.quotes.iter())
        .filter_map(|q| q.speaker_id.as_deref())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();

    let mut voice_by_speaker: BTreeMap<String, String> = BTreeMap::new();
    let mut binding_by_use: BTreeMap<String, (String, Option<&IdentityLockLine>)> = BTreeMap::new();
    let mut seen_speakers: BTreeSet<String> = BTreeSet::new();
    let mut seen_voices: BTreeSet<String> = BTreeSet::new();

    for binding in bindings {
        let voice_use = binding.voice_reference_use_id.trim();
        let speaker_raw = binding.speaker_id.trim();
        let Some(row) = timbre_by_use.get(voice_use) else {
            return Err(ProjectionPortError::ReferenceUseUnsupported {
                reference_use_id: binding.voice_reference_use_id.clone(),
            });
        };
        if speaker_raw.is_empty() || !mother_speakers.contains(speaker_raw) {
            return Err(unknown(ProjectionIdKind::Speaker, &binding.speaker_id));
        }
        let speaker = speaker_label(speaker_raw).to_string();
        if !seen_speakers.insert(speaker.clone()) || !seen_voices.insert(voice_use.to_string()) {
            return Err(ProjectionPortError::ReferenceUseUnsupported {
                reference_use_id: binding.voice_reference_use_id.clone(),
            });
        }
        let lock = match binding.subject_id.as_deref().map(str::trim).filter(|s| !s.is_empty())
        {
            Some(subject_id) => {
                let found = locks
                    .iter()
                    .find(|l| l.subject_id.trim() == subject_id && !l.token.trim().is_empty());
                match found {
                    Some(lock) => Some(lock),
                    None => return Err(unknown(ProjectionIdKind::Subject, subject_id)),
                }
            }
            None => None,
        };
        voice_by_speaker.insert(speaker.clone(), row.token.clone());
        binding_by_use.insert(row.reference_use_id.clone(), (speaker, lock));
    }
    Ok((voice_by_speaker, binding_by_use))
}

/// Vocal quotes use the official vocal sentence with the frozen speaker id; other quotes are
/// written in double quotes. The verbatim text is always its own Quote span.
fn push_quote(
    buf: &mut Buffer,
    shot_id: &str,
    quote: &QuoteIntentV1,
    performer: Option<&H3SoundtrackPerformer>,
    voice_by_speaker: &BTreeMap<String, String>,
) {
    let speaker = speaker_label(quote.speaker_id.as_deref().unwrap_or(""));
    let voice = voice_by_speaker.get(speaker).map(String::as_str);
    let (open, close) = match H3VocalRole::of_quote_role(&quote.role) {
        Some(role) => vocal_frame(
            role,
            &quote.exact_text,
            quote.speaker_id.as_deref().unwrap_or(""),
            performer,
            voice,
        ),
        None => ("\"".to_string(), "\"".to_string()),
    };
    buf.syntax(&open);
    buf.span(
        &quote.exact_text,
        ProjectionSpanOrigin::Quote {
            shot_id: shot_id.to_string(),
            quote_id: quote.quote_id.clone(),
        },
    );
    buf.syntax(&close);
}

/// One expression line as it is written in an H3 field.
struct LineView<'a> {
    index: usize,
    shot_id: &'a str,
    fact_ids: &'a [String],
    reference_use_ids: &'a [String],
    text: &'a str,
}

/// Write `<Picture 1>, <Audio 1>: text`; `omit` is a use already named by the field head.
fn push_line<'a>(
    buf: &mut Buffer,
    line: &LineView<'a>,
    tokens: &BTreeMap<String, String>,
    used: &mut BTreeSet<&'a str>,
    omit: Option<&str>,
) {
    let mut tagged: BTreeSet<&str> = BTreeSet::new();
    for use_id in line.reference_use_ids {
        used.insert(use_id.as_str());
        if Some(use_id.as_str()) == omit {
            continue;
        }
        let Some(token) = tokens.get(use_id) else {
            continue;
        };
        if !tagged.insert(use_id.as_str()) {
            continue;
        }
        if tagged.len() > 1 {
            buf.syntax(", ");
        }
        buf.span(
            token,
            ProjectionSpanOrigin::ReferenceUse {
                reference_use_id: use_id.clone(),
            },
        );
    }
    if !tagged.is_empty() {
        buf.syntax(": ");
    }
    buf.span(
        line.text,
        ProjectionSpanOrigin::ExpressionLine {
            line_index: line.index,
            shot_id: line.shot_id.to_string(),
            fact_ids: line.fact_ids.to_vec(),
        },
    );
}

/// Deterministic H3 projector registered as `h3@1` (native cloud and declared workflow targets
/// declare the same profile).
pub struct H3Projector;

impl VideoPromptProjectionPort for H3Projector {
    fn identity(&self) -> ProjectorIdentity {
        ProjectorIdentity {
            profile_id: H3_PROFILE_ID.to_string(),
            profile_version: u64::from(H3_PROFILE_VERSION),
            compiler_revision: H3_COMPILER_REVISION,
            input_modes: H3_INPUT_MODES.iter().map(|m| m.to_string()).collect(),
            default_resource_refs: vec![H3_RESOURCE_BASE.to_string(), H3_RESOURCE_REF.to_string()],
            mode_resource_refs: H3_INPUT_MODES
                .iter()
                .map(|mode| ModeResourceRefs {
                    input_mode: mode.to_string(),
                    resource_refs: mode_resource_refs(mode)
                        .iter()
                        .map(|r| r.to_string())
                        .collect(),
                })
                .collect(),
        }
    }

    fn reference_token(&self, media_kind: &str, ordinal_within_kind: u32) -> Option<String> {
        if ordinal_within_kind == 0 {
            return None;
        }
        tag_template(media_kind).map(|template| apply_cite_template(template, ordinal_within_kind))
    }

    fn line_organization(&self) -> LineOrganization {
        LineOrganization::Sectioned
    }

    fn speaker_voice_reference_use_ids(
        &self,
        target: &VideoPromptTargetSnapshot,
        references: &[ReferenceMediaInventoryItem],
    ) -> Vec<String> {
        if H3Mode::from_input_mode(&target.input_mode) != Some(H3Mode::Ref2VA) {
            return Vec::new();
        }
        let mut out = Vec::new();
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        for use_id in &target.reference_use_ids {
            if !seen.insert(use_id.as_str()) {
                continue;
            }
            let Some(item) = references.iter().find(|r| r.item_id == *use_id) else {
                continue;
            };
            if item.media_kind != MEDIA_AUDIO {
                continue;
            }
            if explicit_audio_usage(item) != Some(H3_REFERENCE_AUDIO_USAGE_VOICE_TIMBRE) {
                continue;
            }
            out.push(use_id.clone());
        }
        out
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

        let mut items: Vec<&ReferenceMediaInventoryItem> = Vec::new();
        let mut seen: BTreeSet<&str> = BTreeSet::new();
        for use_id in &target.reference_use_ids {
            if !seen.insert(use_id.as_str()) {
                continue;
            }
            let item = input
                .references
                .iter()
                .find(|r| &r.item_id == use_id)
                .ok_or_else(|| unknown(ProjectionIdKind::ReferenceUse, use_id))?;
            items.push(item);
        }
        let plan =
            plan_mode(&target.input_mode, target.duration_seconds, &items).map_err(port_error)?;
        let mode = plan.mode;
        let reference_mode = mode == H3Mode::Ref2VA;
        let tokens = resolve_tokens(&plan, &items)?;
        let alignment = plan.alignment_header().map_err(port_error)?;

        let expression = input
            .expression
            .ok_or(ProjectionPortError::ExpressionRequired)?;
        let document = input.mother.document;
        let shots = index_mother(document);

        let mut lines_by_shot: Vec<Vec<usize>> = vec![Vec::new(); document.shots.len()];
        let mut sections: Vec<H3Section> = Vec::with_capacity(expression.lines.len());
        for (index, line) in expression.lines.iter().enumerate() {
            let shot = shots
                .get(line.shot_id.as_str())
                .ok_or_else(|| unknown(ProjectionIdKind::Shot, &line.shot_id))?;
            let mut classes = Vec::with_capacity(line.fact_ids.len());
            for id in &line.fact_ids {
                let kind = shot
                    .facts
                    .get(id.as_str())
                    .ok_or_else(|| unknown(ProjectionIdKind::Fact, id))?;
                classes.push(H3FactClass::of_semantic_kind(kind));
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
                .find(|id| !tokens.contains_key(id.as_str()))
            {
                return Err(unknown(ProjectionIdKind::ReferenceUse, id));
            }
            sections.push(route_section(
                mode,
                &classes,
                !line.reference_use_ids.is_empty(),
                !line.quote_ids.is_empty(),
            ));
            // Mother shot order is authoritative; lines keep their relative order inside a shot.
            lines_by_shot[shot.position].push(index);
        }
        let view = move |index: usize| {
            let line = &expression.lines[index];
            LineView {
                index,
                shot_id: line.shot_id.as_str(),
                fact_ids: &line.fact_ids,
                reference_use_ids: &line.reference_use_ids,
                text: line.text.as_str(),
            }
        };

        let mut used: BTreeSet<&str> = BTreeSet::new();
        if alignment.is_some() {
            // The official alignment head cites every anchored strict frame picture.
            for anchor in &plan.anchors {
                if let Some(picture) = plan
                    .pictures
                    .iter()
                    .find(|p| p.number == anchor.picture && p.role != H3PictureRole::Reference)
                {
                    used.insert(picture.reference_use_id.as_str());
                }
            }
        }
        let mut locks_by_shot: Vec<Vec<(&IdentityLockLine, Option<&str>)>> =
            vec![Vec::new(); document.shots.len()];
        for lock in input.identity_locks {
            let shot = shots
                .get(lock.shot_id.as_str())
                .ok_or_else(|| unknown(ProjectionIdKind::Shot, &lock.shot_id))?;
            // An empty token is a neutral lock line (no H3 tag for the reference, e.g. a strict
            // frame, CF-21): written as text, bound to no use.
            let use_id = if lock.token.is_empty() {
                None
            } else {
                let use_id = tokens
                    .iter()
                    .find_map(|(id, token)| (*token == lock.token).then_some(id.as_str()))
                    .ok_or_else(|| ProjectionPortError::ReferenceUseUnsupported {
                        reference_use_id: lock.binding_id.clone(),
                    })?;
                used.insert(use_id);
                Some(use_id)
            };
            locks_by_shot[shot.position].push((lock, use_id));
        }

        // Reference audio (Ref2VA only; `resolve_tokens` rejects it elsewhere).
        let audio = audio_rows(&items, &tokens)?;
        let (voice_by_speaker, voice_binding_by_use) = resolve_speaker_voice_bindings(
            &expression.speaker_voice_bindings,
            &audio,
            document,
            input.identity_locks,
        )?;
        let vocal_quotes: Vec<&QuoteIntentV1> = document
            .shots
            .iter()
            .flat_map(|shot| shot.quotes.iter())
            .filter(|q| H3VocalRole::of_quote_role(&q.role).is_some())
            .collect();
        let has_singing = vocal_quotes
            .iter()
            .any(|q| H3VocalRole::of_quote_role(&q.role) == Some(H3VocalRole::Singing));
        let performer_speaker = vocal_quotes
            .iter()
            .find(|q| H3VocalRole::of_quote_role(&q.role) == Some(H3VocalRole::Singing))
            .or(vocal_quotes.first())
            .and_then(|q| q.speaker_id.as_deref())
            .unwrap_or("");
        let performer = match soundtrack_performer(
            &audio,
            &plan,
            input.identity_locks,
            &tokens,
            performer_speaker,
        )? {
            Some((performer, picture_use)) => {
                if let Some(id) = tokens.keys().find(|id| **id == picture_use) {
                    used.insert(id.as_str());
                }
                Some(performer)
            }
            None => None,
        };
        for row in &audio {
            if let Some(id) = tokens.keys().find(|id| **id == row.reference_use_id) {
                used.insert(id.as_str());
            }
        }

        // Shot numbers count only shots that write into the picture field (the first always does).
        let mut shot_numbers: Vec<Option<usize>> = vec![None; document.shots.len()];
        let mut next = 0;
        for (position, shot) in document.shots.iter().enumerate() {
            let writes = position == 0
                || !shot.quotes.is_empty()
                || (!reference_mode && !locks_by_shot[position].is_empty())
                || lines_by_shot[position]
                    .iter()
                    .any(|&i| sections[i] == H3Section::Picture);
            if writes {
                next += 1;
                shot_numbers[position] = Some(next);
            }
        }

        let mut covered: BTreeSet<(&str, &str)> = BTreeSet::new();
        let mut appearances: BTreeMap<&str, BTreeSet<usize>> = BTreeMap::new();
        let mut picture = Buffer::default();
        for (position, shot) in document.shots.iter().enumerate() {
            let Some(number) = shot_numbers[position] else {
                continue;
            };
            let shot_id = shot.shot_id.as_str();
            let index = &shots[shot_id];
            picture.separate(" ");
            picture.syntax(&format!("[Shot {number}]"));
            for (lock, use_id) in &locks_by_shot[position] {
                if let Some(use_id) = use_id {
                    appearances.entry(*use_id).or_default().insert(number);
                }
                if !reference_mode {
                    picture.syntax(" ");
                    picture.span(
                        &lock.text,
                        ProjectionSpanOrigin::IdentityLock {
                            shot_id: shot_id.to_string(),
                            binding_id: lock.binding_id.clone(),
                        },
                    );
                }
            }
            let mut quotes_done: BTreeSet<&str> = BTreeSet::new();
            for &line_index in &lines_by_shot[position] {
                if sections[line_index] != H3Section::Picture {
                    continue;
                }
                let line = &expression.lines[line_index];
                for use_id in &line.reference_use_ids {
                    appearances
                        .entry(use_id.as_str())
                        .or_default()
                        .insert(number);
                }
                picture.syntax(" ");
                push_line(&mut picture, &view(line_index), &tokens, &mut used, None);
                for quote_id in &line.quote_ids {
                    if !quotes_done.insert(quote_id.as_str()) {
                        continue;
                    }
                    if let Some(quote) = index.quotes.get(quote_id.as_str()) {
                        picture.syntax(" ");
                        push_quote(&mut picture, shot_id, quote, performer.as_ref(), &voice_by_speaker);
                    }
                }
            }
            // Quotes are inserted from the mother even when no line listed them; never dropped.
            for quote in &shot.quotes {
                if quotes_done.insert(quote.quote_id.as_str()) {
                    picture.syntax(" ");
                    push_quote(&mut picture, shot_id, quote, performer.as_ref(), &voice_by_speaker);
                }
            }
        }
        // A copied soundtrack is used unchanged; without vocal text the performer lip-syncs to it.
        if let Some(performer) = &performer {
            picture.separate(" ");
            if vocal_quotes.is_empty() {
                picture.syntax(&lip_sync_sentence(performer));
            } else {
                picture.syntax(&use_exactly_sentence(&performer.audio_token));
            }
        }

        let mut grouped: BTreeMap<H3Section, Vec<usize>> = BTreeMap::new();
        for lines in &lines_by_shot {
            for &line_index in lines {
                let line = &expression.lines[line_index];
                for fact_id in &line.fact_ids {
                    covered.insert((line.shot_id.as_str(), fact_id.as_str()));
                }
                if sections[line_index] != H3Section::Picture {
                    grouped
                        .entry(sections[line_index])
                        .or_default()
                        .push(line_index);
                }
            }
        }
        let group = |section: H3Section| grouped.get(&section).cloned().unwrap_or_default();

        let mut out = Buffer::default();
        if let Some(header) = &alignment {
            out.syntax(header.trim());
        }
        for &section in layout(mode) {
            let mut body = Buffer::default();
            match section {
                H3Section::Picture => body.append(std::mem::take(&mut picture)),
                H3Section::OverallSoundscape | H3Section::NonDiegeticMusic => {
                    let official = if section == H3Section::OverallSoundscape {
                        soundscape_sentence(&audio)
                    } else {
                        music_sentence(&audio)
                    };
                    if let Some(sentence) = official {
                        body.syntax(&sentence);
                    }
                    for line_index in group(section) {
                        body.separate(" ");
                        push_line(&mut body, &view(line_index), &tokens, &mut used, None);
                    }
                    if body.text.is_empty() {
                        body.syntax(if section == H3Section::OverallSoundscape {
                            H3_SOUNDSCAPE_UNSPECIFIED
                        } else {
                            H3_SECTION_NOT_APPLICABLE
                        });
                    }
                }
                H3Section::SubjectDefinitions => {
                    for (position, shot) in document.shots.iter().enumerate() {
                        for (lock, _) in &locks_by_shot[position] {
                            body.separate("\n");
                            body.span(
                                &lock.text,
                                ProjectionSpanOrigin::IdentityLock {
                                    shot_id: shot.shot_id.clone(),
                                    binding_id: lock.binding_id.clone(),
                                },
                            );
                        }
                    }
                    for line_index in group(section) {
                        body.separate("\n");
                        push_line(&mut body, &view(line_index), &tokens, &mut used, None);
                    }
                    if let Some(performer) = performer.as_ref() {
                        body.separate("\n");
                        body.syntax(&performer_definition_line(performer));
                    }
                    for row in audio.iter().filter(|r| !r.usage.is_soundtrack_copy()) {
                        body.separate("\n");
                        if let Some((speaker, lock)) = voice_binding_by_use.get(&row.reference_use_id)
                        {
                            let (prefix, suffix) = voice_timbre_bound_parts(&row.token, speaker);
                            body.syntax(&prefix);
                            if let Some(lock) = lock {
                                body.span(
                                    &lock.text,
                                    ProjectionSpanOrigin::IdentityLock {
                                        shot_id: lock.shot_id.clone(),
                                        binding_id: lock.binding_id.clone(),
                                    },
                                );
                            } else {
                                body.syntax(H3_UNNAMED_SPEAKER);
                            }
                            body.syntax(&suffix);
                        } else {
                            body.syntax(&voice_timbre_unbound_line(&row.token));
                        }
                    }
                    if body.text.is_empty() {
                        body.syntax(H3_SECTION_NOT_APPLICABLE);
                    }
                }
                H3Section::Summary => {
                    let head = summary_sentence(performer.as_ref(), has_singing).or_else(|| {
                        summary_task_types(
                            items
                                .iter()
                                .map(|i| (i.media_kind.as_str(), i.audio_usage.as_deref())),
                        )
                    });
                    if let Some(head) = head {
                        body.syntax(&head);
                    }
                    for line_index in group(section) {
                        body.separate(" ");
                        push_line(&mut body, &view(line_index), &tokens, &mut used, None);
                    }
                    if body.text.is_empty() {
                        body.syntax(H3_SECTION_NOT_APPLICABLE);
                    }
                }
                H3Section::RetentionAnalysis => {
                    let mut by_use: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
                    for line_index in group(section) {
                        if let Some(first) = expression.lines[line_index].reference_use_ids.first()
                        {
                            by_use.entry(first.as_str()).or_default().push(line_index);
                        }
                    }
                    let mut written: BTreeSet<&str> = BTreeSet::new();
                    for use_id in &target.reference_use_ids {
                        let use_id = use_id.as_str();
                        let Some(lines) = by_use.get(use_id) else {
                            continue;
                        };
                        if !written.insert(use_id) {
                            continue;
                        }
                        body.separate("\n");
                        if let Some(token) = tokens.get(use_id) {
                            body.span(
                                token,
                                ProjectionSpanOrigin::ReferenceUse {
                                    reference_use_id: use_id.to_string(),
                                },
                            );
                        }
                        if let Some(numbers) = appearances.get(use_id) {
                            let shots: Vec<String> =
                                numbers.iter().map(|n| format!("[Shot {n}]")).collect();
                            body.syntax(&format!(" (appears in {})", shots.join(", ")));
                        }
                        body.syntax(": ");
                        for (n, &line_index) in lines.iter().enumerate() {
                            if n > 0 {
                                body.syntax(" ");
                            }
                            push_line(
                                &mut body,
                                &view(line_index),
                                &tokens,
                                &mut used,
                                Some(use_id),
                            );
                        }
                    }
                    for audio_line in retention_lines(&audio) {
                        body.separate("\n");
                        body.syntax(&audio_line);
                    }
                    if body.text.is_empty() {
                        body.syntax(H3_SECTION_NOT_APPLICABLE);
                    }
                }
            }
            out.separate(H3_SECTION_SEPARATOR);
            out.syntax(&section_head(section_key(section, mode)));
            out.append(body);
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
            if !used.contains(use_id.as_str()) && reported.insert(use_id.as_str()) {
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;

    use super::*;
    use crate::projectors::port::ReviewedMother;
    use crate::projectors::{compiler_registry, VideoPromptProjectorRegistry};
    use bgx_video_prompt_contract::projection::{
        VideoPromptProjectionDraft, VideoPromptTargetSnapshot,
    };
    use bgx_video_prompt_contract::document::{PromptFactV1, ShotDraftV1};

    fn fact(id: &str, kind: &str, text: &str) -> PromptFactV1 {
        PromptFactV1 {
            fact_id: id.into(),
            kind: kind.into(),
            text: text.into(),
            semantic_slot: "action_interaction".into(),
            polarity: Some("positive".into()),
            provenance: "user_text".into(),
            source_fact_id: None,
            entity_ref_ids: Vec::new(),
            source_unit_ids: vec![format!("unit-{id}")],
        }
    }

    fn quote(id: &str, text: &str, role: &str) -> QuoteIntentV1 {
        QuoteIntentV1 {
            quote_id: id.into(),
            exact_text: text.into(),
            role: role.into(),
            speaker_id: None,
            source_unit_id: Some(format!("unit-{id}")),
        }
    }

    /// Mother whose fact *texts* deliberately mention sound / music words on non-audio kinds:
    /// routing must follow the typed kind only.
    fn mother() -> DocumentDraftV1 {
        DocumentDraftV1 {
            shots: vec![
                ShotDraftV1 {
                    shot_id: "s1".into(),
                    facts: vec![
                        fact("f-look", "appearance", "mother appearance"),
                        fact("f-act", "action", "she hums along to the music box"),
                        fact("f-snd", "sound", "mother sound"),
                        fact("f-mus", "music", "mother music"),
                        fact("f-style", "style", "mother style"),
                        fact("f-keep", "continuity", "mother continuity"),
                    ],
                    quotes: vec![
                        quote("q-say", "站住。", "dialogue"),
                        quote("q-sign", "OPEN", "visible_text"),
                    ],
                },
                ShotDraftV1 {
                    shot_id: "s2".into(),
                    facts: vec![fact("f-two", "action", "mother action two")],
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
            reference("use-img2", MEDIA_IMAGE, 2, None),
            reference("use-vid", MEDIA_VIDEO, 1, None),
            reference("use-voice", MEDIA_AUDIO, 1, Some("voice_timbre")),
            reference("use-voice2", MEDIA_AUDIO, 5, Some("voice_timbre")),
            reference("use-song", MEDIA_AUDIO, 3, Some("soundtrack_copy")),
            reference("use-song2", MEDIA_AUDIO, 4, Some("soundtrack_copy")),
            reference("use-audio-bare", MEDIA_AUDIO, 2, None),
            reference("use-first", "first_frame", 1, None),
            reference("use-last", "last_frame", 1, None),
        ]
    }

    fn target(
        profile: &str,
        version: u64,
        mode: &str,
        uses: &[&str],
        duration: Option<f64>,
    ) -> VideoPromptTargetSnapshot {
        serde_json::from_value(json!({
            "profileId": profile,
            "profileVersion": version,
            "inputMode": mode,
            "durationSeconds": duration,
            "capsDigest": "a".repeat(64),
            "expressionLanguage": "en",
            "referenceUseIds": uses,
        }))
        .unwrap()
    }

    fn h3_target(mode: &str, uses: &[&str]) -> VideoPromptTargetSnapshot {
        target(H3_PROFILE_ID, 1, mode, uses, Some(6.0))
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

    fn compile_with(
        target: &VideoPromptTargetSnapshot,
        locks: &[IdentityLockLine],
        expression: Option<&VideoPromptProjectionDraft>,
    ) -> Result<ProjectionCandidate, ProjectionPortError> {
        let document = mother();
        let refs = references();
        let input = ProjectionCompileInput {
            mother: ReviewedMother {
                document: &document,
                canonical_fingerprint: "b",
            },
            references: &refs,
            identity_locks: locks,
            target,
            method: None,
            expression,
        };
        H3Projector.compile(&input)
    }

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

    /// Field labels in output order (each field starts a block).
    fn field_keys(text: &str) -> Vec<String> {
        text.split(H3_SECTION_SEPARATOR)
            .filter_map(|block| block.split_once(": ").map(|(key, _)| key.to_string()))
            .filter(|key| H3_LAYOUT_LABELS.contains(&key.as_str()))
            .collect()
    }

    fn field<'a>(text: &'a str, key: &str) -> &'a str {
        let head = section_head(key);
        text.split(H3_SECTION_SEPARATOR)
            .find_map(|block| block.strip_prefix(head.as_str()))
            .unwrap_or_else(|| panic!("missing {key}: {text}"))
    }

    fn base_lines() -> VideoPromptProjectionDraft {
        draft(json!([
            line(
                "s1",
                &["f-look", "f-act"],
                &["q-say"],
                &[],
                "expression look act"
            ),
            line("s1", &["f-snd"], &[], &[], "expression sound"),
            line("s1", &["f-mus"], &[], &[], "expression music"),
            line(
                "s1",
                &["f-style", "f-keep"],
                &[],
                &[],
                "expression style keep"
            ),
            line("s2", &["f-two"], &[], &[], "expression two"),
        ]))
    }

    #[test]
    fn h3_identity_declares_six_modes_and_per_mode_resources() {
        let identity = H3Projector.identity();
        assert_eq!(identity.profile_id, H3_PROFILE_ID);
        assert_eq!(
            identity.input_modes,
            H3_INPUT_MODES.map(str::to_string).to_vec()
        );
        assert_eq!(
            identity.default_resource_refs,
            vec![H3_RESOURCE_BASE.to_string(), H3_RESOURCE_REF.to_string()]
        );
        for refs in &identity.mode_resource_refs {
            let expected: Vec<String> = match refs.input_mode.as_str() {
                "multi_image_to_video" | "reference_to_video" => {
                    vec![H3_RESOURCE_BASE.into(), H3_RESOURCE_REF.into()]
                }
                _ => vec![H3_RESOURCE_BASE.into()],
            };
            assert_eq!(refs.resource_refs, expected, "{}", refs.input_mode);
        }
        assert_eq!(identity.mode_resource_refs.len(), H3_INPUT_MODES.len());
        let registry = compiler_registry().unwrap();
        assert!(registry.projector(H3_PROFILE_ID, 1).is_some());
        assert!(registry.projector(H3_PROFILE_ID, 2).is_none());
        assert!(VideoPromptProjectorRegistry::new(vec![
            Arc::new(H3Projector) as Arc<dyn VideoPromptProjectionPort>
        ])
        .is_ok());
        assert_eq!(
            H3Projector.reference_token(MEDIA_IMAGE, 2),
            Some(apply_cite_template(CITE_TPL_H3_IMAGE, 2))
        );
        assert_eq!(H3Projector.reference_token("first_frame", 1), None);
        assert_eq!(H3Projector.reference_token(MEDIA_AUDIO, 0), None);
    }

    #[test]
    fn h3_layout_labels_cover_both_layouts_exactly() {
        let mut all: Vec<&str> = H3_BASE_SECTION_KEYS
            .iter()
            .chain(H3_REF_SECTION_KEYS)
            .copied()
            .collect();
        all.sort_unstable();
        all.dedup();
        let mut labels = H3_LAYOUT_LABELS.to_vec();
        labels.sort_unstable();
        assert_eq!(all, labels);
        for mode in [H3Mode::T2VA, H3Mode::I2VA, H3Mode::Ref2VA] {
            let keys: Vec<&str> = layout(mode).iter().map(|s| section_key(*s, mode)).collect();
            let declared = if mode == H3Mode::Ref2VA {
                H3_REF_SECTION_KEYS
            } else {
                H3_BASE_SECTION_KEYS
            };
            assert_eq!(keys, declared.to_vec());
            assert_eq!(parse_mode_wire(mode_wire(mode)), Some(mode));
        }
    }

    #[test]
    fn h3_text_mode_writes_three_fields_routed_by_typed_kind() {
        let candidate =
            compile_with(&h3_target("text_to_video", &[]), &[], Some(&base_lines())).unwrap();
        assert_fully_mapped(&candidate);
        assert!(
            candidate.diagnostics.is_empty(),
            "{:?}",
            candidate.diagnostics
        );
        let text = &candidate.text;
        assert_eq!(field_keys(text), H3_BASE_SECTION_KEYS.to_vec());
        let picture = field(text, "integrated_multimodal_description");
        // An action fact that mentions music stays in the timeline (no keyword routing).
        assert!(picture.starts_with("[Shot 1] expression look act (S1) <d>[Chinese] 站住。</d>"));
        assert!(picture.contains("\"OPEN\""));
        assert!(picture.contains("[Shot 2] expression two"));
        assert!(picture.contains("expression style keep"));
        assert_eq!(field(text, "overall_soundscape"), "expression sound");
        assert_eq!(field(text, "non_diegetic_music"), "expression music");
    }

    #[test]
    fn h3_empty_audio_fields_use_official_bodies() {
        let lines = draft(json!([line("s1", &["f-act"], &[], &[], "expression act")]));
        let candidate = compile_with(&h3_target("text_to_video", &[]), &[], Some(&lines)).unwrap();
        assert_fully_mapped(&candidate);
        let text = &candidate.text;
        assert_eq!(field(text, "overall_soundscape"), H3_SOUNDSCAPE_UNSPECIFIED);
        assert_eq!(field(text, "non_diegetic_music"), H3_SECTION_NOT_APPLICABLE);
        // Quotes are still inserted verbatim even though no line listed them.
        assert!(field(text, "integrated_multimodal_description").contains("站住。"));
        assert!(candidate
            .diagnostics
            .contains(&ProjectionDiagnostic::FactNotCovered {
                shot_id: "s1".into(),
                fact_id: "f-snd".into()
            }));
    }

    #[test]
    fn h3_reference_mode_fills_six_fields_from_mother_facts_and_uses() {
        let lines = draft(json!([
            line("s1", &["f-look"], &[], &["use-img"], "expression look"),
            line(
                "s1",
                &["f-act"],
                &["q-say"],
                &["use-img", "use-voice"],
                "expression act"
            ),
            line("s1", &["f-keep"], &[], &["use-img"], "expression keep"),
            line("s1", &["f-style"], &[], &[], "expression style"),
            line("s1", &["f-snd"], &[], &[], "expression sound"),
            line("s2", &["f-two"], &[], &["use-vid"], "expression two"),
        ]));
        let target = h3_target("reference_to_video", &["use-img", "use-voice", "use-vid"]);
        let candidate = compile_with(&target, &[], Some(&lines)).unwrap();
        assert_fully_mapped(&candidate);
        let text = &candidate.text;
        assert_eq!(field_keys(text), H3_REF_SECTION_KEYS.to_vec());
        let picture1 = apply_cite_template(CITE_TPL_H3_IMAGE, 1);
        let audio1 = apply_cite_template(CITE_TPL_H3_AUDIO, 1);
        assert_eq!(
            field(text, "subject_definitions"),
            format!(
                "{picture1}: expression look\n{audio1} is a voice timbre reference and is not the finished soundtrack."
            )
        );
        assert_eq!(
            field(text, "summary"),
            "[reference generation + audio reference] expression style"
        );
        assert_eq!(
            field(text, "retention_analysis"),
            format!(
                "{picture1} (appears in [Shot 1]): expression keep\n{audio1}: reference - voice timbre only; do not copy the supplied signal."
            )
        );
        let detail = field(text, "detailed_description");
        assert!(detail.starts_with(&format!(
            "[Shot 1] {picture1}, {}: expression act",
            apply_cite_template(CITE_TPL_H3_AUDIO, 1)
        )));
        assert!(detail.contains(&format!(
            "[Shot 2] {}: expression two",
            apply_cite_template(CITE_TPL_H3_VIDEO, 1)
        )));
        assert_eq!(field(text, "non_diegetic_music"), H3_SECTION_NOT_APPLICABLE);
        for retired in [
            "Use only the source facts",
            "preserve the selected reference binding",
            "No identity lock is declared",
        ] {
            assert!(!text.contains(retired), "{retired}");
        }
    }

    #[test]
    fn h3_soundtrack_copy_compiles_one_performer_and_exact_lyrics() {
        let document = DocumentDraftV1 {
            shots: vec![ShotDraftV1 {
                shot_id: "s1".into(),
                facts: vec![fact("f-act", "action", "mother action")],
                quotes: vec![QuoteIntentV1 {
                    quote_id: "v1".into(),
                    exact_text: "山河入梦。".into(),
                    role: "singing".into(),
                    speaker_id: Some("S1".into()),
                    source_unit_id: Some("unit-v1".into()),
                }],
            }],
        };
        let refs = references();
        let lines = draft(json!([line(
            "s1",
            &["f-act"],
            &["v1"],
            &["use-img"],
            "expression act"
        )]));
        let compile = |target: &VideoPromptTargetSnapshot, locks: &[IdentityLockLine]| {
            H3Projector.compile(&ProjectionCompileInput {
                mother: ReviewedMother {
                    document: &document,
                    canonical_fingerprint: "b",
                },
                references: &refs,
                identity_locks: locks,
                target,
                method: None,
                expression: Some(&lines),
            })
        };
        let single = h3_target("reference_to_video", &["use-img", "use-song"]);
        let candidate = compile(&single, &[]).unwrap();
        assert_fully_mapped(&candidate);
        let text = &candidate.text;
        let picture1 = apply_cite_template(CITE_TPL_H3_IMAGE, 1);
        let audio = apply_cite_template(CITE_TPL_H3_AUDIO, 3);
        let detail = field(text, "detailed_description");
        assert!(
            detail.contains(&format!(
                "{H3_PERFORMER_SUBJECT} (S1) sings in exact synchronization with {audio}, <d>[Chinese] 山河入梦。</d>."
            )),
            "{detail}"
        );
        assert!(detail.ends_with(&format!("Use {audio} exactly as it is.")));
        assert!(candidate.source_map.iter().any(|span| {
            matches!(&span.origin, ProjectionSpanOrigin::Quote { quote_id, .. } if quote_id == "v1")
                && &text[span.start..span.end] == "山河入梦。"
        }));
        assert!(field(text, "subject_definitions").contains(&format!(
            "{H3_PERFORMER_SUBJECT} is the visible performer defined by {picture1}."
        )));
        assert_eq!(
            field(text, "summary"),
            format!("[reference generation + audio reuse] {H3_PERFORMER_SUBJECT} performs the song while lip movements stay synchronized to {audio}.")
        );
        assert!(field(text, "retention_analysis").contains(&format!("{audio}: fully_copy")));
        assert_eq!(
            field(text, "overall_soundscape"),
            format!("Use {audio} exactly as it is. Do not add a second vocal performance.")
        );
        assert!(field(text, "non_diegetic_music").starts_with("The music already contained in"));
        assert!(!candidate
            .diagnostics
            .iter()
            .any(|d| matches!(d, ProjectionDiagnostic::ReferenceUseUnused { .. })));

        let lock = |subject: &str| IdentityLockLine {
            shot_id: "s1".into(),
            subject_id: subject.into(),
            binding_id: format!("binding-{subject}"),
            text: "lock".into(),
            token: picture1.clone(),
        };
        assert!(compile(&single, &[lock("a"), lock("a")]).is_ok());
        assert_eq!(
            compile(&single, &[lock("a"), lock("b")]),
            Err(ProjectionPortError::VocalPerformerAmbiguous { count: 2 })
        );
        let two_pictures = h3_target("reference_to_video", &["use-img", "use-img2", "use-song"]);
        assert_eq!(
            compile(&two_pictures, &[]),
            Err(ProjectionPortError::VocalPerformerAmbiguous { count: 2 })
        );
        let two_copies = h3_target("reference_to_video", &["use-img", "use-song", "use-song2"]);
        assert_eq!(
            compile(&two_copies, &[]),
            Err(ProjectionPortError::ReferenceUseUnsupported {
                reference_use_id: "use-song2".into()
            })
        );
    }

    #[test]
    fn h3_reference_mode_without_content_writes_not_applicable() {
        let lines = draft(json!([line("s1", &["f-act"], &[], &[], "expression act")]));
        let candidate = compile_with(
            &h3_target("reference_to_video", &["use-img"]),
            &[],
            Some(&lines),
        )
        .unwrap();
        let text = &candidate.text;
        assert_eq!(
            field(text, "subject_definitions"),
            H3_SECTION_NOT_APPLICABLE
        );
        assert_eq!(field(text, "summary"), "[reference generation]");
        assert_eq!(field(text, "retention_analysis"), H3_SECTION_NOT_APPLICABLE);
        assert!(candidate
            .diagnostics
            .contains(&ProjectionDiagnostic::ReferenceUseUnused {
                reference_use_id: "use-img".into()
            }));
    }

    #[test]
    fn h3_frame_modes_write_the_alignment_head_and_cite_frames() {
        let lines = draft(json!([line("s1", &["f-act"], &[], &[], "expression act")]));
        let candidate = compile_with(
            &h3_target("image_to_video", &["use-first"]),
            &[],
            Some(&lines),
        )
        .unwrap();
        assert_fully_mapped(&candidate);
        let header = super::super::mode::alignment_header(H3Mode::I2VA, Some(6.0))
            .unwrap()
            .unwrap();
        assert!(candidate.text.starts_with(header.trim()));
        assert!(!candidate
            .diagnostics
            .iter()
            .any(|d| matches!(d, ProjectionDiagnostic::ReferenceUseUnused { .. })));
        let lock = IdentityLockLine {
            shot_id: "s1".into(),
            subject_id: "subject-a".into(),
            binding_id: "binding-a".into(),
            text: "lock text".into(),
            token: apply_cite_template(CITE_TPL_H3_IMAGE, 1),
        };
        let locked = compile_with(
            &h3_target("image_to_video", &["use-first"]),
            std::slice::from_ref(&lock),
            Some(&lines),
        )
        .unwrap();
        assert!(field(&locked.text, "integrated_multimodal_description")
            .starts_with("[Shot 1] lock text expression act"));
    }

    #[test]
    fn h3_mode_errors_are_explicit_port_errors() {
        let lines = base_lines();
        assert_eq!(
            compile_with(&h3_target("video_edit", &[]), &[], Some(&lines)),
            Err(ProjectionPortError::InputModeUnsupported {
                input_mode: "video_edit".into()
            })
        );
        assert_eq!(
            compile_with(
                &target(
                    H3_PROFILE_ID,
                    1,
                    "first_last_frame",
                    &["use-first", "use-last"],
                    None
                ),
                &[],
                Some(&lines)
            ),
            Err(ProjectionPortError::TargetDurationRequired {
                input_mode: "first_last_frame".into()
            })
        );
        assert_eq!(
            compile_with(&h3_target("image_to_video", &[]), &[], Some(&lines)),
            Err(ProjectionPortError::ReferenceRoleMissing {
                input_mode: "image_to_video".into(),
                role: "first_frame".into()
            })
        );
        assert_eq!(
            compile_with(
                &h3_target("reference_to_video", &["use-audio-bare"]),
                &[],
                Some(&lines)
            ),
            Err(ProjectionPortError::ReferenceUseUnsupported {
                reference_use_id: "use-audio-bare".into()
            })
        );
        assert_eq!(
            compile_with(&h3_target("text_to_video", &[]), &[], None),
            Err(ProjectionPortError::ExpressionRequired)
        );
        assert!(matches!(
            compile_with(
                &target("other.video", 1, "text_to_video", &[], Some(6.0)),
                &[],
                Some(&lines)
            ),
            Err(ProjectionPortError::ProfileMismatch { .. })
        ));
        let stray = draft(json!([line("s2", &["f-act"], &[], &[], "x")]));
        assert_eq!(
            compile_with(&h3_target("text_to_video", &[]), &[], Some(&stray)),
            Err(ProjectionPortError::UnknownId {
                kind: ProjectionIdKind::Fact,
                id: "f-act".into()
            })
        );
    }

    #[test]
    fn h3_ref2va_binds_each_voice_timbre_to_its_speaker() {
        let picture1 = apply_cite_template(CITE_TPL_H3_IMAGE, 1);
        let audio1 = apply_cite_template(CITE_TPL_H3_AUDIO, 1);
        let audio5 = apply_cite_template(CITE_TPL_H3_AUDIO, 5);
        let document = DocumentDraftV1 {
            shots: vec![ShotDraftV1 {
                shot_id: "s1".into(),
                facts: vec![fact("f-act", "action", "mother action")],
                quotes: vec![
                    QuoteIntentV1 {
                        quote_id: "q1".into(),
                        exact_text: "你好".into(),
                        role: "dialogue".into(),
                        speaker_id: Some("S1".into()),
                        source_unit_id: Some("unit-q1".into()),
                    },
                    QuoteIntentV1 {
                        quote_id: "q2".into(),
                        exact_text: "再见".into(),
                        role: "dialogue".into(),
                        speaker_id: Some("S2".into()),
                        source_unit_id: Some("unit-q2".into()),
                    },
                    QuoteIntentV1 {
                        quote_id: "q3".into(),
                        exact_text: "再来".into(),
                        role: "dialogue".into(),
                        speaker_id: Some("S1".into()),
                        source_unit_id: Some("unit-q3".into()),
                    },
                ],
            }],
        };
        let refs = references();
        let mut lines = draft(json!([line(
            "s1",
            &["f-act"],
            &["q1", "q2", "q3"],
            &["use-img"],
            "expression act"
        )]));
        lines.speaker_voice_bindings = vec![
            bgx_video_prompt_contract::projection::VideoPromptSpeakerVoiceBinding {
                speaker_id: "S1".into(),
                voice_reference_use_id: "use-voice2".into(),
                subject_id: Some("a".into()),
            },
            bgx_video_prompt_contract::projection::VideoPromptSpeakerVoiceBinding {
                speaker_id: "S2".into(),
                voice_reference_use_id: "use-voice".into(),
                subject_id: None,
            },
        ];
        let lock = IdentityLockLine {
            shot_id: "s1".into(),
            subject_id: "a".into(),
            binding_id: "binding-a".into(),
            text: "lock".into(),
            token: picture1.clone(),
        };
        let target = h3_target(
            "reference_to_video",
            &["use-img", "use-img2", "use-voice", "use-voice2"],
        );
        let unbound_lines = draft(json!([line(
            "s1",
            &["f-act"],
            &["q1", "q2", "q3"],
            &["use-img"],
            "expression act"
        )]));
        let unbound = H3Projector
            .compile(&ProjectionCompileInput {
                mother: ReviewedMother {
                    document: &document,
                    canonical_fingerprint: "b",
                },
                references: &refs,
                identity_locks: std::slice::from_ref(&lock),
                target: &target,
                method: None,
                expression: Some(&unbound_lines),
            })
            .unwrap();
        let candidate = H3Projector
            .compile(&ProjectionCompileInput {
                mother: ReviewedMother {
                    document: &document,
                    canonical_fingerprint: "b",
                },
                references: &refs,
                identity_locks: std::slice::from_ref(&lock),
                target: &target,
                method: None,
                expression: Some(&lines),
            })
            .unwrap();
        assert_fully_mapped(&candidate);
        let text = &candidate.text;
        let defs = field(text, "subject_definitions");
        assert!(
            defs.contains(&format!(
                "{audio5} is the voice-timbre reference for lock (S1) and is not the finished soundtrack."
            )),
            "{defs}"
        );
        assert!(
            defs.contains(&format!(
                "{audio1} is the voice-timbre reference for the speaker (S2) and is not the finished soundtrack."
            )),
            "{defs}"
        );
        // Definition lines follow audio row order (use-voice then use-voice2).
        let i1 = defs
            .find(&format!(
                "{audio1} is the voice-timbre reference for the speaker (S2)"
            ))
            .unwrap();
        let i5 = defs
            .find(&format!(
                "{audio5} is the voice-timbre reference for lock (S1)"
            ))
            .unwrap();
        assert!(i1 < i5, "{defs}");
        let detail = field(text, "detailed_description");
        let s1_open = format!(
            "(S1), using the voice timbre referenced from {audio5}, <d>[Chinese] "
        );
        assert_eq!(detail.matches(&s1_open).count(), 2, "{detail}");
        assert!(detail.contains(&format!(
            "(S2), using the voice timbre referenced from {audio1}, <d>[Chinese] "
        )));
        assert!(!field(text, "retention_analysis").contains("(S"));
        assert_eq!(
            field(text, "overall_soundscape"),
            field(&unbound.text, "overall_soundscape")
        );
    }

    #[test]
    fn h3_voice_binding_offer_is_ref2va_timbre_only() {
        let refs = references();
        let ref2va = h3_target(
            "reference_to_video",
            &["use-voice", "use-song", "use-audio-bare", "use-img"],
        );
        assert_eq!(
            H3Projector.speaker_voice_reference_use_ids(&ref2va, &refs),
            vec!["use-voice".to_string()]
        );
        let i2v = h3_target("image_to_video", &["use-first", "use-voice"]);
        assert!(H3Projector
            .speaker_voice_reference_use_ids(&i2v, &refs)
            .is_empty());
        let t2v = h3_target("text_to_video", &[]);
        assert!(H3Projector
            .speaker_voice_reference_use_ids(&t2v, &refs)
            .is_empty());
    }

    #[test]
    fn h3_voice_binding_rejects_non_timbre_and_unknown_ids() {
        let document = DocumentDraftV1 {
            shots: vec![ShotDraftV1 {
                shot_id: "s1".into(),
                facts: vec![fact("f-act", "action", "mother action")],
                quotes: vec![QuoteIntentV1 {
                    quote_id: "q1".into(),
                    exact_text: "你好".into(),
                    role: "dialogue".into(),
                    speaker_id: Some("S1".into()),
                    source_unit_id: Some("unit-q1".into()),
                }],
            }],
        };
        let refs = references();
        let picture1 = apply_cite_template(CITE_TPL_H3_IMAGE, 1);
        let lock = IdentityLockLine {
            shot_id: "s1".into(),
            subject_id: "a".into(),
            binding_id: "binding-a".into(),
            text: "lock".into(),
            token: picture1,
        };
        let base_lines = draft(json!([line(
            "s1",
            &["f-act"],
            &["q1"],
            &["use-img"],
            "expression act"
        )]));
        let compile = |bindings: Vec<
            bgx_video_prompt_contract::projection::VideoPromptSpeakerVoiceBinding,
        >,
                       uses: &[&str]| {
            let mut lines = base_lines.clone();
            lines.speaker_voice_bindings = bindings;
            H3Projector.compile(&ProjectionCompileInput {
                mother: ReviewedMother {
                    document: &document,
                    canonical_fingerprint: "b",
                },
                references: &refs,
                identity_locks: std::slice::from_ref(&lock),
                target: &h3_target("reference_to_video", uses),
                method: None,
                expression: Some(&lines),
            })
        };
        assert_eq!(
            compile(
                vec![
                    bgx_video_prompt_contract::projection::VideoPromptSpeakerVoiceBinding {
                        speaker_id: "S1".into(),
                        voice_reference_use_id: "use-song".into(),
                        subject_id: None,
                    }
                ],
                &["use-img", "use-song"]
            ),
            Err(ProjectionPortError::ReferenceUseUnsupported {
                reference_use_id: "use-song".into()
            })
        );
        assert_eq!(
            compile(
                vec![
                    bgx_video_prompt_contract::projection::VideoPromptSpeakerVoiceBinding {
                        speaker_id: "S9".into(),
                        voice_reference_use_id: "use-voice".into(),
                        subject_id: None,
                    }
                ],
                &["use-img", "use-voice"]
            ),
            Err(ProjectionPortError::UnknownId {
                kind: ProjectionIdKind::Speaker,
                id: "S9".into()
            })
        );
        assert_eq!(
            compile(
                vec![
                    bgx_video_prompt_contract::projection::VideoPromptSpeakerVoiceBinding {
                        speaker_id: "S1".into(),
                        voice_reference_use_id: "use-voice".into(),
                        subject_id: Some("missing".into()),
                    }
                ],
                &["use-img", "use-voice"]
            ),
            Err(ProjectionPortError::UnknownId {
                kind: ProjectionIdKind::Subject,
                id: "missing".into()
            })
        );
    }
}
