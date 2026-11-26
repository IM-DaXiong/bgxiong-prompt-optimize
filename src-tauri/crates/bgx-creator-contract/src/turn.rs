//! CT05: turn contract parsing, `$refSchema`/`enumRef` resolution, budget bindings, review rule
//! closure and full example verification (moved from `creator_tools::contract`; the build script,
//! the runtime and the CLI call this single implementation).

use crate::diagnostics::ContractViolation;
use crate::TURN_CONTRACT_FILE;
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TurnBudgets {
    pub max_attachments_per_message: u32,
    pub max_document_original_bytes: u64,
    pub max_document_original_bytes_per_message: u64,
    pub max_extracted_utf8_bytes: u64,
    pub max_extracted_bytes_per_turn: u64,
    pub page_bytes: u64,
    pub max_request_goals: u32,
    pub max_request_decisions: u32,
    pub max_goal_refs_per_call: u32,
    pub max_evidence_refs_per_binding: u32,
    pub max_reference_catalog_entries: u32,
    pub max_reference_id_chars: u32,
    pub max_goal_description_chars: u32,
    pub max_request_progress_utf8_bytes: u64,
    pub structured_output_attempts: u32,
    pub max_request_page_items: u32,
    pub max_conversation_input_utf8_bytes: u64,
}

/// RC77: `reviewRuleDefinitions.<ruleId>`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewRuleDefinition {
    pub allowed_severities: Vec<String>,
    pub checks: String,
}

/// Presentation metadata is validated by the same parser at build time and runtime.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TurnProgressContract {
    schema_version: u32,
    required: Vec<String>,
    optional: Vec<String>,
    reason_codes: Vec<String>,
    identity: String,
    merge: String,
    snapshot: String,
    result: String,
    legacy: String,
}

pub(crate) struct ContractFile {
    pub(crate) schema_version: u32,
    pub(crate) budgets: TurnBudgets,
    pub(crate) closed_sets: BTreeMap<String, Vec<String>>,
    pub(crate) schemas: Value,
    pub(crate) examples: Value,
    pub(crate) example_schemas: BTreeMap<String, String>,
    pub(crate) turn_stage_event: String,
    turn_progress: TurnProgressContract,
    pub(crate) review_rule_definitions: BTreeMap<String, ReviewRuleDefinition>,
    pub(crate) stage_materials: crate::stage_materials::StageMaterials,
}

fn parse(raw: &str) -> Result<ContractFile, String> {
    let root: Value = serde_json::from_str(raw).map_err(|e| format!("CONTRACT_PARSE: {e}"))?;
    let obj = root
        .as_object()
        .ok_or_else(|| "CONTRACT_PARSE: root must be an object".to_string())?;
    let schema_version = obj
        .get("schemaVersion")
        .and_then(Value::as_u64)
        .ok_or_else(|| "CONTRACT_PARSE: schemaVersion".to_string())? as u32;
    let budgets: TurnBudgets = serde_json::from_value(
        obj.get("budgets")
            .cloned()
            .ok_or_else(|| "CONTRACT_PARSE: budgets".to_string())?,
    )
    .map_err(|e| format!("CONTRACT_PARSE: budgets {e}"))?;
    let mut closed_sets = BTreeMap::new();
    for (k, v) in obj {
        if let Value::Array(items) = v {
            let set: Option<Vec<String>> =
                items.iter().map(|x| x.as_str().map(str::to_string)).collect();
            let set = set.ok_or_else(|| format!("CONTRACT_PARSE: {k} must be a string array"))?;
            closed_sets.insert(k.clone(), set);
        }
    }
    let example_schemas: BTreeMap<String, String> = serde_json::from_value(
        obj.get("exampleSchemas")
            .cloned()
            .ok_or_else(|| "CONTRACT_PARSE: exampleSchemas".to_string())?,
    )
    .map_err(|e| format!("CONTRACT_PARSE: exampleSchemas {e}"))?;
    let turn_stage_event = obj
        .get("turnStageEvent")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| "CONTRACT_PARSE: turnStageEvent".to_string())?
        .to_string();
    let turn_progress: TurnProgressContract = serde_json::from_value(
        obj.get("turnProgress").cloned().ok_or("CONTRACT_PARSE: turnProgress missing")?,
    ).map_err(|error| format!("CONTRACT_PARSE: turnProgress {error}"))?;
    let review_rule_definitions: BTreeMap<String, ReviewRuleDefinition> = serde_json::from_value(
        obj.get("reviewRuleDefinitions")
            .cloned()
            .ok_or_else(|| "CONTRACT_PARSE: reviewRuleDefinitions".to_string())?,
    )
    .map_err(|e| format!("CONTRACT_PARSE: reviewRuleDefinitions {e}"))?;
    let stage_materials = crate::stage_materials::StageMaterials::parse_and_verify(&root)?;
    Ok(ContractFile {
        schema_version,
        budgets,
        closed_sets,
        schemas: obj.get("schemas").cloned().unwrap_or(Value::Null),
        examples: obj.get("examples").cloned().unwrap_or(Value::Null),
        example_schemas,
        turn_stage_event,
        turn_progress,
        review_rule_definitions,
        stage_materials,
    })
}

