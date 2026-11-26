use crate::schema_types::{self, GeneratedTypes};
use std::{collections::BTreeMap, path::Path};
/// Distribution export for Python, derived from the same authoritative inputs.
pub fn exported_contract(root: &Path) -> Result<String, String> {
    let schema = crate::read_contract_json(&root.join("packages/app-contracts/v1/asr.schema.json"));
    let constants = crate::read_contract_json(&root.join("packages/app-contracts/v1/asr.json"));
    let _ = scalars(root);
    let subtitle_timing =
        crate::read_contract_json(&root.join("packages/app-contracts/v1/subtitle-timing.json"));
    let mut export = serde_json::json!({
        "schema": schema, "constants": constants, "subtitleTiming": subtitle_timing
    });
    export.sort_all_objects();
    serde_json::to_string_pretty(&export)
        .map(|s| s + "\n")
        .map_err(|e| e.to_string())
}
pub fn generate(root: &Path) -> Result<GeneratedTypes, String> {
    let schema = crate::read_contract_json(&root.join("packages/app-contracts/v1/asr.schema.json"));
    let names: Vec<&str> = schema["definitions"]
        .as_object()
        .ok_or("ASR definitions missing")?
        .keys()
        .map(String::as_str)
        .collect();
    let documents = BTreeMap::from([("asr.schema.json".into(), schema.clone())]);
    schema_types::generate(&documents, "asr.schema.json", &names, "Asr")
}
pub fn scalars(root: &Path) -> String {
    let raw = crate::read_contract_json(&root.join("packages/app-contracts/v1/asr.json"));
    crate::require_schema_v1(&raw, "asr.json");
    for name in ["health", "prepare", "transcribe", "cancel", "release"] {
        assert!(
            raw["endpoints"][name]
                .as_str()
                .is_some_and(|s| s.starts_with('/')),
            "invalid ASR endpoint"
        );
    }
    validate_budgets(&raw).expect("invalid ASR budgets");
    let schema = crate::read_contract_json(&root.join("packages/app-contracts/v1/asr.schema.json"));
    assert_eq!(schema["definitions"]["protocolVersionV1"]["const"], raw["protocolVersion"], "ASR protocol schema/constants differ");
    let budgets = raw["budgets"].as_object().expect("ASR budgets");
    let mut generated = String::from("// @generated from packages/app-contracts/v1/asr.json. Do not edit.\n");
    for (name, value) in budgets {
        generated.push_str(&format!("pub const {}: u64 = {};\n", constant_name(name), value.as_u64().expect("validated budget")));
    }
    generated.push_str(&format!("pub const SCHEMA_VERSION: u64 = {};\npub const PROTOCOL_VERSION: u64 = {};\n", raw["schemaVersion"], raw["protocolVersion"]));
    generated.push_str(&
    format!("pub const ASR_CONTRACT_JSON: &str = {:?};\npub const ASR_SCHEMA_JSON: &str = {:?};\npub const DEFAULT_LANGUAGE: &str = {:?};\npub const BRIDGE_FEATURE: &str = {:?};\n", raw.to_string(), std::fs::read_to_string(root.join("packages/app-contracts/v1/asr.schema.json")).expect("ASR schema"),raw["defaultLanguage"].as_str().expect("defaultLanguage"),raw["bridgeFeature"].as_str().expect("bridgeFeature")));
    generated
}
fn constant_name(value: &str) -> String {
    let mut name = String::new();
    for c in value.chars() {
        if c.is_ascii_uppercase() { name.push('_'); }
        name.push(c.to_ascii_uppercase());
    }
    name
}
pub fn typescript(root: &Path) -> Result<String, String> {
    let generated = generate(root)?;
    let _ = scalars(root);
    let raw = crate::read_contract_json(&root.join("packages/app-contracts/v1/asr.json"));
    let mut output = generated.typescript + "\nexport type SelectionV1 = AsrSelectionV1;\n";
    output.push_str(&format!("export const ASR_SCHEMA_VERSION = {} as const;\nexport const ASR_PROTOCOL_VERSION = {} as const;\n", raw["schemaVersion"], raw["protocolVersion"]));
    for (name, value) in raw["budgets"].as_object().expect("budgets") {
        output.push_str(&format!("export const ASR_{} = {} as const;\n", constant_name(name), value));
    }
    Ok(output)
}
fn validate_budgets(raw: &serde_json::Value) -> Result<(), String> {
    for name in [
        "controlPayloadMaxBytes",
        "stderrTailMaxBytes",
        "preparedTtlMs",
        "preparedMaxEntries",
        "cloudBindingMaxEntries",
        "cloudBindingTtlMs",
        "residentIdleTtlMs",
        "residentSweepMaxIntervalMs",
        "prepareTimeoutMs",
    ] {
        if !raw["budgets"][name]
            .as_u64()
            .is_some_and(|n| n > 0 && n <= i64::MAX as u64)
        {
            return Err(format!("invalid ASR budget {name}"));
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_consumed_budget_is_required_positive_integer() {
        let raw: serde_json::Value = serde_json::from_str(include_str!(
            "../../../../packages/app-contracts/v1/asr.json"
        ))
        .unwrap();
        validate_budgets(&raw).unwrap();
        for name in [
            "controlPayloadMaxBytes",
            "stderrTailMaxBytes",
            "preparedTtlMs",
            "preparedMaxEntries",
            "cloudBindingMaxEntries",
            "cloudBindingTtlMs",
        "residentIdleTtlMs",
        "residentSweepMaxIntervalMs",
        "prepareTimeoutMs",
        ] {
            for invalid in [
                serde_json::Value::Null,
                serde_json::json!(0),
                serde_json::json!(-1),
                serde_json::json!(1.5),
                serde_json::json!("10"),
                serde_json::json!(u64::MAX),
            ] {
                let mut bad = raw.clone();
                bad["budgets"][name] = invalid;
                assert!(validate_budgets(&bad).is_err(), "{name}");
            }
        }
    }
}
