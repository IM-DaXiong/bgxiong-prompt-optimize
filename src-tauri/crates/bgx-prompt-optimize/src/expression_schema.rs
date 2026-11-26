//! Closed-set draft JSON Schema for `TextCall.schema` on `run_video_expression`.
//!
//! This is a pure-core port of `projection_schema_view::draft_schema_for`:
//! resolve the official `projectionDraft` contract, then bind the run's closed
//! id sets as enums. It does not depend on `bgx_creator_contract`. The `$ref`
//! inliner follows the same rules as `resolve_schema_document` (siblings of
//! `$ref` go through `allOf` unless they are annotations).
//!
//! Long-term this generator should live in `bgx-video-prompt-contract` so the
//! host and core cannot drift. That move needs D to open the contract crate.

use std::sync::OnceLock;

use bgx_video_prompt_contract::channels::{allowed_source_ids, allows, FrozenNegativeContext};
use bgx_video_prompt_contract::projection::{
    VideoPromptNegativeInput, VideoPromptNegativeOperation,
};
use bgx_video_prompt_contract::ProjectionClosedSet;
use serde_json::{json, Value};

const PROJECTION_CONTRACT_FILE: &str = "video-prompt-projection-draft.schema.json";
const DRAFT_SCHEMA: &str =
    include_str!("../../../../packages/app-contracts/v1/video-prompt-projection-draft.schema.json");
const CHANNELS_SCHEMA: &str =
    include_str!("../../../../packages/app-contracts/v1/video-prompt-channels.schema.json");
const SKILLS_SCHEMA: &str = include_str!("../../../../packages/app-contracts/v1/agent-skills.json");

/// Annotation keywords copied from `bgx_creator_contract::schema_value::ANNOTATION_KEYWORDS`.
const ANNOTATION_KEYWORDS: &[&str] = &[
    "title",
    "description",
    "$comment",
    "examples",
    "default",
    "deprecated",
    "readOnly",
    "writeOnly",
    "$id",
    "$schema",
];

fn draft_schema_document() -> &'static Value {
    static DOC: OnceLock<Value> = OnceLock::new();
    DOC.get_or_init(|| {
        serde_json::from_str(DRAFT_SCHEMA)
            .expect("validated video-prompt-projection-draft.schema.json")
    })
}

fn channels_document() -> &'static Value {
    static DOC: OnceLock<Value> = OnceLock::new();
    DOC.get_or_init(|| {
        serde_json::from_str(CHANNELS_SCHEMA).expect("validated video-prompt-channels.schema.json")
    })
}

fn skills_document() -> &'static Value {
    static DOC: OnceLock<Value> = OnceLock::new();
    DOC.get_or_init(|| serde_json::from_str(SKILLS_SCHEMA).expect("validated agent-skills.json"))
}

struct SchemaDocument<'a> {
    file: &'a str,
    root: &'a Value,
    external: &'a dyn Fn(&str) -> Option<&'a Value>,
}

struct ResolvedRef<'a> {
    file: String,
    root: &'a Value,
    target: &'a Value,
    key: String,
}

fn resolve_ref<'a>(
    reference: &str,
    current_file: &str,
    current_root: &'a Value,
    doc: &SchemaDocument<'a>,
) -> Result<ResolvedRef<'a>, String> {
    let (file, pointer) = reference.split_once('#').unwrap_or((reference, ""));
    let (target_file, root): (String, &'a Value) = if file.is_empty() {
        (current_file.to_string(), current_root)
    } else {
        (
            file.to_string(),
            (doc.external)(file).ok_or_else(|| format!("SCHEMA_REF_FILE_MISSING: {reference}"))?,
        )
    };
    let target = if pointer.is_empty() {
        root
    } else {
        let (section, name) = pointer
            .strip_prefix("/definitions/")
            .map(|name| ("definitions", name))
            .or_else(|| pointer.strip_prefix("/$defs/").map(|name| ("$defs", name)))
            .ok_or_else(|| format!("SCHEMA_REF_UNSUPPORTED: {reference}"))?;
        root.get(section)
            .and_then(|defs| defs.get(name))
            .ok_or_else(|| format!("SCHEMA_REF_TARGET_MISSING: {reference}"))?
    };
    Ok(ResolvedRef {
        key: format!("{target_file}#{pointer}"),
        file: target_file,
        root,
        target,
    })
}