fn resolve_node(parsed: &ContractFile, node: &Value, stack: &mut Vec<String>) -> Result<Value, String> {
    match node {
        Value::Object(map) => {
            let mut base = serde_json::Map::new();
            if let Some(name) = map.get("$refSchema") {
                let name = name
                    .as_str()
                    .ok_or_else(|| "SCHEMA_REF: $refSchema must be a string".to_string())?;
                if stack.iter().any(|s| s == name) {
                    return Err(format!("SCHEMA_REF_CYCLE: {} -> {name}", stack.join(" -> ")));
                }
                let target = parsed
                    .schemas
                    .get(name)
                    .ok_or_else(|| format!("SCHEMA_REF_MISSING: {name}"))?;
                stack.push(name.to_string());
                let resolved = resolve_node(parsed, target, stack)?;
                stack.pop();
                if let Value::Object(r) = resolved {
                    base = r;
                }
            }
            for (key, value) in map {
                match key.as_str() {
                    "$refSchema" => {}
                    "enumRef" => {
                        let set_name = value
                            .as_str()
                            .ok_or_else(|| "SCHEMA_ENUM_REF: enumRef must be a string".to_string())?;
                        let set = parsed
                            .closed_sets
                            .get(set_name)
                            .ok_or_else(|| format!("SCHEMA_ENUM_REF_MISSING: {set_name}"))?;
                        base.insert("enum".into(), serde_json::json!(set));
                    }
                    _ => {
                        base.insert(key.clone(), resolve_node(parsed, value, stack)?);
                    }
                }
            }
            Ok(Value::Object(base))
        }
        Value::Array(items) => Ok(Value::Array(
            items
                .iter()
                .map(|v| resolve_node(parsed, v, stack))
                .collect::<Result<Vec<_>, _>>()?,
        )),
        other => Ok(other.clone()),
    }
}

fn resolve_in(parsed: &ContractFile, name: &str) -> Result<Value, String> {
    let root = parsed
        .schemas
        .get(name)
        .ok_or_else(|| format!("SCHEMA_REF_MISSING: {name}"))?;
    let mut stack = vec![name.to_string()];
    let out = resolve_node(parsed, root, &mut stack)?;
    if contains_custom_key(&out) {
        return Err(format!("SCHEMA_CUSTOM_KEY_LEFT: {name}"));
    }
    Ok(out)
}

fn contains_custom_key(v: &Value) -> bool {
    match v {
        Value::Object(m) => {
            m.contains_key("$refSchema") || m.contains_key("enumRef") || m.values().any(contains_custom_key)
        }
        Value::Array(a) => a.iter().any(contains_custom_key),
        _ => false,
    }
}

/// The only contract layout this Host reads.
const SUPPORTED_SCHEMA_VERSION: u32 = 1;

fn schema_limit(parsed: &ContractFile, path: &[&str], key: &str) -> Result<u64, String> {
    let mut node = &parsed.schemas;
    for p in path {
        node = node
            .get(*p)
            .ok_or_else(|| format!("budget binding: schemas/{} missing", path.join("/")))?;
    }
    node.get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("budget binding: schemas/{}/{key} missing", path.join("/")))
}

