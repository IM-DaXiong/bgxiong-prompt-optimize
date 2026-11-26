//! CT02/CT07/CT09/CT30: fast negative set for the pure verifier (no Tauri, no DB).

use bgx_creator_contract::{
    schema_value::{check_value, verify_schema_keywords},
    CreatorCatalog, CreatorTurnContract,
};
use serde_json::{json, Value};
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..").join("..")
}

fn turn_raw() -> String {
    std::fs::read_to_string(repo_root().join("packages/app-contracts/v1/creator-agent-turn-contract.json"))
        .expect("turn contract readable in the repository")
}

fn turn_value() -> Value {
    serde_json::from_str(&turn_raw()).expect("turn contract JSON")
}

fn effect_policy_value() -> Value {
    serde_json::from_str(
        &std::fs::read_to_string(repo_root().join("packages/app-contracts/v1/creator-effect-policy.json"))
            .expect("effect policy readable"),
    ).expect("effect policy JSON")
}

#[test]
fn effect_policy_current_registry_verifies() {
    let policy = effect_policy_value();
    assert!(!policy["entries"].as_array().unwrap().is_empty());
    bgx_creator_contract::effect_policy::verify(&policy).unwrap();
}

#[test]
fn canvas_configuration_schema_requires_the_exact_owner_for_operation_input() {
    let path = repo_root().join("packages/app-contracts/v1/canvas-configuration-change.schema.json");
    let schema: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    verify_schema_keywords(&schema, "canvas.configuration.change.prepare").unwrap();
    let choices = schema["properties"]["actions"]["items"]["properties"]["request"]
        ["properties"]["changes"]["items"]["oneOf"].as_array().unwrap();
    let parameters = choices.iter()
        .find(|branch| branch["properties"]["type"]["const"] == "parameters.patch")
        .unwrap();
    let mut change = json!({"type":"parameters.patch","operationInput":{"nested":{"unknownToHost":[false,0,null]}}});
    assert!(check_value(parameters, &change, "change").is_err());
    change["adapterSelection"] = json!({
        "pluginId":"local.fixture","version":"1.0.0","modelId":"fixture",
        "capabilityKey":"video.generate","operationId":"local.fixture/generate",
        "operationVersion":1,"inputSchemaDigest":"a".repeat(64)
    });
    check_value(parameters, &change, "change").unwrap();
}

#[test]
fn effect_policy_rejects_wrong_replay_for_every_constrained_entry() {
    let policy = effect_policy_value();
    let mut checked = 0;
    for (index, entry) in policy["entries"].as_array().unwrap().iter().enumerate() {
        if matches!(entry["receiptKind"].as_str(),
            Some("uiDraft" | "planCancel" | "taskControl" | "refreshDelivery"))
        {
            continue;
        }
        let mut invalid = policy.clone();
        invalid["entries"][index]["replaySemantics"] = if entry["replaySemantics"] == "readCurrent" {
            json!("idempotentReceipt")
        } else {
            json!("readCurrent")
        };
        let error = bgx_creator_contract::effect_policy::verify(&invalid).unwrap_err();
        assert!(error.contains(entry["toolId"].as_str().unwrap()), "{error}");
        assert!(error.contains("replays as"), "{error}");
        checked += 1;
    }
    assert!(checked > 0);
}

#[test]
fn effect_policy_rejects_invalid_transaction_scope_and_classification() {
    let policy = effect_policy_value();
    let entries = policy["entries"].as_array().unwrap();
    let read = entries.iter().position(|e| e["receiptKind"] == "observation").unwrap();
    let mutation = entries.iter().position(|e| e["receiptKind"] == "mutation").unwrap();
    let classified = entries.iter().position(|e| e["resultClassification"].is_object()).unwrap();
    for (index, field, value) in [
        (read, "transactionGrade", json!("singleDbTransaction")),
        (mutation, "transactionGrade", json!("none")),
        (mutation, "refreshRequirement", json!("none")),
        (mutation, "scopeMapper", json!("none")),
        (read, "receiptMapper", json!("   ")),
        (read, "outcomeStates", json!([])),
        (classified, "resultClassification", json!({"pointer":"/status","outcomes":{"undeclared":"observation"}})),
    ] {
        let mut invalid = policy.clone();
        invalid["entries"][index][field] = value;
        assert!(bgx_creator_contract::effect_policy::verify(&invalid).is_err(), "{field}");
    }
    let mut duplicate = policy.clone();
    duplicate["entries"].as_array_mut().unwrap().push(entries[read].clone());
    assert!(bgx_creator_contract::effect_policy::verify(&duplicate).unwrap_err().contains("duplicates"));
}

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/request-projection-missing-authored-content.json"))
        .expect("fixture JSON")
}

