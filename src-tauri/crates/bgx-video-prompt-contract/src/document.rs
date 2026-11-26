//! Reviewed mother document draft + structural validate (PLAN 20261013 R14a/R14b).
//! Host keeps schema/prompt TextOutputSchema wiring; this crate owns closed-set draft checks.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::identity::{
    MOTHER_PROMPT_VERSION, PROJECT_FORMAT_VERSION, PROJECTOR_VERSION, REVIEW_PROMPT_VERSION,
    SEMANTIC_CONTRACT_VERSION,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PromptFactV1 {
    pub fact_id: String,
    pub kind: String,
    pub text: String,
    pub semantic_slot: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub polarity: Option<String>,
    pub provenance: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_fact_id: Option<String>,
    /// Subject references of every kind, kept verbatim from the validated model draft (VPR013).
    pub entity_ref_ids: Vec<String>,
    /// Frozen source units this fact comes from, kept verbatim from the validated model draft.
    /// Empty only for an inferred fact or when the frozen source has no units.
    pub source_unit_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct QuoteIntentV1 {
    pub quote_id: String,
    pub exact_text: String,
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_unit_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ShotDraftV1 {
    pub shot_id: String,
    pub facts: Vec<PromptFactV1>,
    #[serde(default)]
    pub quotes: Vec<QuoteIntentV1>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DocumentDraftV1 {
    pub shots: Vec<ShotDraftV1>,
}

const CONTRACT_JSON: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../packages/app-contracts/v1/video-prompt-semantic-v1.json"
));

#[derive(Debug, Clone)]
pub struct SemanticContract {
    pub slots: Vec<String>,
    pub kinds: HashSet<String>,
    pub quote_roles: HashSet<String>,
    pub compatibility: HashMap<String, HashSet<String>>,
    pub provenances: HashSet<String>,
    pub issue_codes: HashSet<String>,
    pub mother_review_issue_codes: HashSet<String>,
    pub pending_reasons: HashSet<String>,
    pub max_repair_rounds: u32,
    pub method_resource_max_chars: u64,
    pub labels_zh: HashMap<String, String>,
    pub labels_en: HashMap<String, String>,
    pub project_format_version: u32,
    pub schema_version: u32,
    pub projector_version: u32,
    pub mother_prompt_version: u32,
    pub review_prompt_version: u32,
}

pub fn contract_identity() -> (u32, u32, u32, u32, usize, usize) {
    let c = contract();
    (
        c.schema_version,
        c.projector_version,
        c.mother_prompt_version,
        c.review_prompt_version,
        c.pending_reasons.len(),
        c.quote_roles.len(),
    )
}

pub fn contract() -> &'static SemanticContract {
    static CELL: OnceLock<SemanticContract> = OnceLock::new();
    CELL.get_or_init(|| {
        let v: Value = serde_json::from_str(CONTRACT_JSON).expect("video-prompt-semantic-v1.json");
        let slots = string_list(&v, "semanticSlots");
        let kinds: HashSet<String> = string_list(&v, "factKinds").into_iter().collect();
        let quote_roles: HashSet<String> = string_list(&v, "quoteRoles").into_iter().collect();
        let provenances: HashSet<String> = string_list(&v, "provenances").into_iter().collect();
        let mut compatibility = HashMap::new();
        if let Some(obj) = v.get("kindSlotCompatibility").and_then(|x| x.as_object()) {
            for (k, val) in obj {
                let set = val
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default();
                compatibility.insert(k.clone(), set);
            }
        }
        let issue_codes: HashSet<String> = string_list(&v, "issueCodes").into_iter().collect();
        let review_codes = string_list(&v, "motherReviewIssueCodes");
        let mother_review_issue_codes: HashSet<String> = review_codes.iter().cloned().collect();
        assert!(
            !review_codes.is_empty()
                && mother_review_issue_codes.len() == review_codes.len()
                && mother_review_issue_codes.is_subset(&issue_codes),
            "motherReviewIssueCodes must be a non-empty unique subset of issueCodes"
        );
        SemanticContract {
            slots,
            kinds,
            quote_roles,
            compatibility,
            provenances,
            issue_codes,
            mother_review_issue_codes,
            pending_reasons: string_list(&v, "pendingReasons").into_iter().collect(),
            max_repair_rounds: v
                .get("maxSemanticRepairRounds")
                .and_then(|x| x.as_u64())
                .unwrap_or(3) as u32,
            method_resource_max_chars: v
                .get("methodResourceMaxChars")
                .and_then(|x| x.as_u64())
                .filter(|n| *n > 0)
                .expect("video-prompt-semantic-v1.json methodResourceMaxChars"),
            labels_zh: label_map(&v, "zh"),
            labels_en: label_map(&v, "en"),
            project_format_version: PROJECT_FORMAT_VERSION,
            schema_version: SEMANTIC_CONTRACT_VERSION,
            projector_version: PROJECTOR_VERSION,
            mother_prompt_version: MOTHER_PROMPT_VERSION,
            review_prompt_version: REVIEW_PROMPT_VERSION,
        }
    })
}

