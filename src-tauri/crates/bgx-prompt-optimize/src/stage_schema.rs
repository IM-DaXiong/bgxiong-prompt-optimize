//! Official non-expression output schemas for `run_video_optimize`.
//!
//! Binders are ports of the host true-source functions (document, reference
//! evidence, source normalize, lyric spans, mother review). They only restrict
//! official contracts with this run's closed IDs; they do not invent a looser
//! schema when bind inputs are missing.

use serde_json::{json, Value};

use bgx_video_prompt_contract::document::{contract, DocumentDraftV1};

use crate::error::{error_codes, OptimizeError};

/// Official quote-role enum from `H3QuoteRole::ALL` (host source-normalize types).
const SOURCE_QUOTE_ROLES: &[&str] = &[
    "vocalized_dialogue",
    "vocalized_voiceover",
    "vocalized_singing",
    "visible_text",
    "non_vocal_quote",
];

const REVIEW_SEVERITY_VALUES: [&str; 2] = ["blocking", "minor"];
const REVIEW_SOURCE_CHECK_RESULTS: [&str; 4] = ["preserved", "omitted", "altered", "not_required"];

fn config_err(message: impl Into<String>) -> OptimizeError {
    OptimizeError::new(error_codes::INTERNAL_PROMPT_CONFIG, message)
}

fn sorted_strings(values: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut out: Vec<String> = values.into_iter().collect();
    out.sort();
    out
}

fn closed_array(schema: &mut Value, ids: &[String]) {
    if ids.is_empty() {
        schema["maxItems"] = json!(0);
    } else {
        let unique: std::collections::BTreeSet<&str> = ids.iter().map(String::as_str).collect();
        schema["items"]["enum"] = json!(unique);
    }
}

/// Official `video_source_normalize::output_schema`, then bind unit IDs as the host `run_schema`.
pub fn source_run_schema(source_text: &str) -> Result<(Value, Value), OptimizeError> {
    let units = source_units(source_text);
    if units.is_empty() {
        return Err(OptimizeError::new(
            "SOURCE_NORMALIZE_EMPTY",
            "原稿为空，无法理解。",
        ));
    }
    let mut schema = source_output_schema();
    let ids: Vec<&str> = units.iter().map(|(id, _)| id.as_str()).collect();
    schema["properties"]["facts"]["items"]["properties"]["id"]["enum"] = json!(ids);
    schema["properties"]["quotes"]["items"]["properties"]["sourceUnitId"]["enum"] = json!(ids);
    let catalog = json!({
        "sourceCatalog": {
            "sourceUnits": units.iter().map(|(id, text)| json!({"id": id, "exactText": text})).collect::<Vec<_>>()
        }
    });
    Ok((catalog, schema))
}

/// Official lyric spans schema bound to the same F{n} source unit IDs.
pub fn lyric_run_schema(source_text: &str) -> Result<(Value, Value), OptimizeError> {
    let units = source_units(source_text);
    let mut schema = json!({
        "type":"object","additionalProperties":false,"required":["sungSpans"],
        "properties":{"sungSpans":{"type":"array","items":{"type":"object","additionalProperties":false,
            "required":["sourceUnitId","exactText"],"properties":{
                "sourceUnitId":{"type":"string","minLength":1},"exactText":{"type":"string","minLength":1}
            }}}}
    });
    if units.is_empty() {
        schema["properties"]["sungSpans"]["maxItems"] = json!(0);
    } else {
        schema["properties"]["sungSpans"]["items"]["properties"]["sourceUnitId"]["enum"] =
            json!(units.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>());
    }
    let catalog = json!({
        "sourceUnits": units.iter().map(|(id, text)| json!({"sourceUnitId": id, "exactText": text})).collect::<Vec<_>>()
    });
    Ok((catalog, schema))
}

