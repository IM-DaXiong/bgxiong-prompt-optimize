//! Frozen reference inventory row for one projection compile (pure data).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReferenceMediaInventoryItem {
    pub token: String,
    pub media_kind: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub binding_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_kind: Option<String>,
    /// app_data relative path; None when locatorKind !== "path" (INV-PROMPT-OPT-VL-12).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_relative_path: Option<String>,
    /// H3 reference-audio usage (`voice_timbre` | `soundtrack_copy`). Audio rows only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_usage: Option<String>,
    /// Stable id for one reference row. Empty when the host did not assign one.
    #[serde(default)]
    pub item_id: String,
    /// PadRef entity kind when the host declared one: role, actor, scene, sa, prop.
    #[serde(default)]
    pub declared_entity_kind: String,
    /// Full declared entity. Kind without entity id is rejected before any model call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub declared_identity: Option<DeclaredIdentityV1>,
    /// 1-based ordinal inside media kind. 0 means the freeze step assigns it.
    #[serde(default)]
    pub ordinal_within_kind: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeclaredIdentityV1 {
    pub entity_kind: String,
    pub entity_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inventory_item_roundtrip_json() {
        let item = ReferenceMediaInventoryItem {
            token: "@Image1".into(),
            media_kind: "reference_image".into(),
            label: "Alice".into(),
            binding_id: Some("b1".into()),
            source_kind: None,
            media_relative_path: None,
            audio_usage: None,
            item_id: "ref1".into(),
            declared_entity_kind: "role".into(),
            declared_identity: Some(DeclaredIdentityV1 {
                entity_kind: "role".into(),
                entity_id: "r1".into(),
            }),
            ordinal_within_kind: 1,
        };
        let v = serde_json::to_value(&item).expect("ser");
        let back: ReferenceMediaInventoryItem = serde_json::from_value(v).expect("de");
        assert_eq!(back, item);
    }
}