/// Every schema limit that mirrors a budget must equal it (one number, no drift).
fn verify_budget_bindings(parsed: &ContractFile) -> Result<(), String> {
    let b = &parsed.budgets;
    let ref_id = u64::from(b.max_reference_id_chars);
    let evidence = u64::from(b.max_evidence_refs_per_binding);
    let goal_text = u64::from(b.max_goal_description_chars);
    let checks: Vec<(&[&str], &str, u64)> = vec![
        (&["requestCheckpointArgs", "properties", "goals"], "maxItems", u64::from(b.max_request_goals)),
        (&["requestCheckpointArgs", "properties", "decisions"], "maxItems", u64::from(b.max_request_decisions)),
        (&["creatorCallContextV4", "properties", "goalRefs"], "maxItems", u64::from(b.max_goal_refs_per_call)),
        (&["goalCoverageEntry", "properties", "evidenceRefs"], "maxItems", evidence),
        (&["requestEvidenceReceipt", "properties", "resourceRefs"], "maxItems", evidence),
        (&["requestEvidenceReceipt", "properties", "observationRefs"], "maxItems", evidence),
        (&["referenceCatalog"], "maxItems", u64::from(b.max_reference_catalog_entries)),
        (&["referencePlan", "properties", "refId"], "maxLength", ref_id),
        (&["referenceResource", "properties", "refId"], "maxLength", ref_id),
        (&["referenceObservation", "properties", "refId"], "maxLength", ref_id),
        (&["referenceExplanation", "properties", "refId"], "maxLength", ref_id),
        (&["referenceTarget", "properties", "refId"], "maxLength", ref_id),
        (&["referenceTask", "properties", "refId"], "maxLength", ref_id),
        (&["referenceEffect", "properties", "refId"], "maxLength", ref_id),
        (&["referenceAnswer", "properties", "refId"], "maxLength", ref_id),
        (&["questionOption", "properties", "refId"], "maxLength", ref_id),
        (&["requestGoalDecl", "properties", "subjectRefs", "items"], "maxLength", ref_id),
        (&["requestGoalDecl", "properties", "subjectRefs"], "maxItems", u64::from(b.max_goal_refs_per_call)),
        (&["publishedQuestionV2", "properties", "blockingGoalRefs"], "maxItems", u64::from(b.max_goal_refs_per_call)),
        (&["questionCandidateSelectTarget", "properties", "options"], "maxItems", u64::from(b.max_reference_catalog_entries)),
        (&["workFinding", "properties", "evidenceRefs"], "maxItems", evidence),
        (&["reviewFinding", "properties", "evidenceRefs"], "maxItems", evidence),
        (&["questionFinding", "properties", "evidenceRefs"], "maxItems", evidence),
        (&["workReviewOutput", "properties", "goalCoverage"], "maxItems", u64::from(b.max_request_goals)),
        (&["questionCandidateSelectTarget", "properties", "prompt"], "maxLength", goal_text),
        (&["questionCandidateProvideText", "properties", "prompt"], "maxLength", goal_text),
        (&["continuationProvideText", "properties", "answerText"], "maxLength", goal_text),
        (&["requestGoalDecl", "properties", "desiredOutcome"], "maxLength", goal_text),
        (&["requestGoal", "properties", "desiredOutcome"], "maxLength", goal_text),
        (&["requestPageGoals", "properties", "items"], "maxItems", u64::from(b.max_request_page_items)),
        (&["requestPageCompletionProofs", "properties", "items"], "maxItems", u64::from(b.max_request_page_items)),
        (&["requestPageReviewMaterials", "properties", "items"], "maxItems", u64::from(b.max_request_page_items)),
        (&["requestPageDeliverables", "properties", "items"], "maxItems", u64::from(b.max_request_page_items)),
    ];
    for (path, key, want) in checks {
        let got = schema_limit(parsed, path, key)?;
        if got != want {
            return Err(format!(
                "budget drift: schemas/{}/{key} = {got}, budgets = {want}",
                path.join("/")
            ));
        }
    }
    Ok(())
}

/// RC77/CP05: every review rule has exactly one definition, its severities come from
/// `reviewSeverities` and always include `none`. `reviewRules` is exactly the ordered disjoint
/// union of `workReviewRules`, `replyReviewRules` and `questionReviewRules`, and each domain's
/// findings array holds exactly one entry per rule of that domain (derived, never a second number).
fn verify_review_rules(parsed: &ContractFile) -> Result<(), String> {
    let rules = parsed
        .closed_sets
        .get("reviewRules")
        .ok_or_else(|| "reviewRules missing".to_string())?;
    let severities = parsed
        .closed_sets
        .get("reviewSeverities")
        .ok_or_else(|| "reviewSeverities missing".to_string())?;
    for rule in rules {
        let def = parsed
            .review_rule_definitions
            .get(rule)
            .ok_or_else(|| format!("REVIEW_RULE_UNDEFINED: {rule}"))?;
        if def.checks.trim().is_empty() {
            return Err(format!("REVIEW_RULE_CHECKS_EMPTY: {rule}"));
        }
        if !def.allowed_severities.iter().any(|s| s == "none") {
            return Err(format!("REVIEW_RULE_WITHOUT_NONE: {rule}"));
        }
        for s in &def.allowed_severities {
            if !severities.contains(s) {
                return Err(format!("REVIEW_RULE_SEVERITY_UNKNOWN: {rule} {s}"));
            }
        }
    }
    for name in parsed.review_rule_definitions.keys() {
        if !rules.contains(name) {
            return Err(format!("REVIEW_RULE_DEFINITION_ORPHAN: {name}"));
        }
    }
    let mut union: Vec<String> = Vec::new();
    for (set, findings) in [
        ("workReviewRules", "workFindings"),
        ("replyReviewRules", "replyFindings"),
        ("questionReviewRules", "questionFindings"),
    ] {
        let domain = parsed
            .closed_sets
            .get(set)
            .ok_or_else(|| format!("{set} missing"))?;
        if domain.is_empty() {
            return Err(format!("REVIEW_DOMAIN_EMPTY: {set}"));
        }
        for r in domain {
            if union.contains(r) {
                return Err(format!("REVIEW_RULE_DOMAIN_OVERLAP: {r}"));
            }
            union.push(r.clone());
        }
        let want = domain.len() as u64;
        for key in ["minItems", "maxItems"] {
            let got = schema_limit(parsed, &[findings], key)?;
            if got != want {
                return Err(format!("review rule drift: schemas/{findings}/{key} = {got}, {set} = {want}"));
            }
        }
    }
    if &union != rules {
        return Err("REVIEW_RULE_DOMAIN_DRIFT: reviewRules != work + reply + question".to_string());
    }
    Ok(())
}