/// Official reference evidence schema bound to frozen item IDs.
/// Official `mother_run_output_schema`. Empty shot set is an error, not an unbound schema.
pub fn mother_run_schema(
    expected_shot_ids: &[String],
    source_unit_ids: &[String],
    quote_candidates: &[Value],
) -> Result<Value, OptimizeError> {
    if expected_shot_ids.is_empty() {
        return Err(config_err("mother output shot set is empty"));
    }
    let mut schema = mother_output_schema();
    let shots = &mut schema["properties"]["shots"];
    shots["minItems"] = json!(expected_shot_ids.len());
    shots["maxItems"] = json!(expected_shot_ids.len());
    shots["items"]["required"] = json!(["shotId", "facts"]);
    shots["items"]["properties"]["facts"]["minItems"] = json!(1);
    shots["items"]["properties"]["shotId"]["enum"] = json!(expected_shot_ids);
    let quotes = &mut shots["items"]["properties"]["quotes"];
    if quote_candidates.is_empty() {
        quotes["maxItems"] = json!(0);
    } else {
        let mut ids = Vec::new();
        for candidate in quote_candidates {
            let id = candidate
                .get("quoteId")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| config_err("mother quote candidate lacks quoteId"))?;
            let source = candidate
                .get("sourceUnitId")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty());
            if let Some(source) = source {
                if !source_unit_ids.iter().any(|id| id == source) {
                    return Err(config_err(
                        "mother quote source is outside the frozen units",
                    ));
                }
            }
            ids.push(id.to_string());
        }
        if expected_shot_ids.len() == 1 {
            quotes["maxItems"] = json!(ids.len());
        }
        quotes["items"]["properties"]["quoteId"]["enum"] = json!(ids);
    }
    Ok(schema)
}

