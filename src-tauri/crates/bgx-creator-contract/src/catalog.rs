//! 模块：Creator 工具与规则渐进发现；保持同一合同、执行内核与离线可验证边界。
//! CT18: tool catalog parsing, required fields and wire-name injectivity (pure). The runtime
//! caches the result of [`CreatorCatalog::parse_and_verify`]; build.rs runs it first, so a bad
//! catalog fails `cargo check` instead of the first MCP request.

use crate::diagnostics::ContractViolation;
use crate::CATALOG_FILE;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogToolMeta {
    pub id: String,
    pub group: String,
    pub effect_class: String,
    pub billing_class: String,
    pub revision_required: bool,
    pub undo: String,
    pub available: bool,
    #[serde(default)]
    pub description: String,
    /// Catalog owner (S88: tool -> module mapping; manifests never copy tool ids).
    #[serde(default)]
    pub owner: String,
    /// Shared request association rule (closed set `requestPolicies`); same for every entry.
    pub request_policy: String,
    pub discovery: DiscoveryMeta,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DiscoveryMeta {
    pub summary: String,
    pub aliases: Vec<String>,
    pub required_rule_ids: Vec<String>,
    pub related_tool_ids: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CatalogFile {
    tools: Vec<CatalogToolMeta>,
    /// Main tabs that activate delivery tools (SSOT; was a Rust literal array).
    #[serde(default = "default_delivery_context_main_tabs")]
    delivery_context_main_tabs: Vec<String>,
}

fn default_delivery_context_main_tabs() -> Vec<String> {
    vec!["freeCanvas".into(), "chapters".into(), "segments".into(), "clips".into()]
}

/// Encode a canonical catalog/contract id for vendor wire (`story.change.prepare` ->
/// `story_change_prepare`). Pure transform; the one implementation (wire_names re-exports it).
pub fn encode_wire_name(canonical: &str) -> String {
    canonical.replace('.', "_")
}

/// Fail when two distinct canonical ids share one wire form (e.g. `a.b_c` vs `a_b.c`).
pub fn assert_wire_injective(known_ids: &[impl AsRef<str>]) -> Result<(), String> {
    let mut wire_to_id: HashMap<String, String> = HashMap::new();
    for id in known_ids {
        let id = id.as_ref().to_string();
        let wire = encode_wire_name(&id);
        if let Some(prev) = wire_to_id.insert(wire.clone(), id.clone()) {
            if prev != id {
                return Err(format!("WIRE_COLLISION: {prev} and {id} both encode to {wire}"));
            }
        }
    }
    Ok(())
}

/// The verified catalog.
#[derive(Debug, Clone)]
pub struct CreatorCatalog {
    file: CatalogFile,
}

impl CreatorCatalog {
    pub fn parse_and_verify(raw: &str) -> Result<Self, ContractViolation> {
        let v = |m: String| ContractViolation::new(CATALOG_FILE, "tools", m);
        let file: CatalogFile =
            serde_json::from_str(raw).map_err(|e| v(format!("CATALOG_PARSE: {e}")))?;
        let mut seen = BTreeSet::new();
        for t in &file.tools {
            for (field, value) in [
                ("id", &t.id),
                ("group", &t.group),
                ("effectClass", &t.effect_class),
                ("requestPolicy", &t.request_policy),
            ] {
                if value.trim().is_empty() {
                    return Err(v(format!("CATALOG_FIELD_EMPTY: {} {field}", t.id)));
                }
            }
            if !seen.insert(t.id.as_str()) {
                return Err(v(format!("CATALOG_DUPLICATE_ID: {}", t.id)));
            }
        }
        let published: BTreeSet<_> = file.tools.iter().filter(|t| t.available).map(|t| t.id.as_str()).collect();
        for t in &file.tools {
            if t.discovery.summary.trim().is_empty() { return Err(v(format!("DISCOVERY_EMPTY_SUMMARY: {}", t.id))); }
            for (field, values) in [("aliases", &t.discovery.aliases), ("requiredRuleIds", &t.discovery.required_rule_ids), ("relatedToolIds", &t.discovery.related_tool_ids)] {
                let unique: BTreeSet<_> = values.iter().collect();
                if unique.len() != values.len() || values.iter().any(|x| x.trim().is_empty()) { return Err(v(format!("DISCOVERY_INVALID_LIST: {} {field}",t.id))); }
            }
            for related in &t.discovery.related_tool_ids {
                if !published.contains(related.as_str()) { return Err(v(format!("DISCOVERY_UNKNOWN_RELATED_TOOL: {} {related}",t.id))); }
            }
        }
        let ids: Vec<&str> = file.tools.iter().map(|t| t.id.as_str()).collect();
        assert_wire_injective(&ids).map_err(v)?;
        if file.delivery_context_main_tabs.is_empty() {
            return Err(v("CATALOG_INVALID: deliveryContextMainTabs must not be empty".into()));
        }
        Ok(Self { file })
    }

    pub fn tools(&self) -> &[CatalogToolMeta] {
        &self.file.tools
    }

    pub fn delivery_context_main_tabs(&self) -> &[String] {
        &self.file.delivery_context_main_tabs
    }
}