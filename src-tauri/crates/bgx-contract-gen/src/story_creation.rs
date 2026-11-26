//! One schema graph for the story creation pipeline. build.rs and the offline twin writer share this.
//! Closed value sets live only in the schema enums; policy and skill-host maps are keyed by those
//! enums and verified for exact set equality, so no second hand-written list can drift.
use crate::schema_types::{self, GeneratedTypes};
use serde_json::Value;
use std::{collections::BTreeMap, collections::BTreeSet, path::Path};

pub const SCHEMAS: &[&str] = &[
    "story-creation.schema.json",
    "story-creation-model.schema.json",
    "story-creation-content.schema.json",
    "clip-workflow.schema.json",
];
pub const POLICY: &str = "story-creation-policy.json";
pub const SKILL_HOSTS: &str = "story-creation-skill-hosts.json";
pub const AGENT_HOSTS: &str = "agent-skill-hosts.json";
pub const OUT_RS: &str = "app_contracts_story_creation.rs";
pub const TS_TARGET: &str = "ui/src/api/generated/storyCreation.ts";
pub const CLIP_TS_TARGET: &str = "ui/src/api/generated/clipWorkflow.ts";

/// Clip types are part of the same schema graph, re-exported without duplicating definitions.
pub fn clip_typescript(v1: &Path) -> Result<String, String> {
    let defs = read_json(&v1.join("clip-workflow.schema.json"))?;
    let mut names: Vec<_> = defs["definitions"].as_object().ok_or("clip definitions missing")?
        .keys().cloned().collect();
    names.sort();
    let exports = names.iter().map(|name| format!("  StoryCreation{name} as {name},"))
        .collect::<Vec<_>>().join("\n");
    Ok(format!("// Generated from clip-workflow.schema.json; do not edit.\nexport type {{\n{exports}\n}} from './storyCreation';\n"))
}

fn normalize(node: &mut Value) {
    match node {
        Value::Object(map) => {
            if let Some(Value::String(reference)) = map.get_mut("$ref") {
                if let Some((_, fragment)) = reference.split_once('#') {
                    *reference = format!("#{fragment}");
                }
            }
            for value in map.values_mut() {
                normalize(value);
            }
        }
        Value::Array(values) => {
            for value in values {
                normalize(value);
            }
        }
        _ => {}
    }
}

fn read_json(path: &Path) -> Result<Value, String> {
    println!("cargo:rerun-if-changed={}", path.display());
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut value: Value =
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    value.sort_all_objects();
    Ok(value)
}

fn enum_values(
    definitions: &serde_json::Map<String, Value>,
    name: &str,
) -> Result<BTreeSet<String>, String> {
    definitions
        .get(name)
        .and_then(|d| d.get("enum"))
        .and_then(Value::as_array)
        .ok_or_else(|| format!("missing string enum definition {name}"))?
        .iter()
        .map(|v| {
            v.as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("{name}: non-string enum value"))
        })
        .collect()
}

fn same_set(
    what: &str,
    actual: BTreeSet<String>,
    expected: &BTreeSet<String>,
) -> Result<(), String> {
    if &actual != expected {
        let missing: Vec<_> = expected.difference(&actual).collect();
        let extra: Vec<_> = actual.difference(expected).collect();
        return Err(format!(
            "{what}: closed set drift, missing {missing:?}, unexpected {extra:?}"
        ));
    }
    Ok(())
}

fn string_lit(s: &str) -> Result<String, String> {
    serde_json::to_string(s).map_err(|e| e.to_string())
}

/// Merged and ref-normalised definitions (all schema documents); also used by the host for
/// runtime structural validation of the same contract.
pub fn merged_definitions(v1: &Path) -> Result<serde_json::Map<String, Value>, String> {
    let mut definitions = serde_json::Map::new();
    for name in SCHEMAS {
        let value = read_json(&v1.join(name))?;
        if value["schemaVersion"] != 1 {
            return Err(format!("{name}: unsupported schemaVersion"));
        }
        for (key, schema) in value
            .get("definitions")
            .and_then(Value::as_object)
            .ok_or_else(|| format!("{name}: missing definitions"))?
        {
            if definitions.insert(key.clone(), schema.clone()).is_some() {
                return Err(format!("duplicate story creation definition {key}"));
            }
        }
    }
    let mut holder = Value::Object(definitions);
    normalize(&mut holder);
    holder.sort_all_objects();
    match holder {
        Value::Object(map) => Ok(map),
        _ => Err("definitions holder".into()),
    }
}

