//! Field registry verification shared by build.rs, the offline verifier and client-pack build.
use crate::{validate_schema_value, ContractViolation};
use serde_json::Value;
use std::collections::BTreeSet;

pub fn verify(doc: &Value, schema: &Value, catalog: &Value) -> Result<(), ContractViolation> {
    const FILE: &str = "creator-resource-text-fields.json";
    validate_schema_value(FILE, "registry", schema, doc, "$")?;
    let fail = |message: String| ContractViolation::new(FILE, "resources", message);
    let mut kinds = BTreeSet::new();
    for resource in doc["resources"].as_array().ok_or_else(||fail("resources missing".into()))? {
        let kind = resource["kind"].as_str().ok_or_else(||fail("kind missing".into()))?;
        if !kinds.insert(kind) { return Err(fail(format!("duplicate resource kind: {kind}"))); }
        let mut fields = BTreeSet::new();
        for field in resource["fields"].as_array().ok_or_else(||fail("fields missing".into()))? {
            let id = field["fieldId"].as_str().ok_or_else(||fail("fieldId missing".into()))?;
            if !fields.insert(id) { return Err(fail(format!("duplicate field owner: {kind}.{id}"))); }
            if field["availability"] == "editable" {
                let published = catalog["tools"].as_array().is_some_and(|tools|tools.iter().any(|tool|tool["id"]==field["writeToolId"] && tool["available"]==true));
                if !published { return Err(fail(format!("unpublished field writer: {kind}.{id}"))); }
            }
            if field.get("saveModes").is_some() {
                let history = match kind { "storyProp" => "storyPropField", "visualAsset" => "visualAssetContent", "canvasNode" => "freeCanvasContent", _ => "nodeContent" };
                let cas = match kind { "segment" => "partition", "canvasNode" => "node", _ => "document" };
                if field["adapterId"] != kind || field["historyFamily"] != history || field["casDomain"] != cas {
                    return Err(fail(format!("field adapter/history/CAS owner mismatch: {kind}.{id}")));
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn me_basic_production_field_registry_has_one_valid_owner_per_field() {
        let doc = serde_json::from_str(include_str!("../../../../packages/app-contracts/v1/creator-resource-text-fields.json")).unwrap();
        let schema = serde_json::from_str(include_str!("../../../../packages/app-contracts/v1/creator-resource-text-fields.schema.json")).unwrap();
        let catalog = serde_json::from_str(include_str!("../../../../packages/app-contracts/v1/creator-tool-catalog.json")).unwrap();
        verify(&doc, &schema, &catalog).unwrap();
    }
}
