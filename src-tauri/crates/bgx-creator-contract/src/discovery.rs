//! 模块：Creator 工具与规则渐进发现；保持同一合同、执行内核与离线可验证边界。
//! Pure discovery configuration and reference validation, used by build, CLI and host.
use crate::CreatorCatalog;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Bm25Config {
    pub k1: f64,
    pub b: f64,
    pub tool_id_weight: f64,
    pub aliases_weight: f64,
    pub summary_weight: f64,
    pub description_weight: f64,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DiscoveryConfig {
    pub schema_version: u32,
    pub bootstrap_tool_ids: Vec<String>,
    #[serde(default)]
    pub navigation_read_tools: BTreeMap<String, String>,
    pub core_rule_ids: Vec<String>,
    pub rule_ids: Vec<String>,
    pub max_page_bytes: usize,
    pub max_search_page_bytes: usize,
    pub max_rule_page_bytes: usize,
    pub max_schema_tool_ids: usize,
    pub max_loaded_tool_ids: usize,
    pub max_active_tool_bytes: usize,
    pub max_active_rule_bytes: usize,
    pub max_cache_bytes: usize,
    pub max_cache_entries: usize,
    pub bm25: Bm25Config,
    pub cursor_version: u32,
    pub revision_domains: Vec<String>,
    pub error_codes: Vec<String>,
}
impl DiscoveryConfig {
    pub fn parse(raw: &str) -> Result<Self, String> {
        let c: Self = serde_json::from_str(raw)
            .map_err(|e| format!("CREATOR_CONTRACT_INVALID: discovery: {e}"))?;
        if c.schema_version != 1 || c.cursor_version != 1 {
            return Err("CREATOR_CONTRACT_INVALID: discovery version".into());
        }
        for n in [
            c.max_search_page_bytes,
            c.max_page_bytes,
            c.max_rule_page_bytes,
            c.max_schema_tool_ids,
            c.max_loaded_tool_ids,
            c.max_active_tool_bytes,
            c.max_active_rule_bytes,
            c.max_cache_bytes,
            c.max_cache_entries,
        ] {
            if n == 0 {
                return Err("CREATOR_CONTRACT_INVALID: discovery zero budget".into());
            }
        }
        if !c.bm25.k1.is_finite()
            || c.bm25.k1 <= 0.0
            || !c.bm25.b.is_finite()
            || !(0.0..=1.0).contains(&c.bm25.b)
            || [
                c.bm25.tool_id_weight,
                c.bm25.aliases_weight,
                c.bm25.summary_weight,
                c.bm25.description_weight,
            ]
            .iter()
            .any(|w| !w.is_finite() || *w <= 0.0)
        {
            return Err("CREATOR_CONTRACT_INVALID: BM25 parameters".into());
        }
        for list in [
            &c.bootstrap_tool_ids,
            &c.core_rule_ids,
            &c.rule_ids,
            &c.revision_domains,
            &c.error_codes,
        ] {
            if list.is_empty()
                || list.iter().any(|s| s.trim().is_empty())
                || list.iter().collect::<BTreeSet<_>>().len() != list.len()
            {
                return Err("CREATOR_CONTRACT_INVALID: discovery duplicate/empty list".into());
            }
        }
        Ok(c)
    }
    pub fn verify_schema(&self, raw: &str, schema_raw: &str) -> Result<(), String> {
        let root: serde_json::Value =
            serde_json::from_str(schema_raw).map_err(|e| e.to_string())?;
        crate::verify_schema_document(&crate::SchemaDocument {
            file: "creator-tool-discovery.schema.json",
            root: &root,
            external: &|_: &str| None,
        })?;
        let instance: serde_json::Value = serde_json::from_str(raw).map_err(|e| e.to_string())?;
        crate::validate_schema_value(
            "creator-tool-discovery.json",
            "configuration",
            &root["definitions"]["configuration"],
            &instance,
            "$",
        )
        .map_err(|e| e.to_string())?;
        for (path, expected) in [
            (
                "/definitions/getInput/oneOf/1/properties/schemaToolIds/maxItems",
                self.max_schema_tool_ids,
            ),
            (
                "/definitions/searchInput/oneOf/0/properties/loadedToolIds/maxItems",
                self.max_loaded_tool_ids,
            ),
        ] {
            if root.pointer(path).and_then(|v| v.as_u64()) != Some(expected as u64) {
                return Err(format!(
                    "CREATOR_CONTRACT_INVALID: discovery schema budget drift {path}"
                ));
            }
        }
        Ok(())
    }
    pub fn verify(
        &self,
        catalog: &CreatorCatalog,
        rules: &serde_json::Value,
        errors: &serde_json::Value,
    ) -> Result<(), String> {
        let fail = |s: String| format!("CREATOR_CONTRACT_INVALID: discovery: {s}");
        let tools: BTreeSet<_> = catalog
            .tools()
            .iter()
            .filter(|t| t.available)
            .map(|t| t.id.as_str())
            .collect();
        for id in &self.bootstrap_tool_ids {
            if !tools.contains(id.as_str()) {
                return Err(fail(format!("unknown bootstrap {id}")));
            }
        }
        for (tab, id) in &self.navigation_read_tools {
            let read = catalog.tools().iter().any(|tool| tool.available && &tool.id == id
                && matches!(tool.effect_class.as_str(), "query" | "read" | "observe"));
            if tab.trim().is_empty() || !read {
                return Err(fail(format!("invalid navigation read tool {id} for {tab}")));
            }
        }
        let entries = rules
            .get("rules")
            .and_then(|v| v.as_array())
            .ok_or_else(|| fail("missing rules".into()))?;
        let mut deps = BTreeMap::<String, Vec<String>>::new();
        for r in entries {
            let id = r
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| fail("missing rule id".into()))?;
            let ds = r
                .get("dependencies")
                .and_then(|v| v.as_array())
                .ok_or_else(|| fail(format!("missing dependencies {id}")))?
                .iter()
                .map(|v| {
                    v.as_str()
                        .map(str::to_string)
                        .ok_or_else(|| fail(format!("invalid dependency {id}")))
                })
                .collect::<Result<Vec<_>, _>>()?;
            let source = r
                .get("source")
                .and_then(|v| v.as_object())
                .ok_or_else(|| fail(format!("missing rule source {id}")))?;
            let kind = source
                .get("kind")
                .and_then(|v| v.as_str())
                .ok_or_else(|| fail(format!("missing rule source kind {id}")))?;
            let allowed = if kind == "static" {
                vec!["kind", "text"]
            } else if kind == "internal_material" {
                vec!["kind", "task"]
            } else {
                vec!["kind"]
            };
            if source.keys().any(|k| !allowed.contains(&k.as_str()))
                || r.as_object().is_none_or(|o| {
                    o.keys()
                        .any(|k| !matches!(k.as_str(), "id" | "dependencies" | "source"))
                })
            {
                return Err(fail(format!("unknown rule field {id}")));
            }
            match kind {
                "static" => {
                    if source
                        .get("text")
                        .and_then(|v| v.as_str())
                        .is_none_or(|s| s.trim().is_empty())
                    {
                        return Err(fail(format!("empty static rule {id}")));
                    }
                }
                "internal_material" => {
                    let task = source
                        .get("task")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| fail(format!("internal material task missing {id}")))?;
                    if task.is_empty()
                        || !task
                            .bytes()
                            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                    {
                        return Err(fail(format!("invalid internal material task {id}")));
                    }
                }
                "runtime_intake" | "scene_replan" => {}
                _ => return Err(fail(format!("unknown rule source {kind}"))),
            }
            if deps.insert(id.into(), ds).is_some() {
                return Err(fail(format!("duplicate rule {id}")));
            }
        }
        if deps.keys().collect::<BTreeSet<_>>() != self.rule_ids.iter().collect::<BTreeSet<_>>() {
            return Err(fail("rule id closed set mismatch".into()));
        }
        fn visit(
            id: &str,
            deps: &BTreeMap<String, Vec<String>>,
            stack: &mut BTreeSet<String>,
            done: &mut BTreeSet<String>,
        ) -> Result<(), String> {
            if done.contains(id) {
                return Ok(());
            }
            if !stack.insert(id.into()) {
                return Err(format!("rule dependency cycle {id}"));
            }
            let children = deps.get(id).ok_or_else(|| format!("unknown rule {id}"))?;
            if children.iter().collect::<BTreeSet<_>>().len() != children.len() {
                return Err(format!("duplicate rule dependency {id}"));
            }
            for c in children {
                visit(c, deps, stack, done)?;
            }
            stack.remove(id);
            done.insert(id.into());
            Ok(())
        }
        let mut done = BTreeSet::new();
        for id in deps.keys() {
            visit(id, &deps, &mut BTreeSet::new(), &mut done).map_err(&fail)?;
        }
        for id in &self.core_rule_ids {
            if !deps.contains_key(id) {
                return Err(fail(format!("unknown core rule {id}")));
            }
        }
        for t in catalog.tools() {
            for r in &t.discovery.required_rule_ids {
                if !deps.contains_key(r) {
                    return Err(fail(format!("unknown rule {} {r}", t.id)));
                }
            }
        }
        let codes = errors
            .get("errors")
            .and_then(|v| v.as_array())
            .ok_or_else(|| fail("missing errors".into()))?;
        for id in &self.error_codes {
            if !codes
                .iter()
                .any(|v| v.get("code").and_then(|v| v.as_str()) == Some(id.as_str()))
            {
                return Err(fail(format!("unknown error {id}")));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn documents() -> (String, CreatorCatalog, serde_json::Value, serde_json::Value) {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../packages/app-contracts/v1");
        let read = |name: &str| std::fs::read_to_string(root.join(name)).unwrap();
        (
            read("creator-tool-discovery.json"),
            CreatorCatalog::parse_and_verify(&read("creator-tool-catalog.json")).unwrap(),
            serde_json::from_str(&read("creator-tool-rules.json")).unwrap(),
            serde_json::from_str(&read("creator-tool-errors.json")).unwrap(),
        )
    }
    #[test]
    fn discovery_references_and_budget_fixture() {
        let (raw, c, r, e) = documents();
        DiscoveryConfig::parse(&raw)
            .unwrap()
            .verify(&c, &r, &e)
            .unwrap();
    }
    #[test]
    fn discovery_bad_config_fails_before_runtime() {
        let (raw, _, _, _) = documents();
        for (key, value) in [
            ("schemaVersion", serde_json::json!(99)),
            ("maxPageBytes", serde_json::json!(0)),
            ("cursorVersion", serde_json::json!(2)),
        ] {
            let mut c: serde_json::Value = serde_json::from_str(&raw).unwrap();
            c[key] = value;
            assert!(DiscoveryConfig::parse(&c.to_string()).is_err());
        }
    }
    #[test]
    fn discovery_unknown_rule_cycle_and_bad_sources_fail() {
        let (raw, c, r, e) = documents();
        let conf = DiscoveryConfig::parse(&raw).unwrap();
        let mut broken = r.clone();
        broken["rules"][0]["dependencies"] = serde_json::json!(["unknown"]);
        assert!(conf.verify(&c, &broken, &e).is_err());
        broken = r.clone();
        let id = broken["rules"][0]["id"].clone();
        broken["rules"][0]["dependencies"] = serde_json::json!([id]);
        assert!(conf.verify(&c, &broken, &e).is_err());
        broken = r.clone();
        broken["rules"][0]["source"] = serde_json::json!({"kind":"static","text":""});
        assert!(conf.verify(&c, &broken, &e).is_err());
        broken = r.clone();
        broken["rules"][0]["source"] = serde_json::json!({"kind":"unknown"});
        assert!(conf.verify(&c, &broken, &e).is_err());
        for source in [
            serde_json::json!({"kind":"internal_material"}),
            serde_json::json!({"kind":"internal_material","task":"../outside"}),
            serde_json::json!({"kind":"internal_material","task":"creator-execute","text":"second source"}),
        ] {
            broken = r.clone();
            broken["rules"][0]["source"] = source;
            assert!(conf.verify(&c, &broken, &e).is_err());
        }
    }
    #[test]
    fn discovery_unique_items_is_enforced_by_shared_schema_checker() {
        let schema =
            serde_json::json!({"type":"array","uniqueItems":true,"items":{"type":"string"}});
        assert!(crate::validate_schema_value(
            "fixture",
            "unique",
            &schema,
            &serde_json::json!(["a", "a"]),
            "$"
        )
        .is_err());
        assert!(crate::validate_schema_value(
            "fixture",
            "unique",
            &schema,
            &serde_json::json!(["a", "b"]),
            "$"
        )
        .is_ok());
    }
}
