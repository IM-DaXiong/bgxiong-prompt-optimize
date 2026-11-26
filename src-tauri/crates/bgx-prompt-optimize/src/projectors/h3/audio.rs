//! 模块：video_prompt_adapters::h3::audio
//! 作用：H3 目标的音频段落、人声行与 `<d>` 语法（VS32，架构 PLAN §5.1）。
//!
//! - 参考音行只来自 `reference_audio` 冻结行（按类型相等判断）；参考视频自带的原音轨不会自动
//!   变成 <Audio N>。用途只认闭集（`soundtrack_copy` / `voice_timbre`），音轨复用最多一路。
//! - 人声行按母版 / 冻结人声的 quoteRole（dialogue / voiceover / singing）生成官方句式，原文只放进
//!   `<d>[语言] …</d>`，说话人编号 (Sx) 来自冻结人声行，quoteId 不变。
//! - 音轨复用：只有一个可见演唱者，同步唱且不另加第二路人声；音色参考：只作音色说明，不复制
//!   参考音里的台词或歌词。
//! - 歌词识别提示词不在本文件（属来源归一化，`semantic_control::source`）。

#[cfg(test)]
use crate::projectors::cite_tokens::{apply_cite_template, CITE_TPL_H3_AUDIO, CITE_TPL_H3_IMAGE};
#[cfg(test)]
use bgx_video_prompt_contract::inventory::ReferenceMediaInventoryItem;

/// Closing tag of an H3 vocal block.
pub const D_CLOSE: &str = "</d>";

/// `<Audio N>` of one frozen audio ordinal.
#[cfg(test)]
pub fn audio_token(ordinal: u32) -> String {
    apply_cite_template(CITE_TPL_H3_AUDIO, ordinal)
}

/// `<Picture N>` of one frozen picture number.
#[cfg(test)]
pub fn picture_token(number: u32) -> String {
    apply_cite_template(CITE_TPL_H3_IMAGE, number)
}

/// Closed audio-use set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum H3AudioUse {
    /// The supplied track is the finished soundtrack of the target video.
    SoundtrackCopy,
    /// Only the voice timbre is referenced; the signal is not copied.
    VoiceTimbre,
}

impl H3AudioUse {
    pub fn is_soundtrack_copy(self) -> bool {
        self == H3AudioUse::SoundtrackCopy
    }
}

/// One frozen H3 audio reference.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct H3AudioRow {
    pub reference_use_id: String,
    pub ordinal: u32,
    pub token: String,
    pub usage: H3AudioUse,
    /// Closed-set wire value of the usage.
    pub usage_wire: &'static str,
}

/// Vocal roles that write a `<d>` block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum H3VocalRole {
    Dialogue,
    Voiceover,
    Singing,
}

impl H3VocalRole {
    /// Semantic quote role → vocal role; visible text and non-vocal quotes write no `<d>`.
    pub fn of_quote_role(role: &str) -> Option<Self> {
        match role {
            "dialogue" => Some(H3VocalRole::Dialogue),
            "voiceover" => Some(H3VocalRole::Voiceover),
            "singing" => Some(H3VocalRole::Singing),
            _ => None,
        }
    }
}

/// Language tag inside `<d>[...]` for one verbatim vocal quote.
pub fn vocal_language_tag(exact_text: &str) -> &'static str {
    if exact_text
        .chars()
        .any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c))
    {
        "Chinese"
    } else {
        "original"
    }
}

/// `<d>[Language] ` opening of one vocal block.
pub fn d_open(exact_text: &str) -> String {
    format!("<d>[{}] ", vocal_language_tag(exact_text))
}

/// Speaker id of a frozen vocal occurrence (`S1` when the plan carries none).
pub fn speaker_label(speaker_id: &str) -> &str {
    let id = speaker_id.trim();
    if id.is_empty() {
        "S1"
    } else {
        id
    }
}

