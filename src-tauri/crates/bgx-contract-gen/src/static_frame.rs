//! Static-frame authoring contracts (PLAN 20261016 SF02-SF07): one generator for build.rs and the
//! `gen-static-frame-contracts` CLI. It validates the cross-document invariants (closed records,
//! rule table vs ruleId enum, policy closed sets vs schema enums) and emits:
//! - the static-frame type graph (authoring / review-edit / severity / policy enums / checkpoint /
//!   candidate review) as Rust + TypeScript,
//! - the Stage B' strict wire graph (Rust only),
//! - policy scalars and the rule table (Rust only).
//!
//! Per-request closed sets (x-bindEnum positions) are bound by the host at runtime; this module
//! never turns a dynamic instance closed set into a second static enum.

use crate::schema_types::{self, GeneratedTypes};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

const CONTRACT_DIR: &str = "packages/app-contracts/v1";
pub const SEVERITY_DOC: &str = "review-severity-v1.json";
pub const STRUCTURE_SCHEMA: &str = "storyboard-render-structure-v1.schema.json";
pub const AUTHORING_SCHEMA: &str = "static-frame-authoring-v1.schema.json";
pub const REVIEW_EDIT_SCHEMA: &str = "static-frame-review-edit-v1.schema.json";
pub const RULES_DOC: &str = "static-frame-review-rules-v1.json";
pub const POLICY_DOC: &str = "static-frame-authoring-policy-v1.json";
pub const CHECKPOINT_SCHEMA: &str = "static-frame-checkpoint-v1.schema.json";
pub const CANDIDATE_REVIEW_SCHEMA: &str = "static-frame-candidate-review-v1.schema.json";

pub const TS_TARGET: &str = "ui/src/domain/static-frame-authoring/contracts.generated.ts";
pub const OUT_STATIC_FRAME_RS: &str = "app_contracts_static_frame.rs";
pub const OUT_STRUCTURE_RS: &str = "app_contracts_storyboard_render_structure.rs";
pub const OUT_POLICY_RS: &str = "app_contracts_static_frame_policy.rs";

const ALL_DOCS: &[&str] = &[
    SEVERITY_DOC,
    STRUCTURE_SCHEMA,
    AUTHORING_SCHEMA,
    REVIEW_EDIT_SCHEMA,
    RULES_DOC,
    POLICY_DOC,
    CHECKPOINT_SCHEMA,
    CANDIDATE_REVIEW_SCHEMA,
];

const STATIC_FRAME_ROOTS: &[(&str, &[&str])] = &[
    (SEVERITY_DOC, &["severity"]),
    (
        AUTHORING_SCHEMA,
        &[
            "subjectRef",
            "auditV1",
            "draftV1",
            "authoringErrorKind",
            "authoringErrorDetailV1",
            "authoringErrorV1",
        ],
    ),
    (
        REVIEW_EDIT_SCHEMA,
        &[
            "ruleId",
            "findingResolution",
            "checkStatus",
            "inputFindingV1",
            "changeV1",
            "finalCheckV1",
            "reviewEditResultV1",
        ],
    ),
    (POLICY_DOC, &["aiTaskStage", "reviewScope", "sourceKind"]),
    (
        CHECKPOINT_SCHEMA,
        &[
            "checkpointPhase",
            "callStatus",
            "resultRefV1",
            "shotIdentityV1",
            "checkpointEntryV1",
            "checkpointManifestV1",
        ],
    ),
    (
        CANDIDATE_REVIEW_SCHEMA,
        &[
            "stillReviewState",
            "reviewAction",
            "padPlanRefV1",
            "shotReviewRecordV1",
            "candidateReviewV1",
            "reviewSummaryV1",
            "progressV1",
            "unknownCallV1",
        ],
    ),
];

const STRUCTURE_ROOTS: &[(&str, &[&str])] = &[(STRUCTURE_SCHEMA, &["output"])];

/// Fields that are host-owned (projected from Stage A) or retired; never accepted on the B' wire.
const STRUCTURE_FORBIDDEN_KEYS: &[&str] = &["finalPrompts", "shotPresenceKind", "framingMirror"];

