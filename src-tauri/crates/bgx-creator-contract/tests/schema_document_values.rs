use bgx_creator_contract::{check_document_value, resolve_schema_document, SchemaDocument};
use serde_json::{json, Value};

fn resource_schema() -> Value {
    serde_json::from_str(include_str!(
        "../../../../packages/app-contracts/v1/creator-resource-ref.schema.json"
    ))
    .unwrap()
}

#[test]
fn recursive_media_owners_are_checked_at_each_actual_child() {
    let root = resource_schema();
    let external = |_: &str| None;
    let doc = SchemaDocument {
        file: "creator-resource-ref.schema.json",
        root: &root,
        external: &external,
    };
    let leaf = json!({"kind":"canvasNode","projectId":"p","nodeId":"fca:source"});
    let mut nested = leaf;
    for n in 0..12 {
        nested = json!({"kind":"mediaVersion","owner":nested,"versionId":format!("v{n}")});
    }
    check_document_value(&doc, &nested, "$").unwrap();
    let malformed = json!({"kind":"mediaVersion","versionId":"v","owner":{
        "kind":"mediaVersion","versionId":"v2","owner":{"kind":"canvasNode","projectId":"p"}
    }});
    assert!(check_document_value(&doc, &malformed, "$").is_err());
    let unknown = json!({"kind":"mediaVersion","versionId":"v","owner":{
        "kind":"canvasNode","projectId":"p","nodeId":"fca:source","extra":true
    }});
    assert!(check_document_value(&doc, &unknown, "$").is_err());
    // Finite expansion keeps its previous explicit refusal of cycles.
    assert!(resolve_schema_document(&doc).is_err());
}

#[test]
fn cross_document_refs_keep_the_target_root_and_sibling_constraints() {
    let resource = resource_schema();
    let root = json!({"$ref":"creator-resource-ref.schema.json","required":["versionId"]});
    let external = |name: &str| (name == "creator-resource-ref.schema.json").then_some(&resource);
    let doc = SchemaDocument {
        file: "input.json",
        root: &root,
        external: &external,
    };
    let owner = json!({"kind":"projectFile","relativePath":"audio/source.wav"});
    assert!(check_document_value(&doc, &owner, "$").is_err());
    check_document_value(
        &doc,
        &json!({"kind":"mediaVersion","owner":owner,"versionId":"v"}),
        "$",
    )
    .unwrap();
}

#[test]
fn non_consuming_reference_cycle_is_an_explicit_error() {
    let root = json!({"$ref":"#"});
    let external = |_: &str| None;
    let doc = SchemaDocument {
        file: "cycle.json",
        root: &root,
        external: &external,
    };
    let error = check_document_value(&doc, &json!({}), "$").unwrap_err();
    assert_eq!(error.schema_path, "$ref");
    assert_eq!(error.instance_path, "$");
}

#[test]
fn tagged_clip_graph_action_reports_the_misplaced_nested_field() {
    let mut root: Value = serde_json::from_str(include_str!(
        "../../../../packages/app-contracts/v1/creator-clip-workflow.schema.json"
    )).unwrap();
    root["$ref"] = json!("#/definitions/ChangePrepareArgs");
    let clip: Value = serde_json::from_str(include_str!(
        "../../../../packages/app-contracts/v1/clip-workflow.schema.json"
    )).unwrap();
    let external = |name: &str| (name == "clip-workflow.schema.json").then_some(&clip);
    let doc = SchemaDocument { file: "creator-clip-workflow.schema.json", root: &root, external: &external };
    let mut args = json!({"actions": [{
        "actionId": "edit", "type": "clip.graph.change",
        "request": {"workspaceId":"ws", "expectedRevision":1, "changes":[{"type":"delete","nodeIds":["n"]}]}
    }]});
    check_document_value(&doc, &args, "arguments").expect("valid change action");
    args["actions"][0]["request"]["type"] = json!("clip.graph.change");
    let error = check_document_value(&doc, &args, "arguments").unwrap_err();
    assert_eq!(error.instance_path, "arguments.actions[0].request.type", "{error}");
    assert_eq!(error.schema_path, "additionalProperties");
    assert!(error.branch_reasons.is_empty());
    assert_eq!(error.to_string(), "arguments.actions[0].request.type: not allowed [schemaPath=additionalProperties]");

    args["actions"][0]["type"] = json!("clip.unknown");
    let unknown = check_document_value(&doc, &args, "arguments").unwrap_err();
    assert_eq!(unknown.instance_path, "arguments.actions[0]");
    assert_eq!(unknown.schema_path, "oneOf");
    assert!(unknown.branch_reasons.len() > 1);
}

