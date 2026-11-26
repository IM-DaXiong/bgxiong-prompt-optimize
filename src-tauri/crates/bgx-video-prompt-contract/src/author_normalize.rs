//! Official mother-author reply normalize and multi-shot partition.
//!
//! This is the host `normalize_model_draft` / `normalize_author_reply` /
//! `partition_author_outcome` algorithm, lifted as pure contract logic so the
//! core and the host share one implementation. Callers may pass the official
//! mother-shot item schema; the schema checker is the existing pure
//! `bgx_creator_contract::schema_value::check_value` (same function the host
//! used via `check_schema_value`). No loose parse, no invented string rejection.

use std::collections::HashSet;

use serde::Deserialize;
use serde_json::Value;

use crate::document::{contract, DocumentDraftV1, PromptFactV1, QuoteIntentV1, ShotDraftV1};

/// Official refusal when a batch issue cannot be assigned to an expected shot.
pub const AUTHOR_BATCH_OWNERSHIP_UNREADABLE: &str =
    "这次批量结果缺少可读取的所有权信息，原稿已保留。";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DraftIssueV1 {
    pub shot_id: String,
    #[serde(default)]
    pub fact_id: String,
    pub code: String,
    #[serde(default)]
    pub detail: String,
}

pub fn draft_issue(
    shot_id: &str,
    fact_id: &str,
    code: &str,
    detail: impl Into<String>,
) -> DraftIssueV1 {
    DraftIssueV1 {
        shot_id: shot_id.to_string(),
        fact_id: fact_id.to_string(),
        code: code.to_string(),
        detail: detail.into(),
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelFactV1 {
    pub fact_id: String,
    #[serde(default)]
    pub kind: String,
    pub text: String,
    #[serde(default)]
    pub semantic_slot: Option<String>,
    #[serde(default)]
    pub polarity: Option<String>,
    #[serde(default)]
    pub inferred: bool,
    #[serde(default)]
    pub entity_ref_ids: Vec<String>,
    #[serde(default)]
    pub source_unit_ids: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelQuoteV1 {
    pub quote_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelShotV1 {
    pub shot_id: String,
    #[serde(default)]
    pub facts: Vec<ModelFactV1>,
    #[serde(default)]
    pub quotes: Vec<ModelQuoteV1>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModelDraftV1 {
    #[serde(default)]
    pub shots: Vec<ModelShotV1>,
}

/// Official per-shot ready / unreadable partition, without host review or CAS state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShotAuthorPartition {
    pub shot_id: String,
    pub draft: Option<ShotDraftV1>,
    pub pending_reason: Option<String>,
    pub issues: Vec<DraftIssueV1>,
}

pub fn ledger_source_provenance(source_mode: &str) -> &'static str {
    match source_mode {
        "generated_from_context" => "structured_storyboard",
        _ => "user_text",
    }
}

pub fn parse_model_draft(raw: &str) -> Result<ModelDraftV1, DraftIssueV1> {
    let v: Value = match serde_json::from_str(raw.trim()) {
        Ok(v) => v,
        Err(_) => {
            return Err(draft_issue(
                "",
                "",
                "DRAFT_JSON",
                "response is not a JSON object",
            ));
        }
    };
    serde_path_to_error::deserialize(v).map_err(|e| {
        draft_issue(
            "",
            "",
            "DRAFT_SHAPE",
            format!("{}: {}", e.path(), e.inner()),
        )
    })
}

/// Fill fields the model must not invent. Model `semanticSlot` is read only for multi-slot kinds.
/// Writing labels and source/entity annotations remain as authored metadata; actual output
/// ownership and readable content are checked separately.
pub fn normalize_model_draft(
    raw: &str,
    expected_shot_ids: &[String],
    source_mode: &str,
    _allow_inferred: bool,
    _registered_subject_ids: &[String],
    _source_unit_ids: &[String],
) -> (DocumentDraftV1, Vec<DraftIssueV1>) {
    let model = match parse_model_draft(raw) {
        Ok(m) => m,
        Err(err) => {
            return (DocumentDraftV1 { shots: vec![] }, vec![err]);
        }
    };
    let c = contract();
    let provenance = ledger_source_provenance(source_mode);
    let mut issues = Vec::new();
    let mut seen_shots = HashSet::new();
    let mut shots = Vec::new();
    for shot in &model.shots {
        if shot.shot_id.trim().is_empty() || !seen_shots.insert(shot.shot_id.clone()) {
            issues.push(draft_issue(&shot.shot_id, "", "SHOT_DUPLICATE", "shotId"));
            continue;
        }
        if !expected_shot_ids.is_empty() && !expected_shot_ids.iter().any(|id| id == &shot.shot_id)
        {
            issues.push(draft_issue(
                &shot.shot_id,
                "",
                "SHOT_UNKNOWN",
                "shotId not in ledger",
            ));
            continue;
        }
        let mut facts = Vec::new();
        let mut seen_facts = HashSet::new();
        for fact in &shot.facts {
            if fact.fact_id.trim().is_empty() || !seen_facts.insert(fact.fact_id.clone()) {
                issues.push(draft_issue(
                    &shot.shot_id,
                    &fact.fact_id,
                    "FACT_ID",
                    "factId",
                ));
                continue;
            }
            if fact.text.trim().is_empty() {
                issues.push(draft_issue(
                    &shot.shot_id,
                    &fact.fact_id,
                    "FACT_TEXT_EMPTY",
                    "",
                ));
                continue;
            }
            let slot = fact
                .semantic_slot
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .or_else(|| {
                    c.compatibility
                        .get(&fact.kind)
                        .filter(|slots| slots.len() == 1)
                        .and_then(|slots| slots.iter().next().cloned())
                })
                .unwrap_or_default();
            let fact_provenance = if fact.inferred {
                "inferred_open_dimension"
            } else {
                provenance
            };
            facts.push(PromptFactV1 {
                fact_id: fact.fact_id.clone(),
                kind: fact.kind.clone(),
                text: fact.text.clone(),
                semantic_slot: slot,
                polarity: fact.polarity.clone(),
                provenance: fact_provenance.to_string(),
                source_fact_id: None,
                entity_ref_ids: fact.entity_ref_ids.clone(),
                source_unit_ids: fact.source_unit_ids.clone(),
            });
        }
        let mut quotes = Vec::new();
        for q in shot.quotes.iter() {
            if q.quote_id.trim().is_empty() {
                issues.push(draft_issue(&shot.shot_id, "", "QUOTE_ID_EMPTY", ""));
                continue;
            }
            quotes.push(QuoteIntentV1 {
                quote_id: q.quote_id.clone(),
                exact_text: String::new(),
                role: String::new(),
                speaker_id: None,
                source_unit_id: None,
            });
        }
        if facts.is_empty() {
            issues.push(draft_issue(&shot.shot_id, "", "FACT_TEXT_EMPTY", "no facts"));
        }
        shots.push(ShotDraftV1 {
            shot_id: shot.shot_id.clone(),
            facts,
            quotes,
        });
    }
    for id in expected_shot_ids {
        if !seen_shots.contains(id) {
            issues.push(draft_issue(id, "", "SHOT_MISSING", "ledger shot missing"));
        }
    }
    (DocumentDraftV1 { shots }, issues)
}

/// Official batch walker: only owned expected shotIds are accepted; each child is
/// normalized with the full machine contract. `shot_item_schema` is the official
/// mother-shot item schema (`schema.properties.shots.items`); omit only when the
/// caller has no schema (tests of the walker itself may pass `None`).
pub fn normalize_author_reply(
    raw: &str,
    expected: &[String],
    mode: &str,
    inferred: bool,
    subjects: &[String],
    sources: &[String],
    shot_item_schema: Option<&Value>,
) -> (DocumentDraftV1, Vec<DraftIssueV1>) {
    let value: Value = match serde_json::from_str(raw) {
        Ok(value) => value,
        Err(error) => {
            return (
                DocumentDraftV1 { shots: vec![] },
                vec![draft_issue("", "", "DRAFT_JSON", error.to_string())],
            );
        }
    };
    let Some(rows) = value.get("shots").and_then(|value| value.as_array()) else {
        return (
            DocumentDraftV1 { shots: vec![] },
            vec![draft_issue(
                "",
                "",
                "DRAFT_SHAPE",
                "shots must be an array",
            )],
        );
    };
    if value.as_object().is_none_or(|object| object.len() != 1) {
        return (
            DocumentDraftV1 { shots: vec![] },
            vec![draft_issue(
                "",
                "",
                "DRAFT_SHAPE",
                "unreadable root fields",
            )],
        );
    }
    let mut result = DocumentDraftV1 { shots: vec![] };
    let mut issues = Vec::new();
    let mut seen = HashSet::new();
    for row in rows {
        let Some(id) = row
            .get("shotId")
            .and_then(|value| value.as_str())
            .filter(|id| expected.iter().any(|owned| owned.as_str() == *id))
        else {
            issues.push(draft_issue(
                "",
                "",
                "SHOT_UNKNOWN",
                "batch row has no owned shotId",
            ));
            continue;
        };
        if !seen.insert(id.to_string()) {
            issues.push(draft_issue(id, "", "SHOT_DUPLICATE", id));
            continue;
        }
        let row_raw = serde_json::json!({"shots": [row]}).to_string();
        let (mut draft, mut row_issues) = normalize_model_draft(
            &row_raw,
            &[id.to_string()],
            mode,
            inferred,
            subjects,
            sources,
        );
        if let Some(schema) = shot_item_schema {
            if let Err(error) =
                bgx_creator_contract::schema_value::check_value(schema, row, "motherShot")
            {
                row_issues.push(draft_issue(id, "", "DRAFT_SHAPE", error.to_string()));
            }
        }
        for issue in &mut row_issues {
            if issue.shot_id.is_empty() {
                issue.shot_id = id.into();
            }
        }
        result.shots.append(&mut draft.shots);
        issues.append(&mut row_issues);
    }
    for id in expected.iter().filter(|id| !seen.contains(*id)) {
        issues.push(draft_issue(
            id,
            "",
            "SHOT_MISSING",
            "model did not return this shot",
        ));
    }
    (result, issues)
}

pub fn partition_author_outcome(
    expected: &[String],
    draft: &DocumentDraftV1,
    issues: &[DraftIssueV1],
) -> Result<Vec<ShotAuthorPartition>, String> {
    if issues
        .iter()
        .any(|issue| issue.shot_id.is_empty() || !expected.contains(&issue.shot_id))
    {
        return Err(AUTHOR_BATCH_OWNERSHIP_UNREADABLE.into());
    }
    Ok(expected
        .iter()
        .map(|id| {
            let local: Vec<_> = issues
                .iter()
                .filter(|issue| issue.shot_id == *id)
                .cloned()
                .collect();
            let ready = draft
                .shots
                .iter()
                .find(|shot| shot.shot_id == *id && !shot.facts.is_empty())
                .filter(|_| local.is_empty())
                .cloned();
            ShotAuthorPartition {
                shot_id: id.clone(),
                pending_reason: if ready.is_some() {
                    None
                } else {
                    Some("MODEL_OUTPUT_UNREADABLE".into())
                },
                draft: ready,
                issues: local,
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn expected() -> Vec<String> {
        vec!["s1".into(), "s2".into()]
    }

    fn fact(shot: &str, fact: &str, text: &str) -> Value {
        json!({
            "shotId": shot,
            "facts": [{
                "factId": fact,
                "kind": "action",
                "text": text,
                "entityRefIds": [],
                "sourceUnitIds": ["u1"]
            }],
            "quotes": []
        })
    }

    #[test]
    fn multi_shot_envelope_partitions_ready_and_pending_children() {
        let raw = json!({
            "shots": [
                fact("s1", "f1", "她推门走进雨夜巷口"),
                {
                    "shotId": "s2",
                    "facts": [{
                        "factId": "f2",
                        "kind": "action",
                        "text": "",
                        "entityRefIds": [],
                        "sourceUnitIds": ["u1"]
                    }],
                    "quotes": []
                }
            ]
        })
        .to_string();
        let (draft, issues) = normalize_author_reply(
            &raw,
            &expected(),
            "optimize_existing",
            false,
            &[],
            &["u1".into()],
            None,
        );
        let parts = partition_author_outcome(&expected(), &draft, &issues).expect("owned");
        assert_eq!(parts.len(), 2);
        assert!(parts[0].pending_reason.is_none());
        assert_eq!(parts[0].draft.as_ref().unwrap().shot_id, "s1");
        assert_eq!(
            parts[1].pending_reason.as_deref(),
            Some("MODEL_OUTPUT_UNREADABLE")
        );
        assert!(parts[1]
            .issues
            .iter()
            .any(|issue| issue.code == "FACT_TEXT_EMPTY"));
    }

    #[test]
    fn unknown_or_unowned_shot_is_not_guessed_and_fails_partition() {
        let raw = json!({"shots": [fact("s9", "f1", "她推门走进雨夜巷口")]}).to_string();
        let (draft, issues) = normalize_author_reply(
            &raw,
            &expected(),
            "optimize_existing",
            false,
            &[],
            &[],
            None,
        );
        assert!(issues
            .iter()
            .any(|issue| issue.code == "SHOT_UNKNOWN" && issue.shot_id.is_empty()));
        assert!(issues.iter().any(|issue| issue.code == "SHOT_MISSING"));
        let err = partition_author_outcome(&expected(), &draft, &issues).unwrap_err();
        assert_eq!(err, AUTHOR_BATCH_OWNERSHIP_UNREADABLE);
    }

    #[test]
    fn extra_root_fields_are_rejected_as_unreadable_shape() {
        let raw = json!({
            "shots": [fact("s1", "f1", "她推门走进雨夜巷口")],
            "note": "no"
        })
        .to_string();
        let (draft, issues) = normalize_author_reply(
            &raw,
            &["s1".into()],
            "optimize_existing",
            false,
            &[],
            &[],
            None,
        );
        assert!(draft.shots.is_empty());
        assert_eq!(issues[0].code, "DRAFT_SHAPE");
        assert_eq!(issues[0].detail, "unreadable root fields");
    }

    #[test]
    fn official_shot_item_schema_marks_closed_set_violations() {
        let schema = json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["shotId", "facts"],
            "properties": {
                "shotId": {"type": "string", "enum": ["s1"]},
                "facts": {"type": "array", "minItems": 1},
                "quotes": {"type": "array"}
            }
        });
        let raw = json!({
            "shots": [{
                "shotId": "s1",
                "facts": [{
                    "factId": "f1",
                    "kind": "action",
                    "text": "她推门走进雨夜巷口",
                    "entityRefIds": [],
                    "sourceUnitIds": ["u1"]
                }],
                "quotes": [],
                "extra": true
            }]
        })
        .to_string();
        let (draft, issues) = normalize_author_reply(
            &raw,
            &["s1".into()],
            "optimize_existing",
            false,
            &[],
            &["u1".into()],
            Some(&schema),
        );
        assert!(issues
            .iter()
            .any(|issue| issue.code == "DRAFT_SHAPE" && issue.shot_id == "s1"));
        let parts = partition_author_outcome(&["s1".into()], &draft, &issues).expect("owned");
        assert_eq!(
            parts[0].pending_reason.as_deref(),
            Some("MODEL_OUTPUT_UNREADABLE")
        );
    }
}