fn verify_turn_progress(parsed: &ContractFile) -> Result<(), String> {
    let progress = &parsed.turn_progress;
    let fail = |reason: &str| format!("turnProgress: {reason}");
    if progress.schema_version != 2 { return Err(fail("schemaVersion must be 2")); }
    for (field, text) in [("identity", &progress.identity), ("merge", &progress.merge),
        ("snapshot", &progress.snapshot), ("result", &progress.result), ("legacy", &progress.legacy)] {
        if text.trim().is_empty() { return Err(fail(&format!("{field} must be nonempty"))); }
    }
    let required = ["schemaVersion", "hostIdempotencyKey", "sessionId", "seq", "occurredAtMs", "stage"];
    let optional = ["requestRef", "toolId", "toolCallId", "resultRef", "reasonCode"];
    if progress.required != required || progress.optional != optional {
        return Err(fail("required/optional fields must match version 2 execution identity and structured references"));
    }
    let reasons: std::collections::BTreeSet<_> = progress.reason_codes.iter().collect();
    if reasons.is_empty() || reasons.len() != progress.reason_codes.len()
        || reasons.iter().any(|reason|reason.trim().is_empty()) {
        return Err(fail("reasonCodes must be nonempty unique strings"));
    }
    // Resolve by name exactly as contract::validate_named does; a missing or broken
    // schema cannot hide behind metadata that production would otherwise ignore.
    let schema = resolve_in(parsed, "turnProgressEventV2")?;
    if schema["type"] != "object" || schema["additionalProperties"] != false
        || schema["required"] != serde_json::json!(progress.required) {
        return Err(fail("turnProgressEventV2 must be closed with the same required fields"));
    }
    let properties = schema["properties"].as_object().ok_or_else(||fail("event properties missing"))?;
    let expected: std::collections::BTreeSet<_> = required.into_iter().chain(optional).collect();
    if properties.keys().map(String::as_str).collect::<std::collections::BTreeSet<_>>() != expected {
        return Err(fail("event properties differ from required + optional fields"));
    }
    if properties["schemaVersion"] != serde_json::json!({"const":progress.schema_version})
        || properties["seq"] != serde_json::json!({"type":"integer","minimum":1})
        || properties["occurredAtMs"] != serde_json::json!({"type":"integer","minimum":0}) {
        return Err(fail("event schemaVersion/seq/occurredAtMs constraints drifted"));
    }
    if properties["stage"]["type"] != "string"
        || properties["stage"]["enum"] != serde_json::json!(parsed.closed_sets.get("turnStages").ok_or_else(||fail("turnStages missing"))?) {
        return Err(fail("event stage must use turnStages"));
    }
    for field in ["hostIdempotencyKey", "sessionId", "requestRef", "toolId", "toolCallId", "resultRef", "reasonCode"] {
        if properties[field] != serde_json::json!({"type":"string","minLength":1}) {
            return Err(fail(&format!("event {field} must be a nonempty string")));
        }
    }
    Ok(())
}

