//! Local GPU engine kinds: single authority is packages/app-contracts/v1/local-gpu-occupancy.json.
use std::{collections::BTreeSet, path::Path};

pub const OUT_RS: &str = "local_gpu_occupancy_generated.rs";
pub const TS_TARGET: &str = "ui/src/domain/local-gpu-occupancy/gpuEngineKind.generated.ts";

/// TypeScript closed set of engine kinds, derived from the same contract.
pub fn typescript(root: &Path) -> Result<String, String> {
    let raw = crate::read_contract_json(&root.join("packages/app-contracts/v1/local-gpu-occupancy.json"));
    render(&raw)?;
    let ids: Vec<String> = raw["engineKinds"]
        .as_array()
        .ok_or("engineKinds missing")?
        .iter()
        .map(|k| format!("{:?}", k["id"].as_str().unwrap_or_default()))
        .collect();
    Ok(format!(
        "// @generated from packages/app-contracts/v1/local-gpu-occupancy.json. Do not edit.\nexport const GPU_ENGINE_KINDS = [{}] as const;\nexport type GpuEngineKind = (typeof GPU_ENGINE_KINDS)[number];\n",
        ids.join(", ")
    ))
}

pub fn generate(root: &Path) -> Result<String, String> {
    let raw = crate::read_contract_json(&root.join("packages/app-contracts/v1/local-gpu-occupancy.json"));
    render(&raw)
}

pub fn render(raw: &serde_json::Value) -> Result<String, String> {
    if raw.get("schemaVersion").and_then(|v| v.as_u64()) != Some(1) {
        return Err("local-gpu-occupancy.json schemaVersion must be 1".into());
    }
    let obj = raw.as_object().ok_or("local-gpu-occupancy.json must be an object")?;
    if obj.keys().any(|k| k != "schemaVersion" && k != "engineKinds") {
        return Err("local-gpu-occupancy.json has unknown top-level fields".into());
    }
    let kinds = raw["engineKinds"].as_array().filter(|a| !a.is_empty()).ok_or("engineKinds must be a non-empty array")?;
    let (mut ids, mut variants) = (BTreeSet::new(), BTreeSet::new());
    let mut rows = Vec::with_capacity(kinds.len());
    for kind in kinds {
        let row = kind.as_object().ok_or("engineKind must be an object")?;
        if row.len() != 2 {
            return Err("engineKind must have exactly id and rustVariant".into());
        }
        let id = row.get("id").and_then(|v| v.as_str()).ok_or("engineKind.id missing")?;
        let variant = row.get("rustVariant").and_then(|v| v.as_str()).ok_or("engineKind.rustVariant missing")?;
        if id.is_empty() || !id.bytes().all(|b| b.is_ascii_lowercase() || b == b'_') {
            return Err(format!("engineKind.id `{id}` must be lower snake ascii"));
        }
        let mut chars = variant.chars();
        if !chars.next().is_some_and(|c| c.is_ascii_uppercase()) || !chars.all(|c| c.is_ascii_alphanumeric()) {
            return Err(format!("engineKind.rustVariant `{variant}` must be UpperCamel ascii"));
        }
        if !ids.insert(id.to_string()) || !variants.insert(variant.to_string()) {
            return Err(format!("duplicate engineKind `{id}`/`{variant}`"));
        }
        rows.push((id.to_string(), variant.to_string()));
    }
    let mut out = String::from("// @generated from packages/app-contracts/v1/local-gpu-occupancy.json. Do not edit.\n");
    out.push_str("#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]\npub enum GpuEngineKind {\n");
    for (id, variant) in &rows {
        out.push_str(&format!("    #[serde(rename = {id:?})]\n    {variant},\n"));
    }
    out.push_str("}\n\nimpl GpuEngineKind {\n    pub fn as_str(self) -> &'static str {\n        match self {\n");
    for (id, variant) in &rows {
        out.push_str(&format!("            Self::{variant} => {id:?},\n"));
    }
    out.push_str(&format!("        }}\n    }}\n\n    pub fn all() -> [Self; {}] {{\n        [\n", rows.len()));
    for (_, variant) in &rows {
        out.push_str(&format!("            Self::{variant},\n"));
    }
    out.push_str("        ]\n    }\n}\n");
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contract() -> serde_json::Value {
        serde_json::from_str(include_str!("../../../../packages/app-contracts/v1/local-gpu-occupancy.json")).unwrap()
    }

    #[test]
    fn authoritative_contract_renders_asr_kind() {
        let out = render(&contract()).unwrap();
        assert!(out.contains("#[serde(rename = \"asr\")]\n    Asr,"));
        assert!(out.contains("#[serde(rename = \"singing\")]\n    Singing,"));
        let count = contract()["engineKinds"].as_array().unwrap().len();
        assert!(out.contains(&format!("pub fn all() -> [Self; {count}]")));
    }

    #[test]
    fn rejects_duplicates_and_unknown_fields() {
        let mut dup = contract();
        let first = dup["engineKinds"][0].clone();
        dup["engineKinds"].as_array_mut().unwrap().push(first);
        assert!(render(&dup).is_err());
        let mut extra = contract();
        extra["engineKinds"][0]["label"] = serde_json::json!("x");
        assert!(render(&extra).is_err());
        let mut top = contract();
        top["extra"] = serde_json::json!(1);
        assert!(render(&top).is_err());
    }
}