fn resolve_node<'a>(
    schema: &Value,
    current_file: &str,
    current_root: &'a Value,
    doc: &SchemaDocument<'a>,
    stack: &mut Vec<String>,
) -> Result<Value, String> {
    let object = match schema {
        Value::Bool(_) => return Ok(schema.clone()),
        Value::Object(object) => object,
        _ => {
            return Err(format!(
                "SCHEMA_INVALID: {current_file} must be an object or boolean"
            ))
        }
    };

    if let Some(reference) = object.get("$ref") {
        let reference = reference
            .as_str()
            .ok_or_else(|| "SCHEMA_REF_UNSUPPORTED: $ref must be a string".to_string())?;
        let target = resolve_ref(reference, current_file, current_root, doc)?;
        if stack.iter().any(|item| item == &target.key) {
            return Err(format!(
                "SCHEMA_REF_CYCLE: {} -> {}",
                stack.join(" -> "),
                target.key
            ));
        }
        stack.push(target.key.clone());
        let resolved_target = resolve_node(target.target, &target.file, target.root, doc, stack)?;
        stack.pop();

        let mut siblings = object.clone();
        siblings.remove("$ref");
        siblings.remove("definitions");
        siblings.remove("$defs");
        siblings.remove("schemaVersion");
        if siblings.is_empty() {
            return Ok(resolved_target);
        }
        let resolved_siblings = resolve_node(
            &Value::Object(siblings),
            current_file,
            current_root,
            doc,
            stack,
        )?;
        if let (Some(target), Some(notes)) =
            (resolved_target.as_object(), resolved_siblings.as_object())
        {
            if notes
                .keys()
                .all(|key| ANNOTATION_KEYWORDS.contains(&key.as_str()) || key.starts_with("x-"))
            {
                let mut merged = target.clone();
                merged.extend(notes.clone());
                return Ok(Value::Object(merged));
            }
        }
        return Ok(json!({"allOf": [resolved_target, resolved_siblings]}));
    }

    let mut resolved = serde_json::Map::new();
    for (key, value) in object {
        let next = match key.as_str() {
            "definitions" | "$defs" | "schemaVersion" => continue,
            "properties" => {
                let properties = value.as_object().ok_or_else(|| {
                    format!("SCHEMA_PROPERTIES_INVALID: {current_file}/properties")
                })?;
                let mut out = serde_json::Map::new();
                for (name, property) in properties {
                    out.insert(
                        name.clone(),
                        resolve_node(property, current_file, current_root, doc, stack)?,
                    );
                }
                Value::Object(out)
            }
            "allOf" | "anyOf" | "oneOf" => Value::Array(
                value
                    .as_array()
                    .ok_or_else(|| format!("SCHEMA_COMBINATOR_EMPTY: {current_file}/{key}"))?
                    .iter()
                    .map(|branch| resolve_node(branch, current_file, current_root, doc, stack))
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            "items" if value.is_array() => Value::Array(
                value
                    .as_array()
                    .expect("tuple items")
                    .iter()
                    .map(|position| resolve_node(position, current_file, current_root, doc, stack))
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            "not" | "if" | "then" | "else" | "items" => {
                resolve_node(value, current_file, current_root, doc, stack)?
            }
            "additionalProperties" if !value.is_boolean() => {
                resolve_node(value, current_file, current_root, doc, stack)?
            }
            _ => value.clone(),
        };
        resolved.insert(key.clone(), next);
    }
    Ok(Value::Object(resolved))
}

fn resolve_definition(definition: &str) -> Value {
    let doc = draft_schema_document();
    let synthetic = json!({
        "$ref": format!("#/definitions/{definition}"),
        "definitions": doc["definitions"].clone(),
    });
    let channels = channels_document();
    let skills = skills_document();
    let external = |name: &str| -> Option<&Value> {
        if name == "video-prompt-channels.schema.json" {
            Some(channels)
        } else if name == "agent-skills.json" {
            Some(skills)
        } else {
            None
        }
    };
    let document = SchemaDocument {
        file: PROJECTION_CONTRACT_FILE,
        root: &synthetic,
        external: &external,
    };
    let mut stack = Vec::new();
    resolve_node(
        document.root,
        document.file,
        document.root,
        &document,
        &mut stack,
    )
    .unwrap_or_else(|err| panic!("projection contract {definition} does not resolve: {err}"))
}

fn resolved_draft_schema() -> &'static Value {
    static SCHEMA: OnceLock<Value> = OnceLock::new();
    SCHEMA.get_or_init(|| resolve_definition("projectionDraft"))
}

/// Official issue-to-severity relations from the same contract file, applied to
/// `projectionReviewResult` exactly as the host view does.
fn review_severity_relations() -> Value {
    draft_schema_document()["reviewIssueSeverities"]
        .as_object()
        .expect("contract issue/severity relation")
        .iter()
        .map(|(issue, severity)| {
            json!({
                "if": {"required": ["issue"], "properties": {"issue": {"const": issue}}},
                "then": {"properties": {"severity": {"const": severity}}}
            })
        })
        .collect()
}

fn resolved_review_schema() -> &'static Value {
    static SCHEMA: OnceLock<Value> = OnceLock::new();
    SCHEMA.get_or_init(|| {
        let mut schema = resolve_definition("projectionReviewResult");
        schema["properties"]["findings"]["items"]["allOf"] = json!(review_severity_relations());
        schema
    })
}