/// Subject marker of the single visible performer of a soundtrack copy (only one performer is
/// ever compiled; several candidates are a projection error, CF-43).
pub const H3_PERFORMER_SUBJECT: &str = "<Subject 1>";

/// The single visible performer of a soundtrack copy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct H3SoundtrackPerformer {
    pub subject_token: String,
    pub speaker_id: String,
    pub picture_token: String,
    pub audio_token: String,
}

/// Unnamed speaker placeholder when a voice-timbre row has no identity-lock subject.
pub const H3_UNNAMED_SPEAKER: &str = "the speaker";

/// Official vocal sentence for one frozen occurrence. With a soundtrack copy, singing follows the
/// copied track and no second vocal performance is written.
#[cfg(test)]
pub fn vocal_sentence(
    role: H3VocalRole,
    exact_text: &str,
    speaker_id: &str,
    copy: Option<&H3SoundtrackPerformer>,
    voice: Option<&str>,
) -> String {
    let (before, after) = vocal_frame(role, exact_text, speaker_id, copy, voice);
    format!("{before}{exact_text}{after}")
}

/// [`vocal_sentence`] split around the verbatim text, so the compiler can map the text itself to
/// its quote: `before` ends with the `<d>[Language] ` opening, `after` starts with `</d>`.
pub fn vocal_frame(
    role: H3VocalRole,
    exact_text: &str,
    speaker_id: &str,
    copy: Option<&H3SoundtrackPerformer>,
    voice: Option<&str>,
) -> (String, String) {
    let open = d_open(exact_text);
    let speaker = speaker_label(speaker_id);
    match (role, copy) {
        (H3VocalRole::Singing, Some(performer)) => (
            format!(
                "{} ({speaker}) sings in exact synchronization with {}, {open}",
                performer.subject_token, performer.audio_token
            ),
            format!("{D_CLOSE}."),
        ),
        (H3VocalRole::Singing, None) => {
            let before = match voice {
                Some(a) => format!(
                    "({speaker}) sings, using the voice timbre referenced from {a}, {open}"
                ),
                None => format!("({speaker}) sings {open}"),
            };
            (before, D_CLOSE.to_string())
        }
        (H3VocalRole::Voiceover, _) => {
            let before = match voice {
                Some(a) => format!(
                    "({speaker}) says in an off-screen voiceover, using the voice timbre referenced from {a}: {open}"
                ),
                None => format!("({speaker}) says in an off-screen voiceover: {open}"),
            };
            (
                before,
                format!("{D_CLOSE} while the on-screen lips remain completely closed."),
            )
        }
        (H3VocalRole::Dialogue, _) => {
            let before = match voice {
                Some(a) => {
                    format!("({speaker}), using the voice timbre referenced from {a}, {open}")
                }
                None => format!("({speaker}) {open}"),
            };
            (before, D_CLOSE.to_string())
        }
    }
}

/// Lip-sync line of a soundtrack copy without sung text.
pub fn lip_sync_sentence(performer: &H3SoundtrackPerformer) -> String {
    format!(
        "{} ({}) lip movements stay synchronized to {}. {}",
        performer.subject_token,
        performer.speaker_id,
        performer.audio_token,
        use_exactly_sentence(&performer.audio_token)
    )
}

/// The copied track is used unchanged.
pub fn use_exactly_sentence(audio_token: &str) -> String {
    format!("Use {audio_token} exactly as it is.")
}

/// Soundtrack-copy performer line for `subject_definitions`.
pub fn performer_definition_line(performer: &H3SoundtrackPerformer) -> String {
    format!(
        "{} is the visible performer defined by {}. {} is the copied finished soundtrack used by the performer for synchronized singing.",
        performer.subject_token, performer.picture_token, performer.audio_token
    )
}

/// Unbound voice-timbre line for `subject_definitions`.
pub fn voice_timbre_unbound_line(audio_token: &str) -> String {
    format!("{audio_token} is a voice timbre reference and is not the finished soundtrack.")
}