pub struct StaticFrameGenerated {
    /// Static-frame graph: Rust for OUT_DIR, TypeScript for [`TS_TARGET`].
    pub static_frame: GeneratedTypes,
    /// Stage B' wire graph (Rust consumed; TypeScript not published).
    pub structure: GeneratedTypes,
    /// Policy scalars, rule table and raw contract documents.
    pub policy_rust: String,
}

pub fn load_documents(root: &Path) -> BTreeMap<String, Value> {
    let dir = root.join(CONTRACT_DIR);
    ALL_DOCS
        .iter()
        .map(|name| (name.to_string(), crate::read_contract_json(&dir.join(name))))
        .collect()
}

pub fn generate(root: &Path) -> Result<StaticFrameGenerated, String> {
    generate_for(root, false)
}

/// Host Rust omits only the UI-only progress subgraph and unconsumed policy scalars.
/// The complete TypeScript graph and all source-contract validation remain unchanged.
pub fn generate_host(root: &Path) -> Result<StaticFrameGenerated, String> {
    generate_for(root, true)
}

fn generate_for(root: &Path, host: bool) -> Result<StaticFrameGenerated, String> {
    let docs = load_documents(root);
    let dir = root.join(CONTRACT_DIR);
    let raw = |name: &str| {
        std::fs::read_to_string(dir.join(name))
            .map(|s| s.replace("\r\n", "\n"))
            .map_err(|e| format!("read {name}: {e}"))
    };
    let raw_docs = BTreeMap::from([
        (STRUCTURE_SCHEMA, raw(STRUCTURE_SCHEMA)?),
        (AUTHORING_SCHEMA, raw(AUTHORING_SCHEMA)?),
        (REVIEW_EDIT_SCHEMA, raw(REVIEW_EDIT_SCHEMA)?),
        (RULES_DOC, raw(RULES_DOC)?),
    ]);
    generate_from_for(&docs, &raw_docs, host)
}

pub fn generate_from(
    docs: &BTreeMap<String, Value>,
    raw_docs: &BTreeMap<&str, String>,
) -> Result<StaticFrameGenerated, String> {
    generate_from_for(docs, raw_docs, false)
}

fn generate_from_for(
    docs: &BTreeMap<String, Value>,
    raw_docs: &BTreeMap<&str, String>,
    host: bool,
) -> Result<StaticFrameGenerated, String> {
    validate(docs)?;
    let rust_options = schema_types::RustRepresentationOptions {
        enum_variant_prefixes: BTreeMap::from([
            ("StaticFrameRuleId".into(), "SF-".into()),
            ("StaticFrameAiTaskStage".into(), "storyboard_".into()),
        ]),
        ..Default::default()
    };
    let mut static_frame = schema_types::generate_documents_with_options(
        docs,
        STATIC_FRAME_ROOTS,
        "StaticFrame",
        &rust_options,
    )?;
    if host {
        let host_roots: Vec<_> = STATIC_FRAME_ROOTS
            .iter()
            .map(|(document, definitions)| {
                let kept: Vec<_> = definitions
                    .iter()
                    .copied()
                    .filter(|definition| {
                        !((*document == POLICY_DOC && *definition == "aiTaskStage")
                            || (*document == CANDIDATE_REVIEW_SCHEMA
                                && matches!(*definition, "progressV1" | "unknownCallV1")))
                    })
                    .collect();
                (*document, kept)
            })
            .collect();
        let roots: Vec<_> = host_roots
            .iter()
            .map(|(document, definitions)| (*document, definitions.as_slice()))
            .collect();
        let mut host_options = rust_options.clone();
        host_options
            .enum_variant_prefixes
            .remove("StaticFrameAiTaskStage");
        static_frame.rust = schema_types::generate_documents_with_options(
            docs,
            &roots,
            "StaticFrame",
            &host_options,
        )?
        .rust;
    }
    let structure = schema_types::generate_documents(docs, STRUCTURE_ROOTS, "StageBStructureWire")?;
    let policy_rust = policy_rust(docs, raw_docs, host)?;
    Ok(StaticFrameGenerated {
        static_frame,
        structure,
        policy_rust,
    })
}

fn doc<'a>(docs: &'a BTreeMap<String, Value>, name: &str) -> Result<&'a Value, String> {
    docs.get(name)
        .ok_or_else(|| format!("missing contract {name}"))
}