fn string_list(v: &Value, key: &str) -> Vec<String> {
    v.get(key)
        .and_then(|x| x.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

fn label_map(v: &Value, lang: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let Some(obj) = v
        .pointer(&format!("/sectionLabels/{lang}"))
        .and_then(|x| x.as_object())
    else {
        return out;
    };
    for (k, val) in obj {
        if let Some(s) = val.as_str() {
            out.insert(k.clone(), s.to_string());
        }
    }
    out
}

pub fn kind_slot_compatible(kind: &str, slot: &str) -> bool {
    contract()
        .compatibility
        .get(kind)
        .map(|s| s.contains(slot))
        .unwrap_or(false)
}

pub fn validate_draft(draft: &DocumentDraftV1, _allow_inferred: bool) -> Result<(), String> {
    let c = contract();
    if draft.shots.is_empty() {
        return Err("VID_SEM_DRAFT_EMPTY".into());
    }
    let mut seen_shots = HashSet::new();
    for shot in &draft.shots {
        if shot.shot_id.trim().is_empty() || !seen_shots.insert(shot.shot_id.clone()) {
            return Err("VID_SEM_SHOT_ID".into());
        }
        let mut seen_facts = HashSet::new();
        if shot.facts.is_empty() {
            return Err(format!("VID_SEM_SHOT_FACTS_EMPTY:{}", shot.shot_id));
        }
        for fact in &shot.facts {
            if fact.fact_id.trim().is_empty() || !seen_facts.insert(fact.fact_id.clone()) {
                return Err("VID_SEM_FACT_ID".into());
            }
            if !c.provenances.contains(&fact.provenance) {
                return Err(format!("VID_SEM_PROVENANCE:{}", fact.provenance));
            }
            if fact.text.trim().is_empty() {
                return Err("VID_SEM_FACT_TEXT".into());
            }
        }
        for q in &shot.quotes {
            if q.exact_text.trim().is_empty() {
                return Err("VID_SEM_QUOTE_EMPTY".into());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_draft_roundtrip_json() {
        let draft = DocumentDraftV1 {
            shots: vec![ShotDraftV1 {
                shot_id: "S1".into(),
                facts: vec![PromptFactV1 {
                    fact_id: "f1".into(),
                    kind: "action".into(),
                    text: "walks".into(),
                    semantic_slot: "picture".into(),
                    polarity: None,
                    provenance: "user_text".into(),
                    source_fact_id: None,
                    entity_ref_ids: vec![],
                    source_unit_ids: vec!["u1".into()],
                }],
                quotes: vec![],
            }],
        };
        let v = serde_json::to_value(&draft).expect("ser");
        let back: DocumentDraftV1 = serde_json::from_value(v).expect("de");
        assert_eq!(back, draft);
    }

    #[test]
    fn validate_draft_accepts_minimal_source_fact() {
        let draft = DocumentDraftV1 {
            shots: vec![ShotDraftV1 {
                shot_id: "S1".into(),
                facts: vec![PromptFactV1 {
                    fact_id: "f1".into(),
                    kind: "action".into(),
                    text: "walks".into(),
                    semantic_slot: "picture".into(),
                    polarity: None,
                    provenance: "user_text".into(),
                    source_fact_id: None,
                    entity_ref_ids: vec![],
                    source_unit_ids: vec![],
                }],
                quotes: vec![],
            }],
        };
        assert!(validate_draft(&draft, false).is_ok());
        assert!(contract().schema_version >= 1);
        assert!(!contract().provenances.is_empty());
    }
}