/// Bound voice-timbre line parts for `subject_definitions` (prefix → subject → suffix).
pub fn voice_timbre_bound_parts(audio_token: &str, speaker: &str) -> (String, String) {
    (
        format!("{audio_token} is the voice-timbre reference for "),
        format!(" ({speaker}) and is not the finished soundtrack."),
    )
}

/// Official summary with a soundtrack copy; `None` keeps the compiled summary (no copy).
pub fn summary_sentence(
    performer: Option<&H3SoundtrackPerformer>,
    has_singing: bool,
) -> Option<String> {
    let performer = performer?;
    let (subject, audio) = (&performer.subject_token, &performer.audio_token);
    Some(if has_singing {
        format!("[reference generation + audio reuse] {subject} performs the song while lip movements stay synchronized to {audio}.")
    } else {
        format!("[reference generation + audio reuse] {subject} lip movements stay synchronized to {audio}.")
    })
}

/// `retention_analysis` lines of the audio references.
pub fn retention_lines(rows: &[H3AudioRow]) -> Vec<String> {
    rows.iter()
        .map(|row| match row.usage {
            H3AudioUse::SoundtrackCopy => format!(
                "{}: fully_copy - use the supplied soundtrack as the target video's finished audio signal.",
                row.token
            ),
            H3AudioUse::VoiceTimbre => format!(
                "{}: reference - voice timbre only; do not copy the supplied signal.",
                row.token
            ),
        })
        .collect()
}

/// Soundscape body of a soundtrack copy; `None` keeps the compiled soundscape.
pub fn soundscape_sentence(rows: &[H3AudioRow]) -> Option<String> {
    let copy = rows.iter().find(|r| r.usage.is_soundtrack_copy())?;
    Some(format!(
        "{} Do not add a second vocal performance.",
        use_exactly_sentence(&copy.token)
    ))
}

/// Non-diegetic music body of a soundtrack copy; `None` keeps the compiled body.
pub fn music_sentence(rows: &[H3AudioRow]) -> Option<String> {
    let copy = rows.iter().find(|r| r.usage.is_soundtrack_copy())?;
    Some(format!(
        "The music already contained in {} is reused as part of that copied soundtrack; do not duplicate it.",
        copy.token
    ))
}

/// Remove every `<d>…</d>` block (the projector re-inserts frozen occurrences).
#[cfg(test)]
pub fn strip_d_tags(text: &str) -> String {
    let lower = text.to_ascii_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut from = 0usize;
    while let Some(rel) = lower[from..].find("<d>") {
        let open = from + rel;
        let inner_start = open + 3;
        let Some(close_rel) = lower[inner_start..].find(D_CLOSE) else {
            break;
        };
        out.push_str(&text[from..open]);
        from = inner_start + close_rel + D_CLOSE.len();
    }
    out.push_str(&text[from..]);
    out
}

/// Verbatim texts inside every `<d>` block, language tags removed.
#[cfg(test)]
pub fn d_inners(text: &str) -> Vec<String> {
    let lower = text.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(rel) = lower[from..].find("<d>") {
        let inner_start = from + rel + 3;
        let Some(close_rel) = lower[inner_start..].find(D_CLOSE) else {
            break;
        };
        let inner_end = inner_start + close_rel;
        out.push(strip_lang_tag(text[inner_start..inner_end].trim()));
        from = inner_end + D_CLOSE.len();
    }
    out
}