fn restrict_array(schema: &mut Value, ids: &[String]) {
    if ids.is_empty() {
        schema["maxItems"] = json!(0);
    } else {
        schema["items"]["enum"] = json!(ids);
    }
}

fn negative_already_resolved(context: &FrozenNegativeContext) -> bool {
    context.operation == VideoPromptNegativeOperation::Preserve
        || matches!(
            context.input,
            VideoPromptNegativeInput::Clear {} | VideoPromptNegativeInput::Text { .. }
        )
}

/// Bind closed-set IDs onto the official projectionDraft contract without
/// changing its structure or limits. This is the value that must be sent as
/// `TextCall.schema` so the model is constrained before the reply is parsed.
pub fn draft_schema_for(closed: &ProjectionClosedSet) -> Value {
    let mut schema = resolved_draft_schema().clone();
    let lines = &mut schema["properties"]["lines"]["items"];
    lines["properties"]["shotId"]["enum"] = json!(closed.shot_ids());
    restrict_array(&mut lines["properties"]["factIds"], &closed.fact_ids());
    restrict_array(&mut lines["properties"]["quoteIds"], &closed.quote_ids());
    restrict_array(
        &mut lines["properties"]["referenceUseIds"],
        &closed.reference_use_ids(),
    );
    lines["allOf"] = json!(closed
        .shot_ids()
        .iter()
        .map(|id| {
            let mut props = json!({
                "factIds": {"type":"array", "items":{"type":"string"}},
                "quoteIds": {"type":"array", "items":{"type":"string"}}
            });
            restrict_array(&mut props["factIds"], &closed.shot_fact_ids(id));
            restrict_array(&mut props["quoteIds"], &closed.shot_quote_ids(id));
            json!({
                "if":{"required":["shotId"],"properties":{"shotId":{"const":id}}},
                "then":{"properties":props}
            })
        })
        .collect::<Vec<_>>());
    let conflicts = &mut schema["properties"]["conflicts"]["items"]["properties"];
    restrict_array(&mut conflicts["factIds"], &closed.fact_ids());
    restrict_array(
        &mut conflicts["referenceUseIds"],
        &closed.reference_use_ids(),
    );
    if allows(closed.expansion_policy.as_ref()) {
        let fact_ids = &mut schema["properties"]["lines"]["items"]["properties"]["factIds"];
        fact_ids["items"].as_object_mut().unwrap().remove("enum");
        for relation in schema["properties"]["lines"]["items"]["allOf"]
            .as_array_mut()
            .unwrap()
        {
            relation["then"]["properties"]
                .as_object_mut()
                .unwrap()
                .remove("factIds");
        }
        let proposal = &mut schema["properties"]["proposedFacts"]["items"]["properties"];
        proposal["shotId"]["enum"] = json!(closed.shot_ids());
        proposal["sourceRef"]["enum"] = json!(allowed_source_ids(closed.expansion_policy.as_ref()));
        proposal["kind"].as_object_mut().unwrap().remove("enum");
        proposal["semanticSlot"]
            .as_object_mut()
            .unwrap()
            .remove("enum");
        restrict_array(&mut proposal["entityRefIds"], &closed.entity_ids());
        // Host `mother_fact_relations` is currently empty (writing labels are not IDs).
        schema["properties"]["proposedFacts"]["items"]
            .as_object_mut()
            .expect("projection contract proposed fact is an object")
            .remove("allOf");
    } else {
        schema["properties"]["proposedFacts"]["maxItems"] = json!(0);
    }
    if let Some(context) = &closed.negative_context {
        if negative_already_resolved(context) {
            schema["properties"]
                .as_object_mut()
                .expect("projection draft properties")
                .remove("negative");
        }
    } else {
        schema["properties"]["negative"] = json!({"not":{}});
    }
    let bindings = &mut schema["properties"]["speakerVoiceBindings"];
    let voices = closed.voice_reference_use_ids();
    let speakers = closed.speaker_ids();
    if voices.is_empty() || speakers.is_empty() {
        bindings["maxItems"] = json!(0);
    } else {
        let props = &mut bindings["items"]["properties"];
        props["speakerId"]["enum"] = json!(speakers);
        props["voiceReferenceUseId"]["enum"] = json!(voices);
        let subjects = closed.speaker_subject_ids();
        if subjects.is_empty() {
            props["subjectId"] = json!({"not": {}});
        } else {
            props["subjectId"]["enum"] = json!(subjects);
        }
    }
    schema
}

