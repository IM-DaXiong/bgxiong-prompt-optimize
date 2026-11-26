//! Project-format prompt compiler (PLAN 20261013 R17).
//! Turns a validated `DocumentDraftV1` into the byte-stable project-format text
//! used by mother/expression prompts. No host, Tauri, DB, or model dispatch.

use crate::document::{contract, validate_draft, DocumentDraftV1, ShotDraftV1};

pub fn render_project_format(draft: &DocumentDraftV1, language: &str) -> Result<String, String> {
    validate_draft(draft, true)?;
    let c = contract();
    let labels = if language.eq_ignore_ascii_case("en") {
        &c.labels_en
    } else {
        &c.labels_zh
    };
    let multi = draft.shots.len() > 1;
    let mut blocks = Vec::new();
    for (idx, shot) in draft.shots.iter().enumerate() {
        let mut lines = Vec::new();
        if multi {
            let shot_label = labels.get("shot").cloned().unwrap_or_else(|| "Shot".into());
            lines.push(format!("{shot_label} {}", idx + 1));
        }
        let picture_label = labels
            .get("picture")
            .cloned()
            .unwrap_or_else(|| "Picture".into());
        push_labeled_clauses(
            &mut lines,
            &picture_label,
            picture_clauses(shot, &c.slots, language),
        );
        push_quote_line(&mut lines, shot, "dialogue", labels.get("dialogue"));
        push_quote_line(&mut lines, shot, "voiceover", labels.get("voiceover"));
        push_quote_line(&mut lines, shot, "singing", labels.get("voiceover"));
        push_quote_line(&mut lines, shot, "visible_text", labels.get("visibleText"));
        let sound_label = labels
            .get("sound")
            .cloned()
            .unwrap_or_else(|| "Sound".into());
        push_labeled_clauses(
            &mut lines,
            &sound_label,
            slot_clauses(shot, "sound_music", language),
        );
        let continuity_label = labels
            .get("continuity")
            .cloned()
            .unwrap_or_else(|| "Continuity".into());
        push_labeled_clauses(
            &mut lines,
            &continuity_label,
            slot_clauses(shot, "continuity", language),
        );
        let constraints_label = labels
            .get("constraints")
            .cloned()
            .unwrap_or_else(|| "Constraints".into());
        push_labeled_clauses(
            &mut lines,
            &constraints_label,
            slot_clauses(shot, "constraints", language),
        );
        blocks.push(lines.join("\n"));
    }
    Ok(blocks.join("\n\n"))
}

const PICTURE_SLOTS: &[&str] = &[
    "subject",
    "action_interaction",
    "environment",
    "visual_style",
    "cinematography_style",
    "camera",
    "optics_composition",
    "lighting_color",
    "timing_physics",
];

fn picture_clauses(shot: &ShotDraftV1, slots: &[String], language: &str) -> Vec<String> {
    let mut lines = Vec::new();
    for slot in PICTURE_SLOTS {
        if !slots.iter().any(|s| s == *slot) {
            continue;
        }
        lines.extend(slot_clauses(shot, slot, language));
    }
    for fact in &shot.facts {
        if !PICTURE_SLOTS.contains(&fact.semantic_slot.as_str())
            && !["sound_music", "continuity", "constraints"].contains(&fact.semantic_slot.as_str())
        {
            lines.push(fact.text.clone());
        }
    }
    lines
}

fn slot_clauses(shot: &ShotDraftV1, slot: &str, language: &str) -> Vec<String> {
    shot.facts
        .iter()
        .filter(|f| f.semantic_slot == slot)
        .filter_map(|f| {
            let raw = f.text.trim();
            if raw.is_empty() {
                return None;
            }
            let body = if f.polarity.as_deref() == Some("negative") {
                format!("禁止{raw}")
            } else {
                raw.to_string()
            };
            let done = finish_clause(&body, language);
            if done.is_empty() {
                None
            } else {
                Some(done)
            }
        })
        .collect()
}

