//! Generates contract constants from `packages/app-contracts/v1/agent-skills.json` (INV-CONTRACT-SSOT).
//! Diagnostic codes, severities per profile, phases, budgets, status sets and digest constants are
//! never hand-mirrored in Rust: they are emitted into OUT_DIR and `include!`d by `src/lib.rs`.

use std::path::Path;

use bgx_contract_gen::{read_contract_json, require_schema_v1, write_generated};
use serde_json::Value;

fn camel(snake_or_upper: &str) -> String {
    snake_or_upper
        .split('_')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let lower = p.to_ascii_lowercase();
            let mut cs = lower.chars();
            match cs.next() {
                Some(f) => f.to_ascii_uppercase().to_string() + cs.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

fn upper_snake(camel_case: &str) -> String {
    let mut out = String::new();
    for (i, c) in camel_case.chars().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            out.push('_');
        }
        out.push(c.to_ascii_uppercase());
    }
    out
}

fn str_field<'a>(v: &'a Value, key: &str, hint: &str) -> &'a str {
    v.get(key)
        .and_then(|x| x.as_str())
        .unwrap_or_else(|| panic!("agent-skills.json {hint} missing string {key}"))
}

fn emit_status_enum(out: &mut String, type_name: &str, set: &Value, hint: &str) {
    let obj = set
        .as_object()
        .unwrap_or_else(|| panic!("agent-skills.json statusSets.{hint} must be an object"));
    if obj.is_empty() {
        panic!("agent-skills.json statusSets.{hint} must be non-empty");
    }
    let keys: Vec<&String> = obj.keys().collect();
    out.push_str(&format!(
        "/// Contract status set `statusSets.{hint}` (generated).\n#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]\npub enum {type_name} {{\n"
    ));
    for k in &keys {
        let meaning = obj[*k].as_str().unwrap_or("");
        out.push_str(&format!(
            "    /// {}\n    {},\n",
            meaning.replace('\n', " "),
            camel(k)
        ));
    }
    out.push_str("}\n\n");
    out.push_str(&format!("impl {type_name} {{\n"));
    out.push_str(&format!(
        "    pub const ALL: [{type_name}; {}] = [{}];\n",
        keys.len(),
        keys.iter()
            .map(|k| format!("{type_name}::{}", camel(k)))
            .collect::<Vec<_>>()
            .join(", ")
    ));
    out.push_str("    pub fn as_str(self) -> &'static str {\n        match self {\n");
    for k in &keys {
        out.push_str(&format!(
            "            {type_name}::{} => {:?},\n",
            camel(k),
            k
        ));
    }
    out.push_str("        }\n    }\n");
    out.push_str("    pub fn parse(s: &str) -> Option<Self> {\n        match s {\n");
    for k in &keys {
        out.push_str(&format!(
            "            {:?} => Some({type_name}::{}),\n",
            k,
            camel(k)
        ));
    }
    out.push_str("            _ => None,\n        }\n    }\n}\n\n");
}

fn resolve_path<'a>(root: &'a Value, dotted: &str, hint: &str) -> &'a Value {
    let mut cur = root;
    for part in dotted.split('.') {
        cur = cur
            .get(part)
            .unwrap_or_else(|| panic!("{hint}: path {dotted} not found"));
    }
    cur
}

fn u64_field(v: &Value, key: &str, hint: &str) -> u64 {
    v.get(key)
        .and_then(|x| x.as_u64())
        .filter(|n| *n > 0)
        .unwrap_or_else(|| panic!("{hint} missing positive integer {key}"))
}