/// Official mother-review binder (`review_schema::output_schema`).
pub fn mother_review_schema(input: &Value) -> Result<Value, OptimizeError> {
    let draft: DocumentDraftV1 = serde_json::from_value(input["draft"].clone())
        .map_err(|e| config_err(format!("mother review draft contract: {e}")))?;
    let source = input.get("frozenSource");
    let reviewable: Vec<String> = match input.get("reviewableSourceUnitIds") {
        Some(ids) => serde_json::from_value(ids.clone())
            .map_err(|e| config_err(format!("mother review source IDs: {e}")))?,
        None if source.is_none() => vec![],
        None => return Err(config_err("mother review lacks reviewableSourceUnitIds")),
    };
    let shots: Vec<String> = draft
        .shots
        .iter()
        .map(|shot| shot.shot_id.clone())
        .collect();
    let facts: Vec<String> = draft
        .shots
        .iter()
        .flat_map(|shot| shot.facts.iter().map(|fact| fact.fact_id.clone()))
        .collect();
    let quotes: Vec<String> = draft
        .shots
        .iter()
        .flat_map(|shot| shot.quotes.iter().map(|quote| quote.quote_id.clone()))
        .collect();
    let values = |collection: &str, key: &str| -> Result<Vec<String>, OptimizeError> {
        let Some(source) = source else {
            return Ok(vec![]);
        };
        source[collection]
            .as_array()
            .ok_or_else(|| config_err(format!("mother review lacks frozenSource.{collection}")))?
            .iter()
            .map(|row| {
                row[key]
                    .as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| config_err(format!("mother review lacks {collection}.{key}")))
            })
            .collect()
    };
    let units = values("sourceUnits", "unitId")?;
    let references = values("referenceInventory", "itemId")?;
    let mut schema = review_output_schema();
    schema["description"] = json!("仅提交含 sourceChecks 和 findings 两个字段的完整对象。sourceChecks 的每个结论对象必须完整放在该数组内；item/items 是 schema 的数组说明，不是输出字段名。不得把 sourceUnitId、result、factIds 或 quoteIds 散到顶层。");
    let checks = &mut schema["properties"]["sourceChecks"];
    checks["description"] = json!("来源核对对象数组；每个来源单元对应一个完整对象，不是若干散开的字段。即使只有一个来源也保留数组外层。");
    checks["items"]["description"] = json!("本数组中的一个结论对象：sourceUnitId、factIds、quoteIds、result 四个字段都写在同一对花括号内，不能拆开或写成 item 包装对象。");
    checks["items"]["properties"]["sourceUnitId"]["description"] =
        json!("本行核对的一个来源单元 ID，字符串；只属于当前 sourceChecks 数组项，不能写到顶层。");
    checks["items"]["properties"]["result"]["description"] =
        json!("本行来源单元的结论；与本行 sourceUnitId 同属一个对象，不能写到顶层。");
    checks["items"]["properties"]["factIds"]["description"] = json!("承接本行来源的母版事实 ID 数组；无引用写 []，单个引用也写字符串数组，不能写字符串或 item 字段。");
    checks["items"]["properties"]["quoteIds"]["description"] = json!("承接本行来源的母版引文 ID 数组；无引文写 []，单个引文也写字符串数组，不能用空字符串、单个字符串或 item 字段代替。");
    checks["minItems"] = json!(reviewable.len());
    checks["maxItems"] = json!(reviewable.len());
    if !reviewable.is_empty() {
        checks["items"]["properties"]["sourceUnitId"]["enum"] = json!(reviewable);
    }
    closed_array(&mut checks["items"]["properties"]["factIds"], &facts);
    closed_array(&mut checks["items"]["properties"]["quoteIds"], &quotes);
    if quotes.is_empty() {
        checks["items"]["properties"]["quoteIds"]["description"] = json!("本次母版 quotes 为空，只能写 []。台词若由 facts 承接，只在 factIds 引用该事实，不能新造引文 ID。");
    }
    let writable: Vec<String> = source
        .and_then(|source| source["sourceUnits"].as_array())
        .into_iter()
        .flatten()
        .filter(|unit| unit["writeScope"].as_str() == Some("writable"))
        .filter_map(|unit| unit["unitId"].as_str().map(str::to_owned))
        .collect();
    if !writable.is_empty() {
        let writable_results: Vec<Value> = checks["items"]["properties"]["result"]["enum"]
            .as_array()
            .ok_or_else(|| config_err("mother review result enum missing"))?
            .iter()
            .filter(|value| value.as_str() != Some("not_required"))
            .cloned()
            .collect();
        checks["items"]["allOf"] = json!([{
            "if":{"required":["sourceUnitId"],"properties":{"sourceUnitId":{"enum":writable}}},
            "then":{"properties":{"result":{"enum":writable_results}}}
        }]);
    }
    let finding = &mut schema["properties"]["findings"]["items"]["properties"];
    finding["shotId"] = if shots.is_empty() {
        json!({"type":"null"})
    } else {
        json!({"anyOf":[{"type":"null"},{"type":"string","enum":shots}]})
    };
    for (field, ids) in [
        ("factIds", &facts),
        ("quoteIds", &quotes),
        ("sourceUnitIds", &units),
        ("referenceItemIds", &references),
    ] {
        closed_array(&mut finding[field], ids);
    }
    Ok(schema)
}

/// Official source-unit IDs: `F{n}` over `split_inclusive('\\n')`.
pub fn source_units(source: &str) -> Vec<(String, String)> {
    source
        .split_inclusive('\n')
        .enumerate()
        .map(|(index, text)| (format!("F{}", index + 1), text.to_owned()))
        .collect()
}

fn source_output_schema() -> Value {
    json!({
        "type":"object", "additionalProperties":false,
        "properties":{
            "facts":{"type":"array","items":{
                "type":"object","additionalProperties":true,
                "required":["id"],
                "properties":{
                    "id":{"type":"string","minLength":1},
                    "semanticKind":{"type":"string"},
                    "polarity":{"type":"string"}
                }
            }},
            "quotes":{"type":"array","items":{
                "type":"object","additionalProperties":false,
                "required":["sourceUnitId","exactText","role"],
                "properties":{
                    "sourceUnitId":{"type":"string","minLength":1},
                    "exactText":{"type":"string","minLength":1},
                    "role":{"type":"string","enum":SOURCE_QUOTE_ROLES},
                    "speakerIndex":{"type":"integer","minimum":0,"description":"1-based speaker number in first-speaking order; the same speaker reuses it and different speakers differ; omit when the source does not identify the speaker."},
                    "timeline":{"type":"string","enum":["none","scenetrans","cutoff"]}
                }
            }}
        }
    })
}