#[test]
fn current_turn_contract_verifies_and_reports_counts() {
    let c = CreatorTurnContract::parse_and_verify(&turn_raw()).expect("current contract passes");
    let (examples, mappings, schemas) = c.counts();
    assert!(examples > 0 && mappings == examples && schemas > 0);
}

#[test]
fn incident_example_without_authored_content_is_rejected_with_structure() {
    let fx = fixture();
    let mut v = turn_value();
    v["examples"]["requestProjection"] = fx["value"].clone();
    let err = CreatorTurnContract::parse_and_verify(&v.to_string())
        .err()
        .expect("the incident example must be rejected");
    assert_eq!(err.code, "CREATOR_CONTRACT_INVALID");
    assert!(err.message.starts_with(fx["expectedCode"].as_str().unwrap()), "{err}");
    assert_eq!(err.location.instance_path, fx["expectedLegacyInstancePath"].as_str().unwrap(), "{err}");
    assert_eq!(err.location.schema_path, "oneOf", "{err}");
    let leaf = format!("{}: missing", fx["expectedInstancePath"].as_str().unwrap());
    assert!(err.location.branch_reasons.iter().any(|b| b.ends_with(&leaf)), "{err}");
}

#[test]
fn corrected_example_with_empty_authored_content_passes() {
    let fx = fixture();
    let mut v = turn_value();
    let mut fixed = fx["value"].clone();
    fixed["explanation"]["acceptedReply"]["authoredContent"] = json!([]);
    v["examples"]["requestProjection"] = fixed;
    CreatorTurnContract::parse_and_verify(&v.to_string()).expect("corrected example passes");
}

#[test]
fn missing_mapping_orphan_mapping_and_missing_ref_are_rejected() {
    let mut v = turn_value();
    v["examples"]["unmappedExample"] = json!({});
    let e = CreatorTurnContract::parse_and_verify(&v.to_string()).err().expect("unmapped example");
    assert!(e.message.contains("EXAMPLE_SCHEMA_MISSING"), "{e}");

    let mut v = turn_value();
    v["exampleSchemas"]["orphanExample"] = json!("requestProjectionV1");
    let e = CreatorTurnContract::parse_and_verify(&v.to_string()).err().expect("orphan mapping");
    assert!(e.message.contains("EXAMPLE_MISSING"), "{e}");

    let mut v = turn_value();
    v["schemas"]["brokenRef"] = json!({"$refSchema": "doesNotExist"});
    let e = CreatorTurnContract::parse_and_verify(&v.to_string()).err().expect("missing ref");
    assert!(e.message.contains("SCHEMA_REF_MISSING"), "{e}");

    let mut v = turn_value();
    v["schemas"]["cycA"] = json!({"$refSchema": "cycB"});
    v["schemas"]["cycB"] = json!({"$refSchema": "cycA"});
    let e = CreatorTurnContract::parse_and_verify(&v.to_string()).err().expect("cycle");
    assert!(e.message.contains("SCHEMA_REF_CYCLE"), "{e}");
}

#[test]
fn budget_drift_and_unknown_budget_field_are_rejected() {
    let mut v = turn_value();
    let n = v["budgets"]["maxRequestGoals"].as_u64().unwrap();
    v["budgets"]["maxRequestGoals"] = json!(n + 1);
    let e = CreatorTurnContract::parse_and_verify(&v.to_string()).err().expect("drift");
    assert!(e.message.contains("budget drift"), "{e}");

    let mut v = turn_value();
    v["budgets"]["unknownBudget"] = json!(1);
    let e = CreatorTurnContract::parse_and_verify(&v.to_string()).err().expect("unknown field");
    assert!(e.message.contains("CONTRACT_PARSE: budgets"), "{e}");
}

#[test]
fn stage_material_errors_are_rejected() {
    let mut v = turn_value();
    let stage = v["stageMaterials"]["stages"].as_object().unwrap().keys().next().unwrap().clone();
    v["stageMaterials"]["stages"][&stage]["examples"] = json!([]);
    let e = CreatorTurnContract::parse_and_verify(&v.to_string()).err().expect("empty examples");
    assert!(e.message.contains("STAGE_MATERIALS_INVALID"), "{e}");
}

