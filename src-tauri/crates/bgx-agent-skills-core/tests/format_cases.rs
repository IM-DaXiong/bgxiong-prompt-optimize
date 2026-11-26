//! P06: the frontmatter cases fixture passes through the same core verifier the CLI uses.

use bgx_agent_skills_core::verify::run_cases_json;

const CASES: &str = include_str!("../../../../tests/fixtures/agent-skills/frontmatter-cases.json");

#[test]
fn all_fixture_cases_pass_and_cover_spec_matrix() {
    let s = run_cases_json(CASES);
    assert!(s.ok(), "{}", s.render());
    assert!(s.negative() > 0 && s.results.len() > s.negative());
    let doc: serde_json::Value = serde_json::from_str(CASES).unwrap();
    let specs: std::collections::BTreeSet<String> = doc["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["spec"].as_str().map(str::to_string))
        .collect();
    let mut want: Vec<String> = ["P01", "P02", "P06", "P07", "P08", "P09", "P10"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    want.extend((1..=10).map(|i| format!("W{i:02}")));
    want.extend((4..=13).map(|i| format!("N{i:02}")));
    for w in want {
        assert!(specs.contains(&w), "SPEC case {w} has no fixture case");
    }
}

#[test]
fn zero_cases_fail() {
    let s = run_cases_json(r#"{"cases": []}"#);
    assert!(!s.ok());
    assert!(s.summary_line().contains("cases=0"));
}

#[test]
fn wrong_expectation_fails_and_output_has_no_file_content() {
    let doc = r#"{"cases": [{"id": "x", "containerName": "x", "skillMd": "---\nname: x\ndescription: SECRET-DESCRIPTION-TEXT\nfoo: 1\n---\nbody\n", "expect": {"import": "valid"}}]}"#;
    let s = run_cases_json(doc);
    assert!(!s.ok());
    let out = s.render();
    assert!(
        out.contains("CASE x import=valid_with_warnings author=invalid result=FAIL"),
        "{out}"
    );
    assert!(out.contains("DIAG AGENT_SKILL_UNKNOWN_FIELD author=error import=warning path=SKILL.md field=foo line=4"), "{out}");
    assert!(!out.contains("SECRET-DESCRIPTION-TEXT"));
}

#[test]
fn malformed_documents_and_cases_are_errors() {
    assert!(!run_cases_json("not json").ok());
    assert!(!run_cases_json(r#"{"cases": [{"id": "a", "skillMd": "x", "expect": {}}]}"#).ok());
    assert!(!run_cases_json(r#"{"cases": [{"id": "a", "expect": {"import": "valid"}}]}"#).ok());
    assert!(!run_cases_json(
        r#"{"cases": [{"id": "a", "skillMd": "x", "expect": {"import": "ok"}}]}"#
    )
    .ok());
    assert!(!run_cases_json(
        r#"{"cases": [{"id": "a", "skillMd": "x", "extra": 1, "expect": {"import": "invalid"}}]}"#
    )
    .ok());
    let dup = r#"{"cases": [{"id": "a", "skillMd": "x", "expect": {"import": "invalid"}}, {"id": "a", "skillMd": "x", "expect": {"import": "invalid"}}]}"#;
    assert!(!run_cases_json(dup).ok());
}

#[test]
fn user_facing_packages_must_pass_author_profile() {
    let good = r#"{"cases": [{"id": "fixtures/agent-skills/x/SKILL.md", "containerName": "x", "skillMd": "---\nname: x\ndescription: d\n---\nbody\n", "expect": {"authorUsable": true}}]}"#;
    assert!(run_cases_json(good).ok());
    let bad = r#"{"cases": [{"id": "fixtures/agent-skills/x/SKILL.md", "containerName": "x", "skillMd": "---\nname: X\ndescription: d\n---\nbody\n", "expect": {"authorUsable": true}}]}"#;
    assert!(!run_cases_json(bad).ok());
}