fn str_enum(docs: &BTreeMap<String, Value>, name: &str, key: &str) -> Result<Vec<String>, String> {
    let values = doc(docs, name)?
        .pointer(&format!("/definitions/{key}/enum"))
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{name}#{key} must be a string enum"))?;
    let out: Vec<String> = values
        .iter()
        .map(|v| {
            v.as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("{name}#{key}: non-string enum value"))
        })
        .collect::<Result<_, _>>()?;
    let unique: BTreeSet<&String> = out.iter().collect();
    if out.is_empty() || unique.len() != out.len() {
        return Err(format!("{name}#{key}: enum must be nonempty and unique"));
    }
    Ok(out)
}

fn nonempty_str<'a>(v: &'a Value, key: &str, ctx: &str) -> Result<&'a str, String> {
    v.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| format!("{ctx}: missing nonempty string {key}"))
}

fn ascii_token(v: &Value, key: &str, ctx: &str) -> Result<String, String> {
    let s = nonempty_str(v, key, ctx)?;
    if s.chars()
        .any(|c| !c.is_ascii() || c.is_ascii_control() || c.is_ascii_whitespace())
    {
        return Err(format!(
            "{ctx}: {key} must be printable ASCII without whitespace"
        ));
    }
    Ok(s.to_owned())
}

fn string_list(v: &Value, key: &str, ctx: &str) -> Result<Vec<String>, String> {
    let arr = v
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{ctx}: missing array {key}"))?;
    let out: Vec<String> = arr
        .iter()
        .map(|x| {
            x.as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("{ctx}: {key} entries must be strings"))
        })
        .collect::<Result<_, _>>()?;
    let unique: BTreeSet<&String> = out.iter().collect();
    if unique.len() != out.len() {
        return Err(format!("{ctx}: {key} entries must be unique"));
    }
    Ok(out)
}

/// Every object schema is a closed record and there is no untyped object hole.
fn check_closed(value: &Value, path: &str, errors: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            let is_schema_object = map.get("type").and_then(Value::as_str) == Some("object")
                || map.contains_key("properties");
            if is_schema_object {
                if map.get("additionalProperties") != Some(&Value::Bool(false)) {
                    errors.push(format!(
                        "{path}: object schema must set additionalProperties=false"
                    ));
                }
                if !map.contains_key("properties") {
                    errors.push(format!(
                        "{path}: object schema without properties is an untyped hole"
                    ));
                }
            }
            for (key, child) in map {
                // Annotation payloads (x-*) and enum/const literals are data, not schemas.
                if key.starts_with("x-") || key == "enum" || key == "const" {
                    continue;
                }
                check_closed(child, &format!("{path}/{key}"), errors);
            }
        }
        Value::Array(items) => {
            for (i, child) in items.iter().enumerate() {
                check_closed(child, &format!("{path}/{i}"), errors);
            }
        }
        _ => {}
    }
}

fn find_key(value: &Value, needle: &str) -> bool {
    match value {
        Value::Object(map) => map.iter().any(|(k, v)| {
            (k == needle && !k.starts_with("x-")) || (!k.starts_with("x-") && find_key(v, needle))
        }),
        Value::Array(items) => items.iter().any(|v| find_key(v, needle)),
        _ => false,
    }
}