fn mother_output_schema() -> Value {
    let kinds = sorted_strings(contract().kinds.iter().cloned());
    let slots = sorted_strings(contract().slots.iter().cloned());
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["shots"],
        "properties": {
            "shots": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["shotId"],
                    "properties": {
                        "shotId": {"type": "string", "minLength": 1},
                        "facts": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "additionalProperties": false,
                                "required": ["factId", "text"],
                                "properties": {
                                    "factId": {"type": "string"},
                                    "kind": {"type": "string", "description": format!("推荐分类：{}；分类差异只作建议，保留实际文字。", kinds.join(", "))},
                                    "text": {"type": "string"},
                                    "semanticSlot": {"type": "string", "description": format!("可选写作分组，推荐：{}；不影响原文保留。", slots.join(", "))},
                                    "polarity": {"type": "string"},
                                    "inferred": {"type": "boolean"},
                                    "entityRefIds": {"type": "array", "items": {"type": "string", "minLength": 1}},
                                    "sourceUnitIds": {"type": "array", "items": {"type": "string", "minLength": 1}}
                                }
                            }
                        },
                        "quotes": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "additionalProperties": false,
                                "required": ["quoteId"],
                                "properties": {
                                    "quoteId": {"type": "string", "minLength": 1}
                                }
                            }
                        }
                    }
                }
            }
        }
    })
}

fn review_output_schema() -> Value {
    let codes = sorted_strings(contract().mother_review_issue_codes.iter().cloned());
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["sourceChecks", "findings"],
        "properties": {
            "sourceChecks": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["sourceUnitId", "factIds", "quoteIds", "result"],
                    "properties": {
                        "sourceUnitId": {"type": "string", "minLength": 1},
                        "factIds": {"type": "array", "items": {"type": "string", "minLength": 1}},
                        "quoteIds": {"type": "array", "items": {"type": "string", "minLength": 1}},
                        "result": {"type": "string", "enum": REVIEW_SOURCE_CHECK_RESULTS}
                    }
                }
            },
            "findings": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["shotId", "factIds", "quoteIds", "sourceUnitIds", "referenceItemIds", "code", "severity", "detail"],
                    "properties": {
                        "shotId": {"anyOf": [{"type": "null"}, {"type": "string", "minLength": 1}]},
                        "factIds": {"type": "array", "items": {"type": "string", "minLength": 1}},
                        "quoteIds": {"type": "array", "items": {"type": "string", "minLength": 1}},
                        "sourceUnitIds": {"type": "array", "items": {"type": "string", "minLength": 1}},
                        "referenceItemIds": {"type": "array", "items": {"type": "string", "minLength": 1}},
                        "code": {"type": "string", "enum": codes},
                        "severity": {"type": "string", "enum": REVIEW_SEVERITY_VALUES},
                        "detail": {"type": "string", "minLength": 1}
                    }
                }
            }
        }
    })
}


/// Official multi-shot transport schema (`author_batch_envelope`).
/// The prompt still embeds the full machine contract; the wire envelope only
/// requires an owned `shotId`. Official name: `motherDraftBatch`.
pub fn author_batch_envelope(schema: &Value) -> Value {
    let shot_id = schema["properties"]["shots"]["items"]["properties"]["shotId"].clone();
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["shots"],
        "properties": {
            "shots": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": true,
                    "required": ["shotId"],
                    "properties": { "shotId": shot_id }
                }
            }
        }
    })
}