#[test]
fn unknown_type_name_and_empty_combinator_are_rejected() {
    assert!(check_value(&json!({"type": "strng"}), &json!("x"), "v").is_err());
    assert!(verify_schema_keywords(&json!({"type": "strng"}), "s").is_err());
    assert!(verify_schema_keywords(&json!({"oneOf": []}), "s").is_err());
    assert!(verify_schema_keywords(&json!({"typo": 1}), "s").is_err());
    assert!(verify_schema_keywords(&json!({"type": "string", "pattern": "^(?!x)", "x-note": 1}), "s").is_ok());
}

#[test]
fn closed_object_and_enum_semantics_are_kept() {
    let s = json!({"type": "object", "additionalProperties": false, "required": ["a"],
        "properties": {"a": {"enum": ["x"]}}});
    assert!(check_value(&s, &json!({"a": "x"}), "v").is_ok());
    assert!(check_value(&s, &json!({"a": "y"}), "v").is_err());
    assert!(check_value(&s, &json!({"a": "x", "b": 1}), "v").is_err());
    assert!(check_value(&s, &json!({}), "v").is_err());
}

#[test]
fn catalog_current_passes_and_collisions_fail() {
    let raw = std::fs::read_to_string(repo_root().join("packages/app-contracts/v1/creator-tool-catalog.json"))
        .expect("catalog readable");
    let c = CreatorCatalog::parse_and_verify(&raw).expect("current catalog passes");
    assert!(!c.tools().is_empty());
    let mut v: Value = serde_json::from_str(&raw).unwrap();
    let mut dup = v["tools"][0].clone();
    dup["id"] = json!(v["tools"][0]["id"].as_str().unwrap().replace('.', "_"));
    v["tools"].as_array_mut().unwrap().push(dup);
    let e = CreatorCatalog::parse_and_verify(&v.to_string()).err().expect("wire collision");
    assert!(e.message.contains("WIRE_COLLISION"), "{e}");
}

#[test]
fn report_covers_generator_inputs() {
    let r = bgx_creator_contract::verify_root(&repo_root()).expect("repository verifies");
    assert_eq!(r.status, "passed");
    assert!(r.inputs.iter().any(|i| i.path == "packages/app-contracts/v1/creator-agent-turn-contract.json"));
    assert!(r.inputs.iter().all(|i| !i.path.contains(':')));
}

// ---- R03/R04: schema documents and keyword arguments ----

fn copy_dir(from: &std::path::Path, to: &std::path::Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let p = e.unwrap().path();
        let dest = to.join(p.file_name().unwrap());
        if p.is_dir() {
            copy_dir(&p, &dest);
        } else {
            std::fs::copy(&p, &dest).unwrap();
        }
    }
}