pub fn validate(docs: &BTreeMap<String, Value>) -> Result<(), String> {
    for name in ALL_DOCS {
        let d = doc(docs, name)?;
        if d.get("schemaVersion").and_then(Value::as_u64) != Some(1) {
            return Err(format!("{name}: schemaVersion must be 1"));
        }
    }

    // Closed records everywhere in the schema documents.
    let mut errors = Vec::new();
    for name in [
        STRUCTURE_SCHEMA,
        AUTHORING_SCHEMA,
        REVIEW_EDIT_SCHEMA,
        CHECKPOINT_SCHEMA,
        CANDIDATE_REVIEW_SCHEMA,
    ] {
        let defs = doc(docs, name)?
            .get("definitions")
            .ok_or_else(|| format!("{name}: missing definitions"))?;
        check_closed(defs, name, &mut errors);
    }
    if !errors.is_empty() {
        return Err(errors.join("; "));
    }

    // B' wire never carries host-owned or retired fields.
    let structure_defs = &doc(docs, STRUCTURE_SCHEMA)?["definitions"];
    for key in STRUCTURE_FORBIDDEN_KEYS {
        if find_key(structure_defs, key) {
            return Err(format!("{STRUCTURE_SCHEMA}: forbidden wire field {key}"));
        }
    }
    let root_props = structure_defs
        .pointer("/output/properties")
        .and_then(Value::as_object)
        .ok_or("structure output must be an object")?;
    let root_keys: BTreeSet<&str> = root_props.keys().map(String::as_str).collect();
    if root_keys != BTreeSet::from(["shots", "sceneLookLock", "transitions"]) {
        return Err(format!(
            "{STRUCTURE_SCHEMA}: root must be exactly shots/sceneLookLock/transitions"
        ));
    }

    let severity = str_enum(docs, SEVERITY_DOC, "severity")?;
    if severity != ["blocking", "major", "minor"] {
        return Err(format!(
            "{SEVERITY_DOC}: severity must be blocking|major|minor"
        ));
    }
    let gate = doc(docs, SEVERITY_DOC)?
        .get("gateSemantics")
        .and_then(Value::as_object)
        .ok_or("severity gateSemantics missing")?;
    for s in &severity {
        let blocks = gate
            .get(s)
            .and_then(|g| g.get("blocksGate"))
            .and_then(Value::as_bool)
            .ok_or_else(|| format!("severity gateSemantics.{s}.blocksGate missing"))?;
        if blocks != (s == "blocking") {
            return Err(format!("severity {s}: only blocking may block a gate"));
        }
    }
    if gate.len() != severity.len() {
        return Err("severity gateSemantics has unknown keys".into());
    }

    // Rules: unique, complete, same order as the schema ruleId enum.
    let rule_ids = str_enum(docs, REVIEW_EDIT_SCHEMA, "ruleId")?;
    let rules_doc = doc(docs, RULES_DOC)?;
    let evidence_kinds: BTreeSet<String> = string_list(rules_doc, "evidenceKinds", RULES_DOC)?
        .into_iter()
        .collect();
    let rules = rules_doc
        .get("rules")
        .and_then(Value::as_array)
        .ok_or("rules must be an array")?;
    let mut seen = Vec::new();
    for rule in rules {
        let id = ascii_token(rule, "id", RULES_DOC)?;
        let ctx = format!("{RULES_DOC}#{id}");
        rule.get("canBlock")
            .and_then(Value::as_bool)
            .ok_or_else(|| format!("{ctx}: canBlock must be boolean"))?;
        for key in [
            "titleZh",
            "titleEn",
            "descriptionZh",
            "descriptionEn",
            "revisionBoundary",
        ] {
            nonempty_str(rule, key, &ctx)?;
        }
        for kind in string_list(rule, "evidence", &ctx)? {
            if !evidence_kinds.contains(&kind) {
                return Err(format!("{ctx}: unknown evidence kind {kind}"));
            }
        }
        seen.push(id);
    }
    if seen != rule_ids {
        return Err(format!(
            "{RULES_DOC} ids must equal {REVIEW_EDIT_SCHEMA}#ruleId in order (got {seen:?})"
        ));
    }
    if !rules.iter().any(|r| r["canBlock"] == Value::Bool(false)) {
        return Err(format!(
            "{RULES_DOC}: at least the craft rule must be non-blocking"
        ));
    }
    let final_checks = doc(docs, REVIEW_EDIT_SCHEMA)?
        .pointer("/definitions/finalCheckList")
        .ok_or("finalCheckList missing")?;
    let n = rule_ids.len() as u64;
    if final_checks.get("minItems").and_then(Value::as_u64) != Some(n)
        || final_checks.get("maxItems").and_then(Value::as_u64) != Some(n)
    {
        return Err(format!(
            "finalCheckList must hold exactly {n} checks (one per rule)"
        ));
    }

    // Policy closed sets equal their schema enums; no repair rounds.
    let policy = doc(docs, POLICY_DOC)?;
    if string_list(policy, "authoringErrorKinds", POLICY_DOC)?
        != str_enum(docs, AUTHORING_SCHEMA, "authoringErrorKind")?
    {
        return Err("policy authoringErrorKinds must equal authoringErrorKind enum".into());
    }
    if policy.get("formatRepairAttempts").and_then(Value::as_u64) != Some(0) {
        return Err("policy formatRepairAttempts must be 0 for B'/C/D".into());
    }
    ascii_token(policy, "checkpointPayloadKey", POLICY_DOC)?;
    let metadata_kind = ascii_token(policy, "candidateMetadataKind", POLICY_DOC)?;
    if doc(docs, CANDIDATE_REVIEW_SCHEMA)?
        .pointer("/definitions/candidateReviewV1/properties/metadataKind/const")
        .and_then(Value::as_str)
        != Some(metadata_kind.as_str())
    {
        return Err("candidate metadataKind const must equal policy candidateMetadataKind".into());
    }
    let stage_enum = str_enum(docs, POLICY_DOC, "aiTaskStage")?;
    let stages = policy
        .get("stages")
        .and_then(Value::as_object)
        .ok_or("policy stages missing")?;
    let stage_keys: Vec<&str> = stages.keys().map(String::as_str).collect();
    if stage_keys != ["authoring", "reviewEdit", "structure"] {
        return Err("policy stages must be exactly structure/authoring/reviewEdit".into());
    }
    let mut stage_values = Vec::new();
    for (key, stage) in stages {
        let ctx = format!("{POLICY_DOC}#stages.{key}");
        stage_values.push(ascii_token(stage, "aiTaskStage", &ctx)?);
        let t = stage
            .get("temperature")
            .and_then(Value::as_f64)
            .ok_or_else(|| format!("{ctx}: temperature must be a number"))?;
        if !(0.0..=2.0).contains(&t) {
            return Err(format!("{ctx}: temperature out of range"));
        }
        let schema = nonempty_str(stage, "schema", &ctx)?;
        if !ALL_DOCS.contains(&schema) {
            return Err(format!("{ctx}: unknown schema {schema}"));
        }
    }
    let a: BTreeSet<&String> = stage_values.iter().collect();
    let b: BTreeSet<&String> = stage_enum.iter().collect();
    if a != b || a.len() != stage_values.len() {
        return Err("policy stages must map one-to-one onto aiTaskStage enum".into());
    }
    let versions = policy
        .get("schemaVersions")
        .and_then(Value::as_object)
        .ok_or("policy schemaVersions missing")?;
    if versions.is_empty() || versions.values().any(|v| v.as_u64() != Some(1)) {
        return Err("policy schemaVersions must all be 1".into());
    }
    let scope_enum = str_enum(docs, POLICY_DOC, "reviewScope")?;
    let scopes = policy
        .get("reviewScopes")
        .and_then(Value::as_object)
        .ok_or("policy reviewScopes missing")?;
    let scope_keys: BTreeSet<&String> = scopes.keys().collect();
    let scope_enum_set: BTreeSet<&String> = scope_enum.iter().collect();
    if scope_keys != scope_enum_set {
        return Err("policy reviewScopes keys must equal reviewScope enum".into());
    }
    for (key, scope) in scopes {
        let ctx = format!("{POLICY_DOC}#reviewScopes.{key}");
        let materials = string_list(scope, "qMaterials", &ctx)?;
        let ops = string_list(scope, "patchOps", &ctx)?;
        if materials.is_empty() || ops.is_empty() {
            return Err(format!("{ctx}: qMaterials/patchOps must be nonempty"));
        }
        if key == "upstream_only"
            && (ops.iter().any(|o| o == "stillPrompt")
                || materials.iter().any(|m| m == "stillPrompts"))
        {
            return Err(format!(
                "{ctx}: upstream_only must not expose still prompts to Q/P"
            ));
        }
    }
    Ok(())
}