/// Official `bind_mother_source_scopes`. Skips unless the frozen source mode is
/// `span`. Only `writable` and `visual_only` units may appear in `sourceUnitIds`.
pub fn bind_mother_source_scopes(schema: &mut Value, source: &super::MotherSourceScopeBind) {
    if !matches!(source.source_mode, super::MotherSourceMode::Span) {
        return;
    }
    let allowed: Vec<&str> = source
        .source_units
        .iter()
        .filter(|unit| {
            matches!(
                unit.write_scope,
                super::MotherSourceWriteScope::Writable
                    | super::MotherSourceWriteScope::VisualOnly
            )
        })
        .map(|unit| unit.unit_id.as_str())
        .collect();
    let ids = &mut schema["properties"]["shots"]["items"]["properties"]["facts"]["items"]
        ["properties"]["sourceUnitIds"];
    if allowed.is_empty() {
        ids["maxItems"] = json!(0);
    } else {
        ids["items"]["enum"] = json!(allowed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::{MotherSourceMode, MotherSourceScopeBind, MotherSourceUnitScope, MotherSourceWriteScope};

    fn bound_mother_schema() -> Value {
        mother_run_schema(
            &["s1".into(), "s2".into()],
            &["u1".into(), "u2".into()],
            &[],
        )
        .unwrap()
    }

    #[test]
    fn author_batch_envelope_keeps_owned_shot_id_and_does_not_weaken_the_prompt_contract() {
        let schema = bound_mother_schema();
        let envelope = author_batch_envelope(&schema);
        assert_eq!(envelope["type"], "object");
        assert_eq!(envelope["additionalProperties"], false);
        assert_eq!(envelope["required"], json!(["shots"]));
        let items = &envelope["properties"]["shots"]["items"];
        assert_eq!(items["additionalProperties"], true);
        assert_eq!(items["required"], json!(["shotId"]));
        assert_eq!(
            items["properties"]["shotId"],
            schema["properties"]["shots"]["items"]["properties"]["shotId"]
        );
        assert!(items.get("properties").unwrap().get("facts").is_none());
        assert_eq!(
            schema["properties"]["shots"]["items"]["properties"]["shotId"]["enum"],
            json!(["s1", "s2"])
        );
    }

    #[test]
    fn bind_mother_source_scopes_skips_ordinary_and_binds_writable_or_visual_only_span_units() {
        let mut schema = bound_mother_schema();
        bind_mother_source_scopes(
            &mut schema,
            &MotherSourceScopeBind {
                source_mode: MotherSourceMode::Ordinary,
                source_units: vec![MotherSourceUnitScope {
                    unit_id: "u1".into(),
                    write_scope: MotherSourceWriteScope::Writable,
                }],
            },
        );
        assert!(
            schema["properties"]["shots"]["items"]["properties"]["facts"]["items"]["properties"]
                ["sourceUnitIds"]
                .get("items")
                .and_then(|items| items.get("enum"))
                .is_none()
        );

        bind_mother_source_scopes(
            &mut schema,
            &MotherSourceScopeBind {
                source_mode: MotherSourceMode::Span,
                source_units: vec![
                    MotherSourceUnitScope {
                        unit_id: "u1".into(),
                        write_scope: MotherSourceWriteScope::Writable,
                    },
                    MotherSourceUnitScope {
                        unit_id: "u-visual".into(),
                        write_scope: MotherSourceWriteScope::VisualOnly,
                    },
                    MotherSourceUnitScope {
                        unit_id: "u-context".into(),
                        write_scope: MotherSourceWriteScope::ContextOnly,
                    },
                    MotherSourceUnitScope {
                        unit_id: "u-boundary".into(),
                        write_scope: MotherSourceWriteScope::BoundaryOnly,
                    },
                ],
            },
        );
        assert_eq!(
            schema["properties"]["shots"]["items"]["properties"]["facts"]["items"]["properties"]
                ["sourceUnitIds"]["items"]["enum"],
            json!(["u1", "u-visual"])
        );

        let mut empty = bound_mother_schema();
        bind_mother_source_scopes(
            &mut empty,
            &MotherSourceScopeBind {
                source_mode: MotherSourceMode::Span,
                source_units: vec![MotherSourceUnitScope {
                    unit_id: "u-context".into(),
                    write_scope: MotherSourceWriteScope::ContextOnly,
                }],
            },
        );
        assert_eq!(
            empty["properties"]["shots"]["items"]["properties"]["facts"]["items"]["properties"]
                ["sourceUnitIds"]["maxItems"],
            json!(0)
        );
    }
}