fn push_labeled_clauses(lines: &mut Vec<String>, label: &str, clauses: Vec<String>) {
    let mut first = true;
    for clause in clauses {
        if first {
            lines.push(format!("{label}：{clause}"));
            first = false;
        } else {
            lines.push(clause);
        }
    }
}

fn is_sentence_terminal(c: char) -> bool {
    matches!(c, '。' | '！' | '？' | '.' | '!' | '?' | '；' | ';')
}

/// Collapse joins the model sometimes writes inside one fact (`。，`).
fn collapse_broken_punct(text: &str) -> String {
    let mut s = text.to_string();
    loop {
        let next = s
            .replace("。，", "。")
            .replace("。,", "。")
            .replace("，。", "。")
            .replace(",。", "。")
            .replace("。。", "。")
            .replace("，，", "，");
        if next == s {
            break;
        }
        s = next;
    }
    s
}

fn finish_clause(text: &str, language: &str) -> String {
    let collapsed = collapse_broken_punct(text.trim());
    let trimmed = collapsed
        .trim_end_matches(['，', ',', '、', ' ', '\t'])
        .to_string();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.chars().last().is_some_and(is_sentence_terminal) {
        return trimmed;
    }
    let mark = if language.eq_ignore_ascii_case("en") {
        '.'
    } else {
        '。'
    };
    format!("{trimmed}{mark}")
}

fn push_quote_line(
    lines: &mut Vec<String>,
    shot: &ShotDraftV1,
    role: &str,
    label: Option<&String>,
) {
    let texts: Vec<&str> = shot
        .quotes
        .iter()
        .filter(|q| q.role == role)
        .map(|q| q.exact_text.as_str())
        .collect();
    if texts.is_empty() {
        return;
    }
    let label = label.map(|s| s.as_str()).unwrap_or(role);
    lines.push(format!("{label}：{}", texts.join(" ")));
}



#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{PromptFactV1, QuoteIntentV1};

    fn fact(id: &str, kind: &str, slot: &str, text: &str) -> PromptFactV1 {
        PromptFactV1 {
            fact_id: id.into(),
            kind: kind.into(),
            text: text.into(),
            semantic_slot: slot.into(),
            polarity: Some("positive".into()),
            provenance: "user_text".into(),
            source_fact_id: None,
            entity_ref_ids: vec![],
            source_unit_ids: vec![],
        }
    }

    fn sample() -> DocumentDraftV1 {
        DocumentDraftV1 {
            shots: vec![ShotDraftV1 {
                shot_id: "s1".into(),
                facts: vec![
                    fact("f1", "action", "action_interaction", "她转身跑开"),
                    fact("f2", "environment", "environment", "雨夜巷口"),
                    {
                        let mut ban = fact("f7", "ban", "constraints", "其他路人的脸");
                        ban.polarity = Some("negative".into());
                        ban
                    },
                ],
                quotes: vec![QuoteIntentV1 {
                    quote_id: "Q1".into(),
                    exact_text: "别回头".into(),
                    role: "dialogue".into(),
                    speaker_id: None,
                    source_unit_id: None,
                }],
            }],
        }
    }

    #[test]
    fn render_zh_is_byte_stable_and_contains_dialogue() {
        let d = sample();
        let a = render_project_format(&d, "zh").unwrap();
        let b = render_project_format(&d, "zh").unwrap();
        assert_eq!(a, b);
        assert!(a.contains("她转身跑开"));
        assert!(a.contains("对白：别回头") || a.contains("别回头"));
        assert!(a.contains("禁止其他路人的脸"));
    }

    #[test]
    fn finish_clause_collapses_broken_punct() {
        assert_eq!(collapse_broken_punct("停。，再走"), "停。再走");
        let done = finish_clause("没有句号", "zh");
        assert!(done.ends_with('。'));
        let en = finish_clause("no stop", "en");
        assert!(en.ends_with('.'));
    }
}
