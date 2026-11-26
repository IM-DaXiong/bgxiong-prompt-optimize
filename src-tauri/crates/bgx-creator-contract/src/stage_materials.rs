//! CT06: stage materials parsing and structural checks (moved from
//! `creator_requests::stage_materials`); verified together with the turn contract. The runtime
//! keeps only material assembly.

use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;

/// Keys of the two model stages (SR02: operation and review only).
pub const STAGE_KEYS: [&str; 2] = ["operation", "review"];

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StageEntry {
    pub rules: Vec<String>,
    pub closed_sets: Vec<String>,
    pub examples: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StageMaterials {
    pub note: String,
    pub rules: BTreeMap<String, String>,
    pub stages: BTreeMap<String, StageEntry>,
}

impl StageMaterials {
    /// Parse `stageMaterials` from the contract root and check it against the root's closed sets
    /// and examples.
    pub fn parse_and_verify(root: &Value) -> Result<Self, String> {
        let f: StageMaterials = serde_json::from_value(
            root.get("stageMaterials")
                .cloned()
                .ok_or_else(|| "STAGE_MATERIALS_PARSE: stageMaterials missing".to_string())?,
        )
        .map_err(|e| format!("STAGE_MATERIALS_PARSE: {e}"))?;
        verify(&f, root)?;
        Ok(f)
    }

    pub fn stage(&self, key: &str) -> Option<&StageEntry> {
        self.stages.get(key)
    }
}

/// Structural checks: exactly the two stages; every listed rule, closed set and example exists;
/// no stage lists one twice; every rule text is non-empty and used by at least one stage.
fn verify(f: &StageMaterials, root: &Value) -> Result<(), String> {
    if f.note.trim().is_empty() {
        return Err("STAGE_MATERIALS_INVALID: note".into());
    }
    let keys: Vec<&str> = f.stages.keys().map(String::as_str).collect();
    let mut want: Vec<&str> = STAGE_KEYS.to_vec();
    want.sort();
    if keys != want {
        return Err(format!("STAGE_MATERIALS_INVALID: stages {keys:?} != {want:?}"));
    }
    let examples = root.get("examples").and_then(Value::as_object);
    let example_schemas = root.get("exampleSchemas").and_then(Value::as_object);
    for (stage, e) in &f.stages {
        for (what, list) in [("rules", &e.rules), ("closedSets", &e.closed_sets), ("examples", &e.examples)] {
            if list.is_empty() {
                return Err(format!("STAGE_MATERIALS_INVALID: {stage}.{what} empty"));
            }
            let mut sorted = list.clone();
            sorted.sort();
            sorted.dedup();
            if sorted.len() != list.len() {
                return Err(format!("STAGE_MATERIALS_INVALID: {stage}.{what} lists an entry twice"));
            }
        }
        for r in &e.rules {
            if !f.rules.contains_key(r) {
                return Err(format!("STAGE_MATERIALS_INVALID: {stage} names unknown rule {r}"));
            }
        }
        for c in &e.closed_sets {
            let ok = root.get(c).and_then(Value::as_array).is_some_and(|a| !a.is_empty() && a.iter().all(Value::is_string));
            if !ok {
                return Err(format!("STAGE_MATERIALS_INVALID: {stage} names unknown closed set {c}"));
            }
        }
        for x in &e.examples {
            let has = examples.is_some_and(|m| m.contains_key(x)) && example_schemas.is_some_and(|m| m.contains_key(x));
            if !has {
                return Err(format!("STAGE_MATERIALS_INVALID: {stage} names unknown example {x}"));
            }
        }
    }
    for (id, text) in &f.rules {
        if text.trim().is_empty() {
            return Err(format!("STAGE_MATERIALS_INVALID: rule {id} has no text"));
        }
        if !f.stages.values().any(|e| e.rules.contains(id)) {
            return Err(format!("STAGE_MATERIALS_INVALID: rule {id} is used by no stage"));
        }
    }
    Ok(())
}

