//! P06: case runner shared by the `bgx-agent-skills-verify` CLI and the cargo tests.
//!
//! Input is a JSON document `{ "cases": [ ... ] }` (see
//! `tests/fixtures/agent-skills/frontmatter-cases.json`). Every case is checked with the same
//! core verifier ([`crate::validation::check_skill_md`]); the output lists only case ids, statuses
//! and path/field diagnostics, never file contents.

use serde::Deserialize;

use crate::validation::check_skill_md;
use crate::{FormatStatus, Profile};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CasesDoc {
    #[serde(default)]
    pub schema: Option<String>,
    #[serde(default)]
    pub purpose: Option<String>,
    #[serde(default)]
    pub spec_doc: Option<String>,
    pub cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Case {
    pub id: String,
    /// SPEC section 13 case id, when the case implements one.
    #[serde(default)]
    pub spec: Option<String>,
    #[serde(default)]
    pub why: Option<String>,
    /// Container name for the name/directory rule; `null` skips that rule.
    #[serde(default)]
    pub container_name: Option<String>,
    /// `SKILL.md` text (UTF-8). Exactly one of `skillMd` / `skillMdHex` is required.
    #[serde(default)]
    pub skill_md: Option<String>,
    /// `SKILL.md` raw bytes as hex (for non-UTF-8 or BOM cases).
    #[serde(default)]
    pub skill_md_hex: Option<String>,
    pub expect: Expect,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Expect {
    /// Expected `formatStatus` under the import profile.
    #[serde(default)]
    pub import: Option<String>,
    /// Expected `formatStatus` under the author profile.
    #[serde(default)]
    pub author: Option<String>,
    /// Exact set of diagnostic codes (order-free, duplicates ignored).
    #[serde(default)]
    pub codes: Option<Vec<String>>,
    /// User-facing package: the author profile must not be `invalid`.
    #[serde(default)]
    pub author_usable: Option<bool>,
}

/// One line of verifier output plus the verdict.
#[derive(Debug, PartialEq)]
pub struct CaseResult {
    pub id: String,
    pub passed: bool,
    pub negative: bool,
    pub lines: Vec<String>,
}

#[derive(Debug, PartialEq)]
pub struct RunSummary {
    pub results: Vec<CaseResult>,
    pub errors: Vec<String>,
}

impl RunSummary {
    pub fn passed(&self) -> usize {
        self.results.iter().filter(|r| r.passed).count()
    }
    pub fn failed(&self) -> usize {
        self.results.len() - self.passed()
    }
    pub fn negative(&self) -> usize {
        self.results.iter().filter(|r| r.negative).count()
    }
    /// `true` only when there is at least one case, no input error and no failing case.
    pub fn ok(&self) -> bool {
        self.errors.is_empty() && !self.results.is_empty() && self.failed() == 0
    }
    pub fn summary_line(&self) -> String {
        format!(
            "AGENT_SKILLS_FORMAT cases={} passed={} failed={} positive={} negative={} errors={}",
            self.results.len(),
            self.passed(),
            self.failed(),
            self.results.len() - self.negative(),
            self.negative(),
            self.errors.len()
        )
    }
    pub fn render(&self) -> String {
        let mut out = String::new();
        for e in &self.errors {
            out.push_str(&format!("ERROR {e}\n"));
        }
        for r in &self.results {
            for l in &r.lines {
                out.push_str(l);
                out.push('\n');
            }
        }
        out.push_str(&self.summary_line());
        out.push('\n');
        out
    }
}

fn decode_hex(s: &str) -> Option<Vec<u8>> {
    let digits: Vec<u8> = s.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    digits
        .chunks(2)
        .map(|pair| match pair {
            [hi, lo] => Some((hex_value(*hi)? << 4) | hex_value(*lo)?),
            _ => None,
        })
        .collect()
}

fn hex_value(b: u8) -> Option<u8> {
    char::from(b)
        .to_digit(16)
        .and_then(|d| u8::try_from(d).ok())
}

fn one_line(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

fn run_case(c: &Case) -> Result<CaseResult, String> {
    let bytes: Vec<u8> = match (&c.skill_md, &c.skill_md_hex) {
        (Some(t), None) => t.as_bytes().to_vec(),
        (None, Some(h)) => {
            decode_hex(h).ok_or_else(|| format!("{}: skillMdHex is not hex", c.id))?
        }
        _ => {
            return Err(format!(
                "{}: exactly one of skillMd / skillMdHex is required",
                c.id
            ))
        }
    };
    let e = &c.expect;
    if e.import.is_none() && e.author.is_none() && e.codes.is_none() && e.author_usable.is_none() {
        return Err(format!("{}: expect is empty", c.id));
    }
    for s in [&e.import, &e.author].into_iter().flatten() {
        if FormatStatus::parse(s).is_none() {
            return Err(format!("{}: unknown formatStatus {s}", c.id));
        }
    }
    let check = check_skill_md(&bytes, c.container_name.as_deref());
    let imp = check.format_status(Profile::Import);
    let aut = check.format_status(Profile::Author);
    let mut got: Vec<&str> = check.findings.iter().map(|f| f.code.as_str()).collect();
    got.sort_unstable();
    got.dedup();
    let mut problems = Vec::new();
    if let Some(want) = &e.import {
        if want != imp.as_str() {
            problems.push(format!("import={} expected {want}", imp.as_str()));
        }
    }
    if let Some(want) = &e.author {
        if want != aut.as_str() {
            problems.push(format!("author={} expected {want}", aut.as_str()));
        }
    }
    if let Some(want) = &e.codes {
        let mut w: Vec<&str> = want.iter().map(|s| s.as_str()).collect();
        w.sort_unstable();
        w.dedup();
        if w != got {
            problems.push(format!("codes={got:?} expected {w:?}"));
        }
    }
    if e.author_usable == Some(true) && aut == FormatStatus::Invalid {
        problems.push("author=invalid but the package is user-facing".to_string());
    }
    let negative = e.import.as_deref() == Some("invalid") || e.author.as_deref() == Some("invalid");
    let passed = problems.is_empty();
    let mut lines = vec![format!(
        "CASE {} import={} author={} result={}{}",
        one_line(&c.id),
        imp.as_str(),
        aut.as_str(),
        if passed { "PASS" } else { "FAIL" },
        if passed {
            String::new()
        } else {
            format!(" ({})", problems.join("; "))
        }
    )];
    for f in &check.findings {
        let mut l = format!(
            "  DIAG {} author={} import={} path={}",
            f.code.as_str(),
            f.severity(Profile::Author).as_str(),
            f.severity(Profile::Import).as_str(),
            f.path.as_deref().unwrap_or("-")
        );
        if let Some(field) = f.field() {
            l.push_str(&format!(" field={}", one_line(field)));
        }
        if let Some(line) = f.details.get("line") {
            l.push_str(&format!(" line={line}"));
        }
        if let Some(k) = f.details.get("limitKey").and_then(|v| v.as_str()) {
            l.push_str(&format!(" limitKey={k}"));
        }
        lines.push(l);
    }
    Ok(CaseResult {
        id: c.id.clone(),
        passed,
        negative,
        lines,
    })
}

/// Run all cases of a JSON cases document.
pub fn run_cases_json(text: &str) -> RunSummary {
    let doc: CasesDoc = match serde_json::from_str(text) {
        Ok(d) => d,
        Err(e) => {
            return RunSummary {
                results: Vec::new(),
                errors: vec![format!("cases document: {e}")],
            }
        }
    };
    let mut results = Vec::new();
    let mut errors = Vec::new();
    let mut ids = std::collections::BTreeSet::new();
    if doc.cases.is_empty() {
        errors.push("zero cases".to_string());
    }
    for c in &doc.cases {
        if !ids.insert(c.id.clone()) {
            errors.push(format!("duplicate case id {}", c.id));
        }
        match run_case(c) {
            Ok(r) => results.push(r),
            Err(e) => errors.push(e),
        }
    }
    RunSummary { results, errors }
}