#[cfg(test)]
fn strip_lang_tag(inner: &str) -> String {
    let mut s = inner.trim().to_string();
    loop {
        let t = s.trim();
        if !t.starts_with('[') {
            break;
        }
        let Some(end) = t.find(']') else { break };
        let tag = &t[1..end];
        if tag.is_empty() || !tag.chars().all(|c| c.is_ascii_alphabetic()) {
            break;
        }
        s = t[end + 1..].trim().to_string();
    }
    s.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, kind: &str, usage: Option<&str>) -> ReferenceMediaInventoryItem {
        ReferenceMediaInventoryItem {
            item_id: id.into(),
            media_kind: kind.into(),
            audio_usage: usage.map(str::to_string),
            ..Default::default()
        }
    }

    // Freeze numbered test inputs, then exercise the actual production audio-row compiler.
    fn plan_audio_rows<'a>(
        items: impl IntoIterator<Item = &'a ReferenceMediaInventoryItem>,
    ) -> Result<Vec<H3AudioRow>, crate::projectors::port::ProjectionPortError>
    {
        let mut frozen: Vec<_> = items.into_iter().cloned().collect();
        let mut ordinal = 0;
        let mut tokens = std::collections::BTreeMap::new();
        for item in &mut frozen {
            if item.media_kind == "reference_audio" {
                ordinal += 1;
                item.ordinal_within_kind = ordinal;
                tokens.insert(item.item_id.clone(), audio_token(ordinal));
            }
        }
        super::super::compile::audio_rows(&frozen.iter().collect::<Vec<_>>(), &tokens)
    }

    fn performer() -> H3SoundtrackPerformer {
        H3SoundtrackPerformer {
            subject_token: H3_PERFORMER_SUBJECT.into(),
            speaker_id: "S1".into(),
            picture_token: picture_token(1),
            audio_token: audio_token(1),
        }
    }

    #[test]
    fn only_reference_audio_rows_become_audio_tokens() {
        let items = vec![
            item("vid", "reference_video", Some("soundtrack_copy")),
            item("img", "reference_image", None),
            item("voice", "reference_audio", Some("voice_timbre")),
            item("song", "reference_audio", Some("soundtrack_copy")),
        ];
        let rows = plan_audio_rows(items.iter()).unwrap();
        let ids: Vec<&str> = rows.iter().map(|r| r.reference_use_id.as_str()).collect();
        assert_eq!(ids, vec!["voice", "song"]);
        assert_eq!(rows[0].token, audio_token(1));
        assert_eq!(rows[1].usage, H3AudioUse::SoundtrackCopy);
        let two_copies = vec![
            item("a", "reference_audio", Some("soundtrack_copy")),
            item("b", "reference_audio", Some("soundtrack_copy")),
        ];
        assert_eq!(
            plan_audio_rows(two_copies.iter()),
            Err(crate::projectors::port::ProjectionPortError::ReferenceUseUnsupported { reference_use_id: "b".into() })
        );
        let unknown = vec![item("a", "reference_audio", Some("remix"))];
        assert_eq!(
            plan_audio_rows(unknown.iter()),
            Err(crate::projectors::port::ProjectionPortError::ReferenceUseUnsupported {
                reference_use_id: "a".into()
            })
        );
    }

    #[test]
    fn soundtrack_copy_has_one_synchronized_performer_and_no_second_voice() {
        let rows =
            plan_audio_rows(vec![item("song", "reference_audio", Some("soundtrack_copy"))].iter())
                .unwrap();
        let performer = performer();
        let line = vocal_sentence(H3VocalRole::Singing, "山河入梦。", "", Some(&performer), None);
        assert_eq!(line.matches("(S").count(), 1);
        assert!(line.contains("sings in exact synchronization with <Audio 1>"));
        assert!(line.contains("<d>[Chinese] 山河入梦。</d>"));
        let soundscape = soundscape_sentence(&rows).unwrap();
        assert!(soundscape.contains("Do not add a second vocal performance"));
        assert!(soundscape.contains("exactly as it is"));
        let def = performer_definition_line(&performer);
        assert!(def.contains("<Subject 1>"));
        assert!(def.contains("<Audio 1>"));
        assert!(retention_lines(&rows)[0].contains("fully_copy"));
    }

    #[test]
    fn voice_timbre_never_copies_source_lines() {
        let rows =
            plan_audio_rows(vec![item("voice", "reference_audio", Some("voice_timbre"))].iter())
                .unwrap();
        let mut written = vec![voice_timbre_unbound_line(&rows[0].token)];
        written.extend(retention_lines(&rows));
        for line in &written {
            assert!(!line.contains("<d>"), "{line}");
            assert!(line.contains("voice timbre"), "{line}");
            assert!(!line.contains("fully_copy"), "{line}");
        }
        assert_eq!(soundscape_sentence(&rows), None);
        assert_eq!(music_sentence(&rows), None);
        assert_eq!(summary_sentence(None, true), None);
    }

    #[test]
    fn vocal_blocks_round_trip_verbatim() {
        let text = format!(
            "a {} b {}",
            vocal_sentence(H3VocalRole::Dialogue, "别回头。", "S2", None, None),
            vocal_sentence(H3VocalRole::Voiceover, "I still remember.", "", None, None)
        );
        assert_eq!(d_inners(&text), vec!["别回头。", "I still remember."]);
        assert!(text.contains("(S2) <d>[Chinese] 别回头。</d>"));
        assert!(text.contains("says in an off-screen voiceover"));
        assert!(!strip_d_tags(&text).contains("<d>"));
        assert_eq!(H3VocalRole::of_quote_role("visible_text"), None);
    }

    #[test]
    fn bound_voice_frames_cite_the_audio_outside_the_vocal_block() {
        let a = audio_token(2);
        let (d_before, d_after) =
            vocal_frame(H3VocalRole::Dialogue, "别回头。", "S1", None, Some(a.as_str()));
        assert_eq!(
            d_before,
            format!("(S1), using the voice timbre referenced from {a}, <d>[Chinese] ")
        );
        assert_eq!(d_after, "</d>");

        let (v_before, v_after) = vocal_frame(
            H3VocalRole::Voiceover,
            "I still remember.",
            "S2",
            None,
            Some(a.as_str()),
        );
        assert_eq!(
            v_before,
            format!(
                "(S2) says in an off-screen voiceover, using the voice timbre referenced from {a}: <d>[original] "
            )
        );
        assert_eq!(
            v_after,
            "</d> while the on-screen lips remain completely closed."
        );

        let (s_before, s_after) =
            vocal_frame(H3VocalRole::Singing, "山河入梦。", "S1", None, Some(a.as_str()));
        assert_eq!(
            s_before,
            format!("(S1) sings, using the voice timbre referenced from {a}, <d>[Chinese] ")
        );
        assert_eq!(s_after, "</d>");

        let text = format!(
            "{} {} {}",
            vocal_sentence(H3VocalRole::Dialogue, "别回头。", "S1", None, Some(a.as_str())),
            vocal_sentence(
                H3VocalRole::Voiceover,
                "I still remember.",
                "S2",
                None,
                Some(a.as_str())
            ),
            vocal_sentence(H3VocalRole::Singing, "山河入梦。", "S1", None, Some(a.as_str())),
        );
        assert_eq!(
            d_inners(&text),
            vec!["别回头。", "I still remember.", "山河入梦。"]
        );
        let lower = text.to_ascii_lowercase();
        let mut from = 0usize;
        while let Some(rel) = lower[from..].find("<d>") {
            let open = from + rel;
            let inner_start = open + 3;
            let Some(close_rel) = lower[inner_start..].find("</d>") else {
                break;
            };
            let inner_end = inner_start + close_rel;
            assert!(
                !text[inner_start..inner_end].contains("<Audio"),
                "audio token leaked into vocal block: {}",
                &text[inner_start..inner_end]
            );
            from = inner_end + 4;
        }
        let (prefix, suffix) = voice_timbre_bound_parts(&a, "S1");
        assert_eq!(prefix, format!("{a} is the voice-timbre reference for "));
        assert_eq!(suffix, " (S1) and is not the finished soundtrack.");
        assert_eq!(H3_UNNAMED_SPEAKER, "the speaker");
    }
}
