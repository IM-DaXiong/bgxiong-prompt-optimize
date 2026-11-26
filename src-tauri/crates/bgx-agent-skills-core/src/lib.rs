//! Pure core for standard Agent Skills packages (agentskills.io author format).
//!
//! Bytes and path values in, plain values out. This crate never touches the file system,
//! network, processes, Tauri, the database or Wasmtime (crate role `pure-core`, see
//! `src-tauri/crates/crate-roles.json`). Rules follow
//! `docs/dev/specs/20261002-SPEC-Agent-Skills包加载与执行合同.md`; diagnostic codes, per-profile
//! severities, phases, budgets and status sets are generated from
//! `packages/app-contracts/v1/agent-skills.json` by `build.rs` (INV-CONTRACT-SSOT).

use std::collections::BTreeMap;

include!(concat!(env!("OUT_DIR"), "/agent_skills_contract.rs"));

/// One rule violation found by the core, independent of the strictness profile.
///
/// The same finding is reported in both profiles; its severity comes from the contract
/// (`DiagnosticCode::severity(profile)`), never from the call site.
#[derive(Clone, Debug, PartialEq)]
pub struct Finding {
    pub code: DiagnosticCode,
    /// Human readable message (defaults to the contract `meaning`).
    pub message: String,
    /// Package-relative path (a valid resourceRef) the finding is about, if any.
    pub path: Option<String>,
    /// Structured details (`field`, `line`, `limitKey`, ...).
    pub details: BTreeMap<String, serde_json::Value>,
}

impl Finding {
    pub fn new(code: DiagnosticCode) -> Self {
        Finding {
            code,
            message: code.meaning().to_string(),
            path: None,
            details: BTreeMap::new(),
        }
    }

    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = message.into();
        self
    }

    pub fn with_path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    pub fn with_detail(mut self, key: &str, value: impl Into<serde_json::Value>) -> Self {
        self.details.insert(key.to_string(), value.into());
        self
    }

    /// Frontmatter field the finding is about (`details.field`).
    pub fn with_field(self, field: &str) -> Self {
        self.with_detail("field", field)
    }

    pub fn field(&self) -> Option<&str> {
        self.details.get("field").and_then(|v| v.as_str())
    }

    pub fn severity(&self, profile: Profile) -> Severity {
        self.code.severity(profile)
    }

    /// Contract-shaped diagnostic (`agent-skills.json#/definitions/diagnostic`).
    pub fn to_diagnostic_json(&self, profile: Profile) -> serde_json::Value {
        let mut m = serde_json::Map::new();
        m.insert("code".into(), self.code.as_str().into());
        m.insert("severity".into(), self.severity(profile).as_str().into());
        m.insert("phase".into(), self.code.phase().as_str().into());
        m.insert("message".into(), self.message.clone().into());
        if let Some(p) = &self.path {
            m.insert("path".into(), p.clone().into());
        }
        if !self.details.is_empty() {
            let d: serde_json::Map<String, serde_json::Value> =
                self.details.clone().into_iter().collect();
            m.insert("details".into(), serde_json::Value::Object(d));
        }
        serde_json::Value::Object(m)
    }
}

/// `formatStatus` for a set of findings under one profile (SPEC section 7).
pub fn format_status(findings: &[Finding], profile: Profile) -> FormatStatus {
    let mut warn = false;
    for f in findings {
        match f.severity(profile) {
            Severity::Error => return FormatStatus::Invalid,
            Severity::Warning => warn = true,
            Severity::Info => {}
        }
    }
    if warn {
        FormatStatus::ValidWithWarnings
    } else {
        FormatStatus::Valid
    }
}

/// Lowercase hex of SHA-256 over `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let d = Sha256::digest(bytes);
    let mut s = String::with_capacity(64);
    for b in d.iter() {
        s.push(char::from_digit((b >> 4) as u32, 16).unwrap_or('0'));
        s.push(char::from_digit((b & 0x0f) as u32, 16).unwrap_or('0'));
    }
    s
}

pub mod digest;
pub mod frontmatter;
pub mod identity;
pub mod resource_path;
pub mod resources;
pub mod validation;
pub mod verify;
pub mod worker_protocol;

/// The exact file name of the package entry point (case-sensitive, SPEC section 2).
pub const SKILL_MD: &str = "SKILL.md";