#[test]
fn one_of_promotes_a_unique_singleton_tag_and_preserves_ambiguous_results() {
    use bgx_creator_contract::schema_value::check_value;
    let branch = |tag, field| json!({"properties": {"kind": {"enum": [tag]}}, "required": [field]});
    let a = branch("edit", "patch");
    let b = branch("read", "selection");
    let tagged = json!({"oneOf": [a, b]});
    let input = json!({"kind": "read"});
    let error = check_value(&tagged, &input, "call").unwrap_err();
    assert_eq!((error.instance_path.as_str(),error.schema_path.as_str()), ("call.selection","required"));
    check_value(&tagged, &json!({"kind":"read","selection":"n"}), "call").unwrap();

    for schema in [
        json!({"oneOf":[a, branch("read","selection"), branch("read","another")]}),
        json!({"oneOf":[a, b, {"required":["other"]}]}),
        json!({"oneOf":[{"properties":{"kind":{"const":"read"},"mode":{"const":"first"}}},
                         {"properties":{"kind":{"const":"write"},"mode":{"const":"second"}}}]}),
    ] {
        let error = check_value(&schema, &json!({"kind":"read","mode":"second"}), "call").unwrap_err();
        assert_eq!(error.schema_path, "oneOf");
        assert!(error.branch_reasons.len() > 1);
    }
    let overlapping = json!({"oneOf":[b, b]});
    let error = check_value(&overlapping, &json!({"kind":"read","selection":"n"}), "call").unwrap_err();
    assert!(error.message.contains("matched 2"));
}

#[test]
fn document_and_resolved_checkers_agree_for_all_structural_keywords() {
    let root = json!({
        "definitions":{"number":{"type":"integer","minimum":1,"maximum":5}},
        "type":"object","additionalProperties":false,"required":["items"],
        "properties":{
            "items":{"type":"array","minItems":1,"maxItems":2,"uniqueItems":true,"items":{"$ref":"#/definitions/number"}},
            "choice":{"oneOf":[{"const":"a"},{"const":"b"}]},
            "label":{"type":"string","minLength":1,"maxLength":3}
        },
        "if":{"required":["choice"]},"then":{"required":["label"]},
        "not":{"required":["label"],"properties":{"label":{"const":"bad"}}}
    });
    let external = |_: &str| None;
    let doc = SchemaDocument {
        file: "finite.json",
        root: &root,
        external: &external,
    };
    let resolved = resolve_schema_document(&doc).unwrap();
    let cases = [
        json!({"items":[1]}),
        json!({"items":[1,5],"choice":"a","label":"好"}),
        json!({"items":[]}),
        json!({"items":[0]}),
        json!({"items":[1,1]}),
        json!({"items":[1],"choice":"a"}),
        json!({"items":[1],"label":"bad"}),
        json!({"items":[1],"extra":1}),
        json!({"items":[1,2,3]}),
        json!({"items":[1],"choice":"c","label":"ok"}),
    ];
    for value in cases {
        assert_eq!(
            check_document_value(&doc, &value, "$").is_ok(),
            bgx_creator_contract::schema_value::check_value(&resolved, &value, "$").is_ok(),
            "{value}"
        );
    }
}

#[test]
fn singing_shared_fixture_uses_recursive_resource_contract_without_expansion() {
    let root: Value = serde_json::from_str(include_str!(
        "../../../../packages/app-contracts/v1/singing-relyrics.schema.json"
    ))
    .unwrap();
    let resource = resource_schema();
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../../fixtures/singing-relyrics/contract-cases.json"
    ))
    .unwrap();
    let mut docs = std::collections::BTreeMap::new();
    docs.insert("creator-resource-ref.schema.json", resource);
    docs.insert("singing-relyrics.schema.json", root.clone());
    for (name, raw) in [
        ("creator-call-context-v4.generated.schema.json", include_str!("../../../../packages/app-contracts/v1/creator-call-context-v4.generated.schema.json")),
        ("creator-effect-receipt.schema.json", include_str!("../../../../packages/app-contracts/v1/creator-effect-receipt.schema.json")),
        ("creator-view-refresh.schema.json", include_str!("../../../../packages/app-contracts/v1/creator-view-refresh.schema.json")),
    ] {
        docs.insert(name, serde_json::from_str(raw).unwrap());
    }
    let external = |name: &str| docs.get(name);
    let doc = SchemaDocument {
        file: "singing-relyrics.schema.json",
        root: &root,
        external: &external,
    };
    check_document_value(&doc, &fixture["prepare"]["arguments"], "$").unwrap();
    let mut prepare = fixture["prepare"]["arguments"].clone();
    prepare["sourceAudio"] =
        json!({"kind":"mediaVersion","versionId":"v1","owner":prepare["sourceAudio"]});
    check_document_value(&doc, &prepare, "$").unwrap();
    prepare["sourceAudio"]["owner"]["extra"] = json!(true);
    assert!(check_document_value(&doc, &prepare, "$").is_err());
    for case in fixture["schemaCases"].as_array().unwrap() {
        let entry = json!({"$ref":format!("singing-relyrics.schema.json#/definitions/{}", case["definition"].as_str().unwrap())});
        let external = |name: &str| docs.get(name);
        let doc = SchemaDocument {
            file: "fixture-case",
            root: &entry,
            external: &external,
        };
        let result = check_document_value(&doc, &case["payload"], "$");
        assert_eq!(
            result.is_ok(),
            case["expect"] == "accept",
            "{}: {result:?}",
            case["name"]
        );
    }
}