/// An isolated contract tree including the referenced adapter schemas, with one contract edited.
fn broken_root(tag: &str, file: &str, edit: impl FnOnce(&mut Value)) -> PathBuf {
    let root = std::env::temp_dir().join(format!("bgx-contract-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    for sub in [
        "packages/creator-client/v1",
        "packages/app-contracts/v1",
    ] {
        copy_dir(&repo_root().join(sub), &root.join(sub));
    }
    let path = root.join("packages/app-contracts/v1").join(file);
    let mut v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    edit(&mut v);
    std::fs::write(&path, v.to_string()).unwrap();
    root
}

fn assert_root_rejected(root: PathBuf, needle: &str) {
    let r = bgx_creator_contract::verify_root(&root);
    let _ = std::fs::remove_dir_all(&root);
    let e = r.err().expect("an invalid schema document must not produce a passing report");
    assert_eq!(e.code, "CREATOR_CONTRACT_INVALID");
    assert!(e.message.contains(needle), "{e}");
}

#[test]
fn isolated_contract_tree_resolves_workbench_projection_and_rejects_its_removal() {
    let root = broken_root("adapter-ref", "creator-tool-envelope.schema.json", |_| {});
    bgx_creator_contract::verify_root(&root).expect("unedited isolated tree verifies");
    std::fs::remove_file(root.join(
        "packages/app-contracts/v1/prompt-projection-capability.schema.json",
    ))
    .unwrap();
    assert_root_rejected(root, "SCHEMA_REF_FILE_MISSING");
}

#[test]
fn r03_invalid_envelope_type_fails_the_report() {
    let root = broken_root("strng", "creator-tool-envelope.schema.json", |v| v["type"] = json!("strng"));
    assert_root_rejected(root, "SCHEMA_TYPE_INVALID");
}

#[test]
fn r03_missing_definition_fails_the_report() {
    let root = broken_root("nodef", "creator-local-source-v1.schema.json", |v| {
        v["definitions"].as_object_mut().unwrap().remove("readInput");
    });
    assert_root_rejected(root, "definitions.readInput");
}

#[test]
fn r03_missing_referenced_definition_fails_the_report() {
    let root = broken_root("noref", "creator-local-source-v1.schema.json", |v| {
        v["definitions"].as_object_mut().unwrap().remove("sourceRef");
    });
    assert_root_rejected(root, "sourceRef");
}

#[test]
fn r03_bad_ref_fails_the_report() {
    let root = broken_root("badref", "creator-tool-envelope.schema.json", |v| {
        v["properties"]["r03Probe"] = json!({"$ref": "#/definitions/doesNotExist"});
    });
    assert_root_rejected(root, "SCHEMA_REF_TARGET_MISSING");
}

#[test]
fn r03_report_counts_verified_schema_documents() {
    let r = bgx_creator_contract::verify_root(&repo_root()).expect("repository verifies");
    assert!(r.counts.schema_documents_verified > 0);
    assert!(r.checks.iter().any(|c| c == "generatorInputs.schemaDocuments"));
}

#[test]
fn r04_mistyped_length_argument_is_rejected_not_ignored() {
    let e = verify_schema_keywords(&json!({"type": "string", "maxLength": "1"}), "s").unwrap_err();
    assert!(e.contains("SCHEMA_KEYWORD_ARGUMENT_INVALID"), "{e}");
    assert!(verify_schema_keywords(&json!({"type": "integer", "minimum": "0"}), "s").is_err());
    assert!(verify_schema_keywords(&json!({"type": "array", "maxItems": -1}), "s").is_err());
    assert!(verify_schema_keywords(&json!({"type": "string", "maxLength": 1}), "s").is_ok());
}

#[test]
fn r04_additional_properties_sub_schema_is_executed() {
    let s = json!({"type": "object", "additionalProperties": {"type": "string"}});
    verify_schema_keywords(&s, "s").expect("accepted schema form");
    let e = check_value(&s, &json!({"bad": 42}), "v").unwrap_err();
    assert_eq!(e.instance_path, "v.bad");
    assert!(check_value(&s, &json!({"ok": "x"}), "v").is_ok());
    let open = json!({"type": "object", "additionalProperties": true});
    assert!(check_value(&open, &json!({"any": 42}), "v").is_ok());
}


#[test]
fn turn_feedback_named_event_schema_is_resolved_and_rejects_bad_identity_and_unknown_fields() {
    let contract=CreatorTurnContract::parse_and_verify(&turn_raw()).expect("production contract");
    assert_eq!(contract.turn_progress_schema_version(),2);
    assert!(contract.turn_progress_reason_codes().iter().any(|value|value=="REPLY_ACCEPTED"));
    let event=json!({"schemaVersion":2,"hostIdempotencyKey":"turn","sessionId":"session","seq":1,
        "occurredAtMs":1,"stage":"reading","reasonCode":"MODEL_STARTED"});
    contract.validate_named("turnProgressEventV2",&event).unwrap();
    for (field,value) in [("seq",json!(0)),("sessionId",json!("")),("stage",json!("unknown")),("body",json!("not a progress field"))] {
        let mut invalid=event.clone();invalid[field]=value;
        assert!(contract.validate_named("turnProgressEventV2",&invalid).is_err(),"{field}");
    }
}

#[test]
fn turn_feedback_progress_metadata_is_strict_and_cannot_drift_from_named_schema() {
    for (pointer,value) in [("/turnProgress/unknown",json!(true)),("/turnProgress/schemaVersion",json!(3)),
        ("/turnProgress/merge",json!("")),("/turnProgress/reasonCodes",json!(["same","same"])),
        ("/schemas/turnProgressEventV2/properties/seq/minimum",json!(0)),
        ("/schemas/turnProgressEventV2/properties/stage/enum",json!(["invented"]))] {
        let mut invalid=turn_value();
        if pointer=="/turnProgress/unknown" {invalid["turnProgress"]["unknown"]=value;}
        else {*invalid.pointer_mut(pointer).unwrap()=value;}
        let error=CreatorTurnContract::parse_and_verify(&invalid.to_string()).err().expect("progress drift rejected");
        assert!(error.to_string().contains("turnProgress"),"{pointer}: {error}");
    }
    let mut missing=turn_value();missing["schemas"].as_object_mut().unwrap().remove("turnProgressEventV2");
    let error=CreatorTurnContract::parse_and_verify(&missing.to_string()).err().expect("named schema is mandatory");
    assert!(error.to_string().contains("turnProgressEventV2"),"{error}");
}