fn emit_code_enum(out: &mut String, codes: &[(String, String, bool)]) {
    out.push_str("/// Worker/transport error codes (`workerErrorCodes` + `transportErrorCodes`, generated).\n#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]\npub enum WorkerErrorCode {\n");
    for (code, meaning, _) in codes {
        let short = code.strip_prefix("AGENT_SKILL_WORKER_").unwrap_or_else(|| {
            panic!("agent-skill-worker.json code {code} must start with AGENT_SKILL_WORKER_")
        });
        out.push_str(&format!(
            "    /// `{code}`: {meaning}\n    {},\n",
            camel(short)
        ));
    }
    out.push_str("}\n\nimpl WorkerErrorCode {\n");
    let variant = |c: &str| camel(c.strip_prefix("AGENT_SKILL_WORKER_").unwrap());
    out.push_str(&format!(
        "    pub const ALL: [WorkerErrorCode; {}] = [{}];\n",
        codes.len(),
        codes
            .iter()
            .map(|c| format!("WorkerErrorCode::{}", variant(&c.0)))
            .collect::<Vec<_>>()
            .join(", ")
    ));
    for (name, idx) in [("as_str", 0usize), ("meaning", 1usize)] {
        out.push_str(&format!(
            "    pub fn {name}(self) -> &'static str {{\n        match self {{\n"
        ));
        for c in codes {
            let val = if idx == 0 { &c.0 } else { &c.1 };
            out.push_str(&format!(
                "            WorkerErrorCode::{} => {:?},\n",
                variant(&c.0),
                val
            ));
        }
        out.push_str("        }\n    }\n");
    }
    out.push_str("    /// True for codes raised by the parent-side transport, never sent by the worker.\n    pub fn is_transport(self) -> bool {\n        match self {\n");
    for c in codes {
        out.push_str(&format!(
            "            WorkerErrorCode::{} => {},\n",
            variant(&c.0),
            c.2
        ));
    }
    out.push_str(
        "        }\n    }\n    pub fn parse(s: &str) -> Option<Self> {\n        match s {\n",
    );
    for c in codes {
        out.push_str(&format!(
            "            {:?} => Some(WorkerErrorCode::{}),\n",
            c.0,
            variant(&c.0)
        ));
    }
    out.push_str("            _ => None,\n        }\n    }\n}\n\n");
}