fn lit(s: &str) -> String {
    format!("{s:?}")
}

fn policy_rust(
    docs: &BTreeMap<String, Value>,
    raw_docs: &BTreeMap<&str, String>,
    host: bool,
) -> Result<String, String> {
    let policy = doc(docs, POLICY_DOC)?;
    let stage = |key: &str| -> Result<(String, f64), String> {
        let s = &policy["stages"][key];
        Ok((
            s["aiTaskStage"].as_str().ok_or("stage name")?.to_owned(),
            s["temperature"].as_f64().ok_or("stage temperature")?,
        ))
    };
    let (structure_stage, structure_t) = stage("structure")?;
    let (authoring_stage, authoring_t) = stage("authoring")?;
    let (review_stage, review_t) = stage("reviewEdit")?;
    let raw = |name: &str| -> Result<String, String> {
        raw_docs
            .get(name)
            .map(|s| lit(s))
            .ok_or_else(|| format!("raw document {name} missing"))
    };
    let mut out = String::from("// @generated by bgx-contract-gen static_frame. Do not edit.\n\n");
    out.push_str(&format!(
        "pub const STORYBOARD_RENDER_STRUCTURE_SCHEMA_JSON: &str = {};\n\
         pub const STATIC_FRAME_AUTHORING_SCHEMA_JSON: &str = {};\n\
         pub const STATIC_FRAME_REVIEW_EDIT_SCHEMA_JSON: &str = {};\n\
         pub const STATIC_FRAME_REVIEW_RULES_JSON: &str = {};\n\
         pub const STATIC_FRAME_STAGE_STRUCTURE: &str = {};\n\
         pub const STATIC_FRAME_STAGE_AUTHORING: &str = {};\n\
         pub const STATIC_FRAME_STAGE_REVIEW_EDIT: &str = {};\n\
         {}\
         pub const STATIC_FRAME_AUTHORING_TEMPERATURE: f64 = {:?};\n\
         pub const STATIC_FRAME_REVIEW_EDIT_TEMPERATURE: f64 = {:?};\n\
         pub const STATIC_FRAME_CHECKPOINT_PAYLOAD_KEY: &str = {};\n\
         pub const STATIC_FRAME_CANDIDATE_METADATA_KIND: &str = {};\n\
         {}\n",
        raw(STRUCTURE_SCHEMA)?,
        raw(AUTHORING_SCHEMA)?,
        raw(REVIEW_EDIT_SCHEMA)?,
        raw(RULES_DOC)?,
        lit(&structure_stage),
        lit(&authoring_stage),
        lit(&review_stage),
        if host {
            String::new()
        } else {
            format!("pub const STATIC_FRAME_STRUCTURE_TEMPERATURE: f64 = {structure_t:?};\n")
        },
        authoring_t,
        review_t,
        lit(policy["checkpointPayloadKey"]
            .as_str()
            .ok_or("checkpointPayloadKey")?),
        lit(policy["candidateMetadataKind"]
            .as_str()
            .ok_or("candidateMetadataKind")?),
        if host {
            String::new()
        } else {
            format!(
                "pub const STATIC_FRAME_FORMAT_REPAIR_ATTEMPTS: u32 = {};\n",
                policy["formatRepairAttempts"]
                    .as_u64()
                    .ok_or("formatRepairAttempts")?
            )
        },
    ));
    out.push_str(
        "#[derive(Clone, Copy, Debug, PartialEq, Eq)]\npub struct StaticFrameRuleSpec {\n    pub id: &'static str,\n    pub can_block: bool,\n    pub title_zh: &'static str,\n    pub title_en: &'static str,\n    pub description_zh: &'static str,\n    pub description_en: &'static str,\n    pub revision_boundary: &'static str,\n}\n\n",
    );
    out.push_str("pub const STATIC_FRAME_REVIEW_RULES: &[StaticFrameRuleSpec] = &[\n");
    for rule in doc(docs, RULES_DOC)?["rules"].as_array().ok_or("rules")? {
        let s = |k: &str| lit(rule[k].as_str().unwrap_or_default());
        out.push_str(&format!(
            "    StaticFrameRuleSpec {{ id: {}, can_block: {}, title_zh: {}, title_en: {}, description_zh: {}, description_en: {}, revision_boundary: {} }},\n",
            s("id"),
            rule["canBlock"].as_bool().unwrap_or(false),
            s("titleZh"),
            s("titleEn"),
            s("descriptionZh"),
            s("descriptionEn"),
            s("revisionBoundary"),
        ));
    }
    out.push_str("];\n\n");
    out.push_str(
        "#[derive(Clone, Copy, Debug, PartialEq, Eq)]\npub struct StaticFrameReviewScopeSpec {\n    pub scope: &'static str,\n    pub q_materials: &'static [&'static str],\n    pub patch_ops: &'static [&'static str],\n}\n\n",
    );
    out.push_str("pub const STATIC_FRAME_REVIEW_SCOPES: &[StaticFrameReviewScopeSpec] = &[\n");
    for (scope, spec) in policy["reviewScopes"].as_object().ok_or("reviewScopes")? {
        let list = |k: &str| -> String {
            let items: Vec<String> = spec[k]
                .as_array()
                .map(|a| a.iter().filter_map(Value::as_str).map(lit).collect())
                .unwrap_or_default();
            format!("&[{}]", items.join(", "))
        };
        out.push_str(&format!(
            "    StaticFrameReviewScopeSpec {{ scope: {}, q_materials: {}, patch_ops: {} }},\n",
            lit(scope),
            list("qMaterials"),
            list("patchOps"),
        ));
    }
    out.push_str("];\n");
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
    }

    fn docs() -> BTreeMap<String, Value> {
        load_documents(&root())
    }

    fn raw_docs() -> BTreeMap<&'static str, String> {
        BTreeMap::from([
            (STRUCTURE_SCHEMA, String::new()),
            (AUTHORING_SCHEMA, String::new()),
            (REVIEW_EDIT_SCHEMA, String::new()),
            (RULES_DOC, String::new()),
        ])
    }

    #[test]
    fn generation_is_deterministic_and_ts_twin_is_current() {
        let first = generate(&root()).expect("valid static-frame contracts");
        let second = generate(&root()).expect("valid static-frame contracts");
        assert_eq!(first.static_frame.rust, second.static_frame.rust);
        assert_eq!(
            first.static_frame.typescript,
            second.static_frame.typescript
        );
        assert_eq!(first.structure.rust, second.structure.rust);
        assert_eq!(first.policy_rust, second.policy_rust);
        let host = generate_host(&root()).expect("host projection");
        assert_eq!(host.static_frame.typescript, first.static_frame.typescript);
        assert_eq!(host.structure.rust, first.structure.rust);
        for declaration in [
            "pub enum StaticFrameAiTaskStage",
            "pub struct StaticFrameProgressV1",
            "pub struct StaticFrameUnknownCallV1",
        ] {
            assert!(first.static_frame.rust.contains(declaration));
            assert!(!host.static_frame.rust.contains(declaration));
        }
        for name in [
            "STATIC_FRAME_STRUCTURE_TEMPERATURE",
            "STATIC_FRAME_FORMAT_REPAIR_ATTEMPTS",
        ] {
            assert!(first.policy_rust.contains(name));
            assert!(!host.policy_rust.contains(name));
        }
        // Remaining generated declarations retain exact field/serde/closed-record contracts.
        let declaration_blocks =
            |source: &str| source.split("\n\n").map(str::to_owned).collect::<Vec<_>>();
        let full_blocks = declaration_blocks(&first.static_frame.rust);
        for block in declaration_blocks(&host.static_frame.rust) {
            assert!(full_blocks.contains(&block));
        }
        assert_eq!(
            full_blocks.len(),
            declaration_blocks(&host.static_frame.rust).len() + 3
        );
        let full_policy_lines: Vec<_> = first
            .policy_rust
            .lines()
            .filter(|line| {
                !line.starts_with("pub const STATIC_FRAME_STRUCTURE_TEMPERATURE:")
                    && !line.starts_with("pub const STATIC_FRAME_FORMAT_REPAIR_ATTEMPTS:")
            })
            .collect();
        assert_eq!(
            host.policy_rust.lines().collect::<Vec<_>>(),
            full_policy_lines
        );
        let ts = std::fs::read_to_string(root().join(TS_TARGET))
            .expect("generated TS present")
            .replace("\r\n", "\n");
        assert_eq!(
            ts, first.static_frame.typescript,
            "run gen-static-frame-contracts --write"
        );
    }

    #[test]
    fn generated_shapes_cover_tagged_unions_and_closed_records() {
        let g = generate_from(&docs(), &raw_docs()).unwrap();
        let rust = &g.static_frame.rust;
        assert!(rust.contains("#[serde(tag = \"action\""));
        assert!(rust.contains("#[serde(tag = \"kind\""));
        assert!(rust.contains("pub struct StaticFrameDraftV1"));
        assert!(rust.contains("pub enum StaticFrameReviewEditResultV1"));
        assert!(rust.contains("pub struct StaticFrameCheckpointManifestV1"));
        assert!(!rust.contains("staticFramePrompts"));
        assert!(g
            .structure
            .rust
            .contains("pub struct StageBStructureWireOutput"));
        assert!(!g.structure.rust.contains("final_prompts"));
        assert!(!g.structure.rust.contains("shot_presence_kind"));
        assert!(!g.structure.rust.contains("framing_mirror"));
        assert_eq!(
            g.policy_rust.matches("StaticFrameRuleSpec { id:").count(),
            12
        );
        assert!(g
            .policy_rust
            .contains("STATIC_FRAME_FORMAT_REPAIR_ATTEMPTS: u32 = 0;"));
    }

    #[test]
    fn invalid_contracts_are_rejected() {
        let base = docs();
        let mutate = |f: &dyn Fn(&mut BTreeMap<String, Value>)| {
            let mut d = base.clone();
            f(&mut d);
            generate_from(&d, &raw_docs()).is_err()
                && generate_from_for(&d, &raw_docs(), true).is_err()
        };
        // Open record (unknown keys accepted) is refused.
        assert!(mutate(&|d| {
            d.get_mut(AUTHORING_SCHEMA).unwrap()["definitions"]["draftV1"]
                .as_object_mut()
                .unwrap()
                .remove("additionalProperties");
        }));
        // Retired finalPrompts on the B' wire is refused.
        assert!(mutate(&|d| {
            d.get_mut(STRUCTURE_SCHEMA).unwrap()["definitions"]["output"]["properties"]
                ["finalPrompts"] = serde_json::json!({"type":"array","items":{"type":"string"}});
        }));
        // Host-owned shotPresenceKind on the wire is refused.
        assert!(mutate(&|d| {
            d.get_mut(STRUCTURE_SCHEMA).unwrap()["definitions"]["shot"]["properties"]
                ["shotPresenceKind"] = serde_json::json!({"type":"string"});
        }));
        // Rule table and ruleId enum diverge.
        assert!(mutate(&|d| {
            d.get_mut(RULES_DOC).unwrap()["rules"]
                .as_array_mut()
                .unwrap()
                .pop();
        }));
        // Duplicate rule.
        assert!(mutate(&|d| {
            let rules = d.get_mut(RULES_DOC).unwrap()["rules"]
                .as_array_mut()
                .unwrap();
            rules[1] = rules[0].clone();
        }));
        // finalChecks count not bound to the rule count.
        assert!(mutate(&|d| {
            d.get_mut(REVIEW_EDIT_SCHEMA).unwrap()["definitions"]["finalCheckList"]["minItems"] =
                serde_json::json!(11);
        }));
        // A repair round sneaks back in.
        assert!(mutate(&|d| {
            d.get_mut(POLICY_DOC).unwrap()["formatRepairAttempts"] = serde_json::json!(1);
        }));
        // Error-kind closed set diverges between policy and schema.
        assert!(mutate(&|d| {
            d.get_mut(POLICY_DOC).unwrap()["authoringErrorKinds"]
                .as_array_mut()
                .unwrap()
                .pop();
        }));
        // upstream_only must not let Q/P see or patch still prompts.
        assert!(mutate(&|d| {
            d.get_mut(POLICY_DOC).unwrap()["reviewScopes"]["upstream_only"]["patchOps"]
                .as_array_mut()
                .unwrap()
                .push(serde_json::json!("stillPrompt"));
        }));
        // Only blocking may block a gate.
        assert!(mutate(&|d| {
            d.get_mut(SEVERITY_DOC).unwrap()["gateSemantics"]["major"]["blocksGate"] =
                serde_json::json!(true);
        }));
        // Union without a shared discriminator.
        assert!(mutate(&|d| {
            d.get_mut(REVIEW_EDIT_SCHEMA).unwrap()["definitions"]["reviewEditResultV1"]["oneOf"][0]
                ["properties"]
                .as_object_mut()
                .unwrap()
                .remove("action");
        }));
        // Candidate metadata kind diverges from policy.
        assert!(mutate(&|d| {
            d.get_mut(POLICY_DOC).unwrap()["candidateMetadataKind"] = serde_json::json!("other");
        }));
    }
}
