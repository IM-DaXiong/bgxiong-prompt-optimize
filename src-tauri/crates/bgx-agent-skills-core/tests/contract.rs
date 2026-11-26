//! P00: generated contract constants match `packages/app-contracts/v1/agent-skills.json`.

use bgx_agent_skills_core::*;

const CONTRACT: &str = include_str!("../../../../packages/app-contracts/v1/agent-skills.json");

fn contract() -> serde_json::Value {
    serde_json::from_str(CONTRACT).expect("agent-skills.json parses")
}

#[test]
fn every_contract_diagnostic_is_generated_with_phase_and_severities() {
    let c = contract();
    let diags = c["diagnostics"].as_array().unwrap();
    assert_eq!(diags.len(), DiagnosticCode::ALL.len());
    for d in diags {
        let code = d["code"].as_str().unwrap();
        let g = DiagnosticCode::parse(code).unwrap_or_else(|| panic!("{code} not generated"));
        assert_eq!(g.as_str(), code);
        assert_eq!(g.phase().as_str(), d["phase"].as_str().unwrap());
        assert_eq!(
            g.severity(Profile::Import).as_str(),
            d["severity"]["import"].as_str().unwrap()
        );
        assert_eq!(
            g.severity(Profile::Author).as_str(),
            d["severity"]["author"].as_str().unwrap()
        );
        assert_eq!(g.meaning(), d["meaning"].as_str().unwrap());
    }
}

#[test]
fn every_budget_is_generated() {
    let c = contract();
    let b = c["budgets"].as_object().unwrap();
    let pairs: &[(&str, u64)] = &[
        (budgets::MAX_FILE_COUNT_KEY, budgets::MAX_FILE_COUNT),
        (
            budgets::MAX_DIRECTORY_DEPTH_KEY,
            budgets::MAX_DIRECTORY_DEPTH,
        ),
        (
            budgets::MAX_RELATIVE_PATH_BYTES_KEY,
            budgets::MAX_RELATIVE_PATH_BYTES,
        ),
        (
            budgets::MAX_PATH_SEGMENT_BYTES_KEY,
            budgets::MAX_PATH_SEGMENT_BYTES,
        ),
        (
            budgets::MAX_SINGLE_FILE_BYTES_KEY,
            budgets::MAX_SINGLE_FILE_BYTES,
        ),
        (
            budgets::MAX_TOTAL_UNCOMPRESSED_BYTES_KEY,
            budgets::MAX_TOTAL_UNCOMPRESSED_BYTES,
        ),
        (budgets::MAX_SKILL_MD_BYTES_KEY, budgets::MAX_SKILL_MD_BYTES),
        (
            budgets::MAX_FRONTMATTER_BYTES_KEY,
            budgets::MAX_FRONTMATTER_BYTES,
        ),
        (
            budgets::NAME_HARD_MAX_CHARS_KEY,
            budgets::NAME_HARD_MAX_CHARS,
        ),
        (
            budgets::NAME_SPEC_MAX_CHARS_KEY,
            budgets::NAME_SPEC_MAX_CHARS,
        ),
        (
            budgets::DESCRIPTION_SPEC_MAX_CHARS_KEY,
            budgets::DESCRIPTION_SPEC_MAX_CHARS,
        ),
        (
            budgets::COMPATIBILITY_SPEC_MAX_CHARS_KEY,
            budgets::COMPATIBILITY_SPEC_MAX_CHARS,
        ),
        (
            budgets::BODY_ADVISORY_MAX_LINES_KEY,
            budgets::BODY_ADVISORY_MAX_LINES,
        ),
    ];
    for (k, v) in pairs {
        assert_eq!(b[*k]["value"].as_u64(), Some(*v), "budget {k}");
    }
}

#[test]
fn status_sets_and_digest_constants_match() {
    let c = contract();
    let sets = &c["statusSets"];
    for s in Source::ALL {
        assert!(sets["source"].get(s.as_str()).is_some());
    }
    assert_eq!(sets["source"].as_object().unwrap().len(), Source::ALL.len());
    assert_eq!(
        sets["formatStatus"].as_object().unwrap().len(),
        FormatStatus::ALL.len()
    );
    assert_eq!(
        sets["severity"].as_object().unwrap().len(),
        Severity::ALL.len()
    );
    assert_eq!(sets["phase"].as_object().unwrap().len(), Phase::ALL.len());
    assert_eq!(
        sets["resourceKind"].as_object().unwrap().len(),
        ResourceKind::ALL.len()
    );
    assert!(
        Source::parse("project").is_none(),
        "project scope is not supported (K01)"
    );
    assert_eq!(c["packageDigest"]["prefix"].as_str(), Some(DIGEST_PREFIX));
    assert_eq!(
        c["packageDigest"]["domainSeparator"].as_str(),
        Some(DIGEST_DOMAIN_SEPARATOR)
    );
    assert_eq!(
        c["packageDigest"]["algorithm"].as_str(),
        Some(DIGEST_ALGORITHM)
    );
    for (k, v) in c["limitationDiagnostic"].as_object().unwrap() {
        let kind = LimitationKind::parse(k).unwrap();
        assert_eq!(kind.diagnostic_code().as_str(), v.as_str().unwrap());
    }
}

#[test]
fn format_status_follows_profile_severity() {
    let w = Finding::new(DiagnosticCode::NameCharset);
    assert_eq!(
        format_status(std::slice::from_ref(&w), Profile::Import),
        FormatStatus::ValidWithWarnings
    );
    assert_eq!(format_status(&[w], Profile::Author), FormatStatus::Invalid);
    let i = Finding::new(DiagnosticCode::MacosxIgnored);
    assert_eq!(format_status(&[i], Profile::Author), FormatStatus::Valid);
    assert_eq!(format_status(&[], Profile::Author), FormatStatus::Valid);
}

#[test]
fn diagnostic_json_has_contract_shape() {
    let f = Finding::new(DiagnosticCode::UnknownField)
        .with_path(SKILL_MD)
        .with_field("version");
    let j = f.to_diagnostic_json(Profile::Import);
    assert_eq!(j["code"], "AGENT_SKILL_UNKNOWN_FIELD");
    assert_eq!(j["severity"], "warning");
    assert_eq!(j["phase"], "validate");
    assert_eq!(j["path"], "SKILL.md");
    assert_eq!(j["details"]["field"], "version");
    assert!(!j["message"].as_str().unwrap().is_empty());
    let keys: Vec<&String> = j.as_object().unwrap().keys().collect();
    for k in keys {
        assert!(["code", "severity", "phase", "message", "path", "details"].contains(&k.as_str()));
    }
}

#[test]
fn sha256_hex_is_lowercase_64() {
    let h = sha256_hex(b"");
    assert_eq!(
        h,
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
}