fn verify_contract(parsed: &ContractFile) -> Result<(), String> {
    verify_budget_bindings(parsed)?;
    verify_review_rules(parsed)?;
    verify_turn_progress(parsed)?;
    if parsed.schema_version != SUPPORTED_SCHEMA_VERSION {
        return Err(format!(
            "schemaVersion {} is not supported (expected {SUPPORTED_SCHEMA_VERSION})",
            parsed.schema_version
        ));
    }
    let schemas = parsed
        .schemas
        .as_object()
        .ok_or_else(|| "schemas object required".to_string())?;
    for name in schemas.keys() {
        let resolved = resolve_in(parsed, name)?;
        crate::schema_value::verify_schema_keywords(&resolved, &format!("schemas/{name}"))?;
    }
    let examples = parsed
        .examples
        .as_object()
        .ok_or_else(|| "examples object required".to_string())?;
    for key in examples.keys() {
        if !parsed.example_schemas.contains_key(key) {
            return Err(format!("EXAMPLE_SCHEMA_MISSING: {key}"));
        }
    }
    for example in parsed.example_schemas.keys() {
        if !examples.contains_key(example) {
            return Err(format!("EXAMPLE_MISSING: {example}"));
        }
    }
    Ok(())
}

/// Every example against its mapped schema, typed (instance path, keyword, branch reasons).
fn verify_examples(parsed: &ContractFile) -> Result<(), ContractViolation> {
    let examples = parsed.examples.as_object();
    for (example, schema) in &parsed.example_schemas {
        let value = examples
            .and_then(|m| m.get(example))
            .ok_or_else(|| violation(example, format!("EXAMPLE_MISSING: {example}")))?;
        let resolved = resolve_in(parsed, schema).map_err(|e| violation(schema, e))?;
        crate::schema_value::check_value(&resolved, value, example).map_err(|e| {
            ContractViolation::new(TURN_CONTRACT_FILE, schema, "EXAMPLE_INVALID: ").with_value_error(e)
        })?;
    }
    Ok(())
}


/// The verified turn contract (C02). Construct only through [`CreatorTurnContract::parse_and_verify`].
pub struct CreatorTurnContract {
    file: ContractFile,
}

fn violation(schema_ref: &str, e: String) -> ContractViolation {
    ContractViolation::new(TURN_CONTRACT_FILE, schema_ref, e)
}

impl CreatorTurnContract {
    /// Parse, resolve every schema, check budget bindings, review rules, stage materials and
    /// every example against its schema. Nothing is defaulted or skipped.
    pub fn parse_and_verify(raw: &str) -> Result<Self, ContractViolation> {
        let file = parse(raw).map_err(|e| violation("", e))?;
        verify_contract(&file).map_err(|e| violation("", e))?;
        verify_examples(&file)?;
        Ok(Self { file })
    }
    pub fn schema_version(&self) -> u32 {
        self.file.schema_version
    }
    pub fn budgets(&self) -> &TurnBudgets {
        &self.file.budgets
    }
    pub fn closed_set(&self, name: &str) -> Option<&[String]> {
        self.file.closed_sets.get(name).map(Vec::as_slice)
    }
    pub fn schemas(&self) -> &Value {
        &self.file.schemas
    }
    pub fn schema_named(&self, name: &str) -> Option<&Value> {
        self.file.schemas.get(name)
    }
    pub fn examples(&self) -> &Value {
        &self.file.examples
    }
    pub fn example_schemas(&self) -> &BTreeMap<String, String> {
        &self.file.example_schemas
    }
    pub fn turn_stage_event(&self) -> &str {
        &self.file.turn_stage_event
    }
    pub fn turn_progress_schema_version(&self) -> u32 {
        self.file.turn_progress.schema_version
    }
    pub fn turn_progress_reason_codes(&self) -> &[String] {
        &self.file.turn_progress.reason_codes
    }
    pub fn review_rule_definition(&self, rule_id: &str) -> Option<&ReviewRuleDefinition> {
        self.file.review_rule_definitions.get(rule_id)
    }
    pub fn stage_materials(&self) -> &crate::stage_materials::StageMaterials {
        &self.file.stage_materials
    }
    /// Plain JSON Schema for a contract schema name.
    pub fn resolve_schema(&self, name: &str) -> Result<Value, ContractViolation> {
        resolve_in(&self.file, name).map_err(|e| violation(name, e))
    }
    /// Validate a value against a named contract schema.
    pub fn validate_named(&self, name: &str, value: &Value) -> Result<(), ContractViolation> {
        let schema = self.resolve_schema(name)?;
        crate::schema_value::validate_schema_value(TURN_CONTRACT_FILE, name, &schema, value, name)
    }
    /// (examples, exampleSchemas mappings, schemas) ? reported, never hard-coded (V03).
    pub fn counts(&self) -> (usize, usize, usize) {
        (
            self.file.examples.as_object().map_or(0, |m| m.len()),
            self.file.example_schemas.len(),
            self.file.schemas.as_object().map_or(0, |m| m.len()),
        )
    }
}
