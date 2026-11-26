//! Prompt target provenance uses the existing Comfy mapping closed sets as its single source.
use std::{collections::BTreeMap, path::Path};
use crate::schema_types::{self, GeneratedTypes};

pub fn generate(root: &Path) -> Result<GeneratedTypes, String> {
    let name = "comfy-workflow-mapping.json";
    let mut raw = crate::read_contract_json(&root.join("packages/app-contracts/v1").join(name));
    crate::require_schema_v1(&raw, name);
    for (definition, source) in [("confirmationKind", "confirmationKinds"), ("evidenceKind", "evidenceKinds")] {
        let values = crate::require_str_array(&raw, source);
        raw["definitions"][definition] = serde_json::json!({"type":"string","enum":values});
    }
    schema_types::generate(&BTreeMap::from([(name.to_owned(), raw)]), name, &["promptProjectionOrigin"], "Comfy")
}