/// W00: constants for the SKILL worker stdio protocol (`agent-skill-worker.json`) with limits and
/// the import allowlist taken from `content-skill-limits.json` (single source, never mirrored).
fn generate_worker_contract(contracts_dir: &Path, out_dir: &Path) {
    let w = read_contract_json(&contracts_dir.join("agent-skill-worker.json"));
    require_schema_v1(&w, "agent-skill-worker.json");
    let hint = "agent-skill-worker.json";
    let ls = w
        .get("limitsSource")
        .expect("agent-skill-worker.json missing limitsSource");
    let lim_file = str_field(ls, "file", "limitsSource");
    let lim_name = lim_file
        .strip_prefix("packages/app-contracts/v1/")
        .unwrap_or_else(|| panic!("limitsSource.file must live in packages/app-contracts/v1"));
    let lim = read_contract_json(&contracts_dir.join(lim_name));
    require_schema_v1(&lim, lim_name);
    let lim_u64 = |key: &str| -> u64 {
        let path = str_field(ls, key, "limitsSource");
        resolve_path(&lim, path, lim_name)
            .as_u64()
            .filter(|n| *n > 0)
            .unwrap_or_else(|| panic!("{lim_name} {path} must be a positive integer"))
    };
    let allowed_path = str_field(ls, "allowedImports", "limitsSource");
    let allowed: Vec<String> = resolve_path(&lim, allowed_path, lim_name)
        .as_array()
        .unwrap_or_else(|| panic!("{lim_name} {allowed_path} must be an array"))
        .iter()
        .map(|x| {
            x.as_str()
                .unwrap_or_else(|| panic!("{lim_name} {allowed_path} entries must be strings"))
                .to_string()
        })
        .collect();
    if allowed.is_empty() {
        panic!("{lim_name} {allowed_path} must be non-empty");
    }
    let framing = w
        .get("framing")
        .expect("agent-skill-worker.json missing framing");
    let abi = w.get("abi").expect("agent-skill-worker.json missing abi");

    let mut out = String::from("// @generated by bgx-agent-skills-core/build.rs from packages/app-contracts/v1/agent-skill-worker.json and content-skill-limits.json. Do not edit.\n\n");
    let consts: [(&str, &str, u64, &str); 9] = [
        (
            "PROTOCOL_VERSION",
            "u32",
            u64_field(&w, "protocolVersion", hint),
            "protocolVersion",
        ),
        (
            "FRAME_MAX_BYTES",
            "usize",
            u64_field(framing, "frameMaxBytes", hint),
            "framing.frameMaxBytes",
        ),
        (
            "COMPONENT_MAX_BYTES",
            "usize",
            u64_field(&w, "componentMaxBytes", hint),
            "componentMaxBytes",
        ),
        (
            "STDERR_TAIL_MAX_CHARS",
            "usize",
            u64_field(&w, "stderrTailMaxChars", hint),
            "stderrTailMaxChars",
        ),
        (
            "HELLO_WALL_MS",
            "u64",
            u64_field(&w, "helloWallMs", hint),
            "helloWallMs",
        ),
        (
            "CANCEL_GRACE_MS",
            "u64",
            u64_field(&w, "cancelGraceMs", hint),
            "cancelGraceMs",
        ),
        (
            "SHUTDOWN_GRACE_MS",
            "u64",
            u64_field(&w, "shutdownGraceMs", hint),
            "shutdownGraceMs",
        ),
        (
            "EPOCH_TICK_MS",
            "u64",
            u64_field(&w, "epochTickMs", hint),
            "epochTickMs",
        ),
        (
            "MAX_INFLIGHT_REQUESTS",
            "usize",
            u64_field(&w, "maxInflightRequests", hint),
            "maxInflightRequests",
        ),
    ];
    for (name, ty, val, key) in consts {
        out.push_str(&format!("/// `{key}`\npub const {name}: {ty} = {val};\n"));
    }
    for (name, ty, key) in [
        ("WASM_MEMORY_BYTES", "usize", "memoryBytes"),
        ("WASM_FUEL", "u64", "fuel"),
        ("WASM_WALL_MS", "u64", "wallMs"),
        ("INPUT_MAX_BYTES", "usize", "inputMaxBytes"),
        ("OUTPUT_MAX_BYTES", "usize", "outputMaxBytes"),
    ] {
        out.push_str(&format!(
            "/// `{lim_name}` {} (via `limitsSource.{key}`)\npub const {name}: {ty} = {};\n",
            str_field(ls, key, "limitsSource"),
            lim_u64(key)
        ));
    }
    out.push_str(&format!(
        "/// `{lim_name}` {allowed_path}\npub const ALLOWED_IMPORTS: &[&str] = &{:?};\n",
        allowed
    ));
    out.push_str(&format!(
        "/// `abi.world` (helper-compat ABI, not a generic SKILL ABI)\npub const ABI_WORLD: &str = {:?};\n\n",
        str_field(abi, "world", "abi")
    ));

    let mut codes: Vec<(String, String, bool)> = Vec::new();
    for (key, transport) in [("workerErrorCodes", false), ("transportErrorCodes", true)] {
        let m = w
            .get(key)
            .and_then(|x| x.as_object())
            .unwrap_or_else(|| panic!("agent-skill-worker.json missing {key}"));
        for (code, meaning) in m {
            if codes.iter().any(|c| &c.0 == code) {
                panic!("agent-skill-worker.json duplicate code {code}");
            }
            let meaning = meaning
                .as_str()
                .unwrap_or_else(|| panic!("{key}.{code} must be a string"));
            codes.push((code.clone(), meaning.to_string(), transport));
        }
    }
    emit_code_enum(&mut out, &codes);

    for (key, name) in [
        ("parentToWorker", "PARENT_MESSAGES"),
        ("workerToParent", "WORKER_MESSAGES"),
    ] {
        let m = w
            .get(key)
            .and_then(|x| x.as_object())
            .unwrap_or_else(|| panic!("agent-skill-worker.json missing {key}"));
        let items: Vec<String> = m
            .iter()
            .map(|(t, fields)| {
                let fs: Vec<String> = fields
                    .as_array()
                    .unwrap_or_else(|| panic!("{key}.{t} must be an array"))
                    .iter()
                    .map(|f| format!("{:?}", f.as_str().expect("field names are strings")))
                    .collect();
                format!("({t:?}, &[{}])", fs.join(", "))
            })
            .collect();
        out.push_str(&format!(
            "/// `{key}` message types with their required fields.\npub const {name}: &[(&str, &[&str])] = &[{}];\n",
            items.join(", ")
        ));
    }
    write_generated(&out_dir.join("agent_skill_worker_contract.rs"), &out);
}

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let contract_path = Path::new(&manifest_dir)
        .join("..")
        .join("..")
        .join("..")
        .join("packages")
        .join("app-contracts")
        .join("v1")
        .join("agent-skills.json");
    let c = read_contract_json(&contract_path);
    require_schema_v1(&c, "agent-skills.json");

    let mut out = String::from("// @generated by bgx-agent-skills-core/build.rs from packages/app-contracts/v1/agent-skills.json. Do not edit.\n\n");

    // Status sets (closed enums).
    let sets = c
        .get("statusSets")
        .expect("agent-skills.json missing statusSets");
    for (key, ty) in [
        ("source", "Source"),
        ("profile", "Profile"),
        ("severity", "Severity"),
        ("phase", "Phase"),
        ("formatStatus", "FormatStatus"),
        ("installState", "InstallState"),
        ("executionReadiness", "ExecutionReadiness"),
        ("limitationKind", "LimitationKind"),
        ("activationSource", "ActivationSource"),
        ("resourceKind", "ResourceKind"),
    ] {
        let set = sets
            .get(key)
            .unwrap_or_else(|| panic!("agent-skills.json missing statusSets.{key}"));
        emit_status_enum(&mut out, ty, set, key);
    }
    for (key, required) in [
        ("profile", ["import", "author"].as_slice()),
        ("severity", ["error", "warning", "info"].as_slice()),
        (
            "formatStatus",
            ["valid", "valid_with_warnings", "invalid"].as_slice(),
        ),
        ("resourceKind", ["text", "binary"].as_slice()),
        ("source", ["builtin", "user"].as_slice()),
    ] {
        for r in required {
            if sets[key].get(*r).is_none() {
                panic!("agent-skills.json statusSets.{key} must contain {r} (core logic depends on it)");
            }
        }
    }

    // Diagnostics.
    let diags = c
        .get("diagnostics")
        .and_then(|d| d.as_array())
        .expect("agent-skills.json missing diagnostics array");
    if diags.is_empty() {
        panic!("agent-skills.json diagnostics must be non-empty");
    }
    let mut variants: Vec<(String, String, String, String, String, String)> = Vec::new();
    for d in diags {
        let code = str_field(d, "code", "diagnostics[]");
        let short = code
            .strip_prefix("AGENT_SKILL_")
            .unwrap_or_else(|| panic!("diagnostic code {code} must start with AGENT_SKILL_"));
        let phase = str_field(d, "phase", code);
        if sets["phase"].get(phase).is_none() {
            panic!("diagnostic {code} phase {phase} not in statusSets.phase");
        }
        let sev = d
            .get("severity")
            .unwrap_or_else(|| panic!("diagnostic {code} missing severity"));
        let imp = str_field(sev, "import", code);
        let aut = str_field(sev, "author", code);
        for s in [imp, aut] {
            if sets["severity"].get(s).is_none() {
                panic!("diagnostic {code} severity {s} not in statusSets.severity");
            }
        }
        let meaning = str_field(d, "meaning", code);
        variants.push((
            camel(short),
            code.to_string(),
            camel(phase),
            camel(imp),
            camel(aut),
            meaning.to_string(),
        ));
    }
    out.push_str("/// Contract diagnostic codes (`diagnostics[]`, generated). Severity per profile and phase come from the contract.\n#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]\npub enum DiagnosticCode {\n");
    for v in &variants {
        out.push_str(&format!(
            "    /// `{}`: {}\n    {},\n",
            v.1,
            v.5.replace('\n', " "),
            v.0
        ));
    }
    out.push_str("}\n\nimpl DiagnosticCode {\n");
    out.push_str(&format!(
        "    pub const ALL: [DiagnosticCode; {}] = [{}];\n",
        variants.len(),
        variants
            .iter()
            .map(|v| format!("DiagnosticCode::{}", v.0))
            .collect::<Vec<_>>()
            .join(", ")
    ));
    out.push_str("    pub fn as_str(self) -> &'static str {\n        match self {\n");
    for v in &variants {
        out.push_str(&format!(
            "            DiagnosticCode::{} => {:?},\n",
            v.0, v.1
        ));
    }
    out.push_str(
        "        }\n    }\n    pub fn parse(s: &str) -> Option<Self> {\n        match s {\n",
    );
    for v in &variants {
        out.push_str(&format!(
            "            {:?} => Some(DiagnosticCode::{}),\n",
            v.1, v.0
        ));
    }
    out.push_str("            _ => None,\n        }\n    }\n    pub fn phase(self) -> Phase {\n        match self {\n");
    for v in &variants {
        out.push_str(&format!(
            "            DiagnosticCode::{} => Phase::{},\n",
            v.0, v.2
        ));
    }
    out.push_str("        }\n    }\n    pub fn severity(self, profile: Profile) -> Severity {\n        match (self, profile) {\n");
    for v in &variants {
        out.push_str(&format!(
            "            (DiagnosticCode::{}, Profile::Import) => Severity::{},\n            (DiagnosticCode::{}, Profile::Author) => Severity::{},\n",
            v.0, v.3, v.0, v.4
        ));
    }
    out.push_str("        }\n    }\n    /// Contract `meaning` (Chinese, human readable); used as default message.\n    pub fn meaning(self) -> &'static str {\n        match self {\n");
    for v in &variants {
        out.push_str(&format!(
            "            DiagnosticCode::{} => {:?},\n",
            v.0, v.5
        ));
    }
    out.push_str("        }\n    }\n}\n\n");

    // limitationKind -> diagnostic code mapping.
    let lim = c
        .get("limitationDiagnostic")
        .and_then(|x| x.as_object())
        .expect("agent-skills.json missing limitationDiagnostic");
    out.push_str("impl LimitationKind {\n    pub fn diagnostic_code(self) -> DiagnosticCode {\n        match self {\n");
    for (k, v) in lim {
        let code = v
            .as_str()
            .expect("limitationDiagnostic values must be strings");
        let short = code
            .strip_prefix("AGENT_SKILL_")
            .expect("limitationDiagnostic code prefix");
        if !variants.iter().any(|x| x.1 == code) {
            panic!("limitationDiagnostic {k} -> {code} not in diagnostics");
        }
        out.push_str(&format!(
            "            LimitationKind::{} => DiagnosticCode::{},\n",
            camel(k),
            camel(short)
        ));
    }
    out.push_str("        }\n    }\n}\n\n");

    // Budgets.
    let budgets = c
        .get("budgets")
        .and_then(|x| x.as_object())
        .expect("agent-skills.json missing budgets");
    out.push_str("/// Contract budgets (`budgets.*.value`, generated).\npub mod budgets {\n");
    for (k, v) in budgets {
        let n = v
            .get("value")
            .and_then(|x| x.as_u64())
            .unwrap_or_else(|| panic!("budgets.{k}.value must be a non-negative integer"));
        let unit = v.get("unit").and_then(|x| x.as_str()).unwrap_or("");
        out.push_str(&format!(
            "    /// `budgets.{k}` ({unit})\n    pub const {}: u64 = {n};\n    pub const {}_KEY: &str = {:?};\n",
            upper_snake(k),
            upper_snake(k),
            k
        ));
    }
    out.push_str("}\n\n");

    // packageDigest constants.
    let pd = c
        .get("packageDigest")
        .expect("agent-skills.json missing packageDigest");
    out.push_str(&format!(
        "/// `packageDigest.algorithm`\npub const DIGEST_ALGORITHM: &str = {:?};\n/// `packageDigest.prefix`\npub const DIGEST_PREFIX: &str = {:?};\n/// `packageDigest.domainSeparator`\npub const DIGEST_DOMAIN_SEPARATOR: &str = {:?};\n",
        str_field(pd, "algorithm", "packageDigest"),
        str_field(pd, "prefix", "packageDigest"),
        str_field(pd, "domainSeparator", "packageDigest"),
    ));

    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR");
    write_generated(&Path::new(&out_dir).join("agent_skills_contract.rs"), &out);
    generate_worker_contract(
        contract_path.parent().expect("contracts dir"),
        Path::new(&out_dir),
    );
}