/// Bind closed-set IDs onto the official `projectionReviewResult` contract without
/// changing its structure. Used as `TextCall.schema` and `outputContract` for
/// `video-expression-review`.
pub fn review_schema_for(closed: &ProjectionClosedSet, line_count: usize) -> Value {
    let mut schema = resolved_review_schema().clone();
    let props = &mut schema["properties"]["findings"]["items"]["properties"];
    props["shotId"]["anyOf"][0]["enum"] = json!(closed.shot_ids());
    props["lineIndex"]["anyOf"][0]["maximum"] = json!(line_count.saturating_sub(1));
    restrict_array(&mut props["factIds"], &closed.fact_ids());
    restrict_array(&mut props["quoteIds"], &closed.quote_ids());
    restrict_array(&mut props["referenceUseIds"], &closed.reference_use_ids());
    let negative_sources = closed
        .negative_context
        .as_ref()
        .map(|context| context.source_ids.clone())
        .unwrap_or_default();
    restrict_array(&mut props["sourceRefs"], &negative_sources);
    schema
}

#[cfg(test)]
mod tests {

    use super::*;
    use bgx_video_prompt_contract::document::DocumentDraftV1;
    use bgx_video_prompt_contract::projection::{
        VideoPromptExpressionLanguage, VideoPromptTargetSnapshot,
    };

    fn closed_mother() -> ProjectionClosedSet {
        let mother: DocumentDraftV1 = serde_json::from_value(json!({"shots":[
            {"shotId":"s1","facts":[{"factId":"f1","kind":"action","text":"She walks.","semanticSlot":"action_interaction","polarity":"positive","provenance":"user_text","sourceFactId":null,"entityRefIds":[],"sourceUnitIds":["u1"]}],"quotes":[{"quoteId":"q1","exactText":"hi","role":"dialogue","speakerId":null,"sourceUnitId":null}]}
        ]}))
        .unwrap();
        let target = VideoPromptTargetSnapshot {
            profile_id: "generic".into(),
            profile_version: 1,
            input_mode: "text_to_video".into(),
            duration_seconds: None,
            caps_digest: "a".repeat(64),
            expression_language: VideoPromptExpressionLanguage::Zh,
            reference_use_ids: vec![],
        };
        ProjectionClosedSet::new(&mother, &target)
    }

    #[test]
    fn draft_schema_binds_closed_set_enums_and_does_not_allow_unknown_facts() {
        let closed = closed_mother();
        let schema = draft_schema_for(&closed);
        let lines = &schema["properties"]["lines"]["items"]["properties"];
        assert_eq!(lines["shotId"]["enum"], json!(["s1"]));
        assert_eq!(lines["factIds"]["items"]["enum"], json!(["f1"]));
        assert_eq!(lines["quoteIds"]["items"]["enum"], json!(["q1"]));
        assert_eq!(lines["referenceUseIds"]["maxItems"], json!(0));
        let fact_enum = lines["factIds"]["items"]["enum"]
            .as_array()
            .cloned()
            .unwrap();
        assert!(!fact_enum.iter().any(|value| value == "not-a-fact"));
        assert_eq!(schema["properties"]["proposedFacts"]["maxItems"], json!(0));
        assert_eq!(schema["properties"]["negative"], json!({"not":{}}));
        assert_eq!(
            schema["properties"]["speakerVoiceBindings"]["maxItems"],
            json!(0)
        );
        let shot_branch = &schema["properties"]["lines"]["items"]["allOf"][0];
        assert_eq!(
            shot_branch["if"]["properties"]["shotId"]["const"],
            json!("s1")
        );
        assert_eq!(
            shot_branch["then"]["properties"]["factIds"]["items"]["enum"],
            json!(["f1"])
        );
        assert!(schema.get("$ref").is_none());
        assert!(schema["properties"]["lines"].is_object());
    }
}