pub fn definitions_json(v1: &Path) -> Result<String, String> {
    let definitions = merged_definitions(v1)?;
    serde_json::to_string(&definitions).map_err(|e| e.to_string())
}

pub fn generate(v1: &Path) -> Result<GeneratedTypes, String> {
    let definitions = merged_definitions(v1)?;
    let names: Vec<String> = definitions.keys().cloned().collect();
    let stages = enum_values(&definitions, "Stage")?;
    let errors = enum_values(&definitions, "ErrorCode")?;
    let phases = enum_values(&definitions, "Phase")?;
    let document = serde_json::json!({"definitions": definitions});
    let docs = BTreeMap::from([("story-creation.merged.json".to_string(), document)]);
    let roots: Vec<&str> = names.iter().map(String::as_str).collect();
    let rust_options = schema_types::RustRepresentationOptions {
        enum_variant_prefixes: BTreeMap::from([("StoryCreationErrorCode".into(), "SC_".into())]),
        // Physical Rust only: shrink large_enum_variant without changing JSON/TS wire.
        // Case field keys come from emit_union case_name + schema property name.
        boxed_fields: BTreeSet::from([
            (
                "StoryCreationBatchRequestScenes".into(),
                "continueStoryboard".into(),
            ),
            (
                "StoryCreationClipArtifactClipPlan".into(),
                "source".into(),
            ),
            (
                "StoryCreationClipArtifactFramePlan".into(),
                "source".into(),
            ),
        ]),
    };
    let mut generated = schema_types::generate_with_options(
        &docs,
        "story-creation.merged.json",
        &roots,
        "StoryCreation",
        &rust_options,
    )?;

    // Merged, normalised definitions for runtime structural validation (one source for the host).
    let merged = serde_json::to_string(&definitions).map_err(|e| e.to_string())?;
    generated.rust.push_str(&format!(
        "pub const STORY_CREATION_DEFINITIONS_JSON: &str = {};\n",
        string_lit(&merged)?
    ));

    // Policy: keyed by the schema enums, never a second list.
    let policy = read_json(&v1.join(POLICY))?;
    if policy["schemaVersion"] != 1 {
        return Err(format!("{POLICY}: unsupported version"));
    }
    let policy_errors = policy["errors"]
        .as_object()
        .ok_or("policy: missing errors")?;
    same_set(
        "policy.errors vs ErrorCode",
        policy_errors.keys().cloned().collect(),
        &errors,
    )?;
    for (code, entry) in policy_errors {
        if entry["automaticModelRetry"] != false
            || entry["class"].as_str().is_none_or(str::is_empty)
        {
            return Err(format!(
                "policy.errors.{code}: automaticModelRetry must be false and class set"
            ));
        }
    }
    let retries = policy["retry"]["automaticRetriesPerItem"]
        .as_u64()
        .ok_or("policy: automaticRetriesPerItem")?;
    let backoff: Vec<u64> = policy["retry"]["backoffSeconds"]
        .as_array()
        .ok_or("policy: backoffSeconds")?
        .iter()
        .map(|v| {
            v.as_u64()
                .filter(|s| *s > 0)
                .ok_or("policy: backoffSeconds must be positive integers")
        })
        .collect::<Result<_, _>>()?;
    if backoff.len() as u64 != retries || backoff.windows(2).any(|w| w[0] > w[1]) {
        return Err(
            "policy: backoffSeconds must have one non-decreasing entry per automatic retry".into(),
        );
    }
    // User rule 2026-10-14 (supersedes SPEC 8.2 "repairRounds = 0"): a refused reply goes back to
    // the model with its field-level rejection, at most 5 repair rounds.
    let repair_rounds = policy["retry"]["repairRounds"]
        .as_u64()
        .filter(|n| (1..=5).contains(n))
        .ok_or("policy: repairRounds must be an integer in 1..=5")?;
    if policy["retry"]["honorRetryAfter"] != true {
        return Err("policy: honorRetryAfter must be true".into());
    }
    if policy["retry"]["retryableDisposition"] != "retrySameRequest" {
        return Err("policy: retryableDisposition must be retrySameRequest".into());
    }
    let in_flight = policy["scheduler"]["maxInFlight"]
        .as_u64()
        .filter(|n| *n >= 1)
        .ok_or("policy: maxInFlight")?;
    let default_page = policy["pagination"]["defaultPageSize"]
        .as_u64()
        .filter(|n| *n >= 1)
        .ok_or("policy: defaultPageSize")?;
    let max_page = policy["pagination"]["maxPageSize"]
        .as_u64()
        .filter(|n| *n >= default_page)
        .ok_or("policy: maxPageSize")?;
    let event = policy["events"]["runChanged"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("policy: events.runChanged")?;
    let _ = phases; // phases are schema-only; policy carries no per-phase data yet.
    let compact = serde_json::to_string(&policy).map_err(|e| e.to_string())?;
    let backoff_csv = backoff
        .iter()
        .map(u64::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    generated.rust.push_str(&format!(
        "pub const STORY_CREATION_POLICY_JSON: &str = {};\npub const STORY_CREATION_AUTOMATIC_RETRIES_PER_ITEM: u32 = {retries};\npub const STORY_CREATION_RETRY_BACKOFF_SECONDS: &[u64] = &[{backoff_csv}];\npub const STORY_CREATION_REPAIR_ROUNDS_PER_ITEM: u32 = {repair_rounds};\npub const STORY_CREATION_MAX_IN_FLIGHT: usize = {in_flight};\npub const STORY_CREATION_DEFAULT_PAGE_SIZE: u32 = {default_page};\npub const STORY_CREATION_MAX_PAGE_SIZE: u32 = {max_page};\npub const STORY_CREATION_EVENT_RUN_CHANGED: &str = {};\n",
        string_lit(&compact)?,
        string_lit(event)?
    ));
    generated.typescript.push_str(&format!(
        "export const STORY_CREATION_POLICY = {compact} as const;\nexport const STORY_CREATION_AUTOMATIC_RETRIES_PER_ITEM = {retries};\nexport const STORY_CREATION_RETRY_BACKOFF_SECONDS = [{backoff_csv}] as const;\nexport const STORY_CREATION_REPAIR_ROUNDS_PER_ITEM = {repair_rounds};\nexport const STORY_CREATION_MAX_IN_FLIGHT = {in_flight};\nexport const STORY_CREATION_DEFAULT_PAGE_SIZE = {default_page};\nexport const STORY_CREATION_MAX_PAGE_SIZE = {max_page};\nexport const STORY_CREATION_EVENT_RUN_CHANGED = {};\n",
        string_lit(event)?
    ));

    // Skill purposes: one stage -> one registered host id; map keyed by the Stage enum.
    let hosts = read_json(&v1.join(SKILL_HOSTS))?;
    if hosts["schemaVersion"] != 1 {
        return Err(format!("{SKILL_HOSTS}: unsupported version"));
    }
    let stage_map = hosts["stages"]
        .as_object()
        .ok_or("skill hosts: missing stages")?;
    same_set(
        "skill-hosts.stages vs Stage",
        stage_map.keys().cloned().collect(),
        &stages,
    )?;
    let registered: BTreeSet<String> = read_json(&v1.join(AGENT_HOSTS))?["hosts"]
        .as_array()
        .ok_or("agent-skill-hosts: missing hosts")?
        .iter()
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect();
    let mut seen = BTreeSet::new();
    let mut rust_pairs = Vec::new();
    for (stage, host) in stage_map {
        let host = host
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or("skill host must be a string")?;
        if !registered.contains(host) {
            return Err(format!(
                "skill host {host} for stage {stage} is not registered in {AGENT_HOSTS}"
            ));
        }
        if !seen.insert(host.to_owned()) {
            return Err(format!("skill host {host} is used by two stages"));
        }
        rust_pairs.push(format!("({}, {})", string_lit(stage)?, string_lit(host)?));
    }
    generated.rust.push_str(&format!(
        "pub const STORY_CREATION_SKILL_HOSTS: &[(&str, &str)] = &[{}];\n",
        rust_pairs.join(", ")
    ));
    generated.typescript.push_str(&format!(
        "export const STORY_CREATION_SKILL_HOSTS = {} as const;\n",
        serde_json::to_string(stage_map).map_err(|e| e.to_string())?
    ));
    Ok(generated)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v1() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../packages/app-contracts/v1")
    }

    #[test]
    fn real_contract_generates_closed_records_and_tagged_unions() {
        let generated = generate(&v1()).expect("story creation contracts generate");
        assert!(generated
            .rust
            .contains("pub enum StoryCreationSourceTarget"));
        assert!(generated.rust.contains("#[serde(tag = \"op\""));
        assert!(generated.rust.contains("#[serde(tag = \"mode\""));
        assert!(generated.rust.contains("#[serde(tag = \"operation\""));
        assert!(generated.rust.contains("#[serde(tag = \"stage\""));
        assert!(generated
            .typescript
            .contains("export interface StoryCreationStageSettings"));
        assert!(generated
            .rust
            .contains("continue_storyboard: Option<Box<StoryCreationContinueStoryboard>>"));
        assert!(generated.rust.contains(
            "source: Box<StoryCreationClipSourceSnapshot>"
        ));
        assert!(generated.rust.contains(
            "source: Box<StoryCreationClipFrameSource>"
        ));
        assert!(generated
            .rust
            .contains("#[serde(rename = \"SC_SOURCE_CONFLICT\")]\n    SourceConflict,"));
        let definitions = merged_definitions(&v1()).unwrap();
        let mut expected_open = BTreeSet::new();
        for (definition, schema) in &definitions {
            if schema["type"] != "object" {
                continue;
            }
            let name = format!("StoryCreation{definition}");
            let declaration = format!("pub struct {name} {{");
            let Some(record) = generated.rust.split("\n\n")
                .find(|block| block.contains(&declaration)) else { continue };
            if schema["additionalProperties"] == true {
                expected_open.insert(name.clone());
                assert!(!record.contains("deny_unknown_fields"), "{name}: authorized extras must survive");
                assert!(record.contains("#[serde(flatten)]\n    pub additional_fields: std::collections::BTreeMap<String, serde_json::Value>,"),
                    "{name}: preserve unknown values through serde flatten, not ignore them");
                let ts = generated.typescript.split("\n\n")
                    .find(|block| block.contains(&format!("export interface {name} {{")))
                    .expect("open record has a TypeScript twin");
                assert!(ts.contains("[key: string]: unknown;"), "{name}: TS must retain extensions");
            } else {
                assert_eq!(schema["additionalProperties"], false, "{name}: explicit closure required");
                assert!(record.contains("#[serde(deny_unknown_fields)]"), "{name}: closed records must reject unknown fields");
                assert!(!record.contains("additional_fields:"), "{name}: closed record cannot gain an extension map");
            }
        }
        assert!(!expected_open.is_empty(), "exercise the real open clip contracts");
        assert_eq!(definitions["ModelRequest"]["properties"]["taskContext"], serde_json::json!({}));
        let mut seen_open = BTreeSet::new();
        let mut seen_task_context = false;
        let mut owner = "";
        for line in generated.rust.lines() {
            if let Some(name) = line.strip_prefix("pub struct ").and_then(|rest| rest.strip_suffix(" {")) {
                owner = name;
            }
            if !line.contains("serde_json::Value") { continue; }
            if line.trim() == "pub additional_fields: std::collections::BTreeMap<String, serde_json::Value>," {
                assert!(expected_open.contains(owner), "untyped extension on unauthorized owner {owner}");
                assert!(seen_open.insert(owner.to_owned()), "duplicate extension map on {owner}");
            } else {
                assert_eq!(owner, "StoryCreationModelRequest", "untyped core field: {line}");
                assert_eq!(line.trim(), "pub task_context: Option<serde_json::Value>,", "untyped core field");
                assert!(!seen_task_context, "duplicate task context");
                seen_task_context = true;
            }
        }
        assert_eq!(seen_open, expected_open);
        assert!(seen_task_context);
    }

    #[test]
    fn policy_must_cover_every_error_code() {
        let defs = merged_definitions(&v1()).unwrap();
        let errors = enum_values(&defs, "ErrorCode").unwrap();
        let mut short = errors.clone();
        short.remove("SC_CANCELLED");
        assert!(same_set("t", short, &errors).is_err());
    }
}
