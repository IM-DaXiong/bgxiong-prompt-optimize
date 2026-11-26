//! T00: package-level conformance. One fixture (`tests/fixtures/agent-skills/conformance.json`)
//! drives the core manifest/digest/verifier here, the `bgx-agent-skills-verify` CLI below, and
//! the host import/export tests (`agent_skills::tests::conformance`). Expected digests in the
//! fixture were computed from the SPEC 8.2 formula by a separate script, and this test
//! recomputes them again, so a digest change cannot slip through by regenerating one side.

use std::collections::BTreeSet;

use bgx_agent_skills_core::digest::{package_digest, package_digest_of_entries};
use bgx_agent_skills_core::resources::{build_manifest, PackageFile};
use bgx_agent_skills_core::validation::check_skill_md;
use bgx_agent_skills_core::verify::run_cases_json;
use bgx_agent_skills_core::{
    format_status, sha256_hex, Profile, DIGEST_DOMAIN_SEPARATOR, DIGEST_PREFIX, SKILL_MD,
};
use serde_json::Value;

const FIXTURE: &str = include_str!("../../../../tests/fixtures/agent-skills/conformance.json");

struct Pkg {
    id: String,
    container: String,
    files: Vec<(String, Vec<u8>)>,
    expect: Value,
}

fn hex_bytes(h: &str) -> Vec<u8> {
    assert!(h.len() % 2 == 0, "odd hex length");
    (0..h.len() / 2)
        .map(|i| u8::from_str_radix(&h[2 * i..2 * i + 2], 16).expect("hex"))
        .collect()
}

fn load() -> Vec<Pkg> {
    let doc: Value = serde_json::from_str(FIXTURE).expect("conformance.json is JSON");
    assert_eq!(doc["schema"], "bgx-agent-skills-conformance/v1");
    doc["packages"]
        .as_array()
        .expect("packages")
        .iter()
        .map(|p| Pkg {
            id: p["id"].as_str().expect("id").to_string(),
            container: p["container"].as_str().expect("container").to_string(),
            files: p["files"]
                .as_array()
                .expect("files")
                .iter()
                .map(|f| {
                    let path = f["path"].as_str().expect("path").to_string();
                    let bytes = match (f.get("text"), f.get("hex")) {
                        (Some(t), None) => t.as_str().expect("text").as_bytes().to_vec(),
                        (None, Some(h)) => hex_bytes(h.as_str().expect("hex")),
                        _ => panic!("file {path} needs exactly one of text/hex"),
                    };
                    (path, bytes)
                })
                .collect(),
            expect: p["expect"].clone(),
        })
        .collect()
}

fn strs(v: &Value) -> BTreeSet<String> {
    v.as_array()
        .expect("array")
        .iter()
        .map(|x| x.as_str().expect("string").to_string())
        .collect()
}

/// SPEC 8.2 written out again from scratch (no core digest code).
fn digest_by_formula(files: &[(String, Vec<u8>)]) -> String {
    use sha2::{Digest, Sha256};
    let mut lines: Vec<(&[u8], String)> = files
        .iter()
        .map(|(p, b)| (p.as_bytes(), sha256_hex(b)))
        .collect();
    lines.sort();
    let mut h = Sha256::new();
    h.update(DIGEST_DOMAIN_SEPARATOR.as_bytes());
    for (p, hex) in lines {
        h.update(hex.as_bytes());
        h.update(b"  ");
        h.update(p);
        h.update(b"\n");
    }
    let mut out = String::from(DIGEST_PREFIX);
    for b in h.finalize().iter() {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

#[test]
fn fixture_covers_the_required_package_kinds() {
    let pkgs = load();
    let ids: BTreeSet<&str> = pkgs.iter().map(|p| p.id.as_str()).collect();
    for want in [
        "plain-body",
        "optional-frontmatter",
        "custom-dirs",
        "image-asset",
        "json-resource",
        "script-resources",
        "case-collision",
        "duplicate-entry",
        "no-skill-md",
        "unsafe-path",
        "name-directory-mismatch",
    ] {
        assert!(ids.contains(want), "conformance package {want} is missing");
    }
    assert_eq!(ids.len(), pkgs.len(), "package ids are unique");
    let negative = pkgs
        .iter()
        .filter(|p| p.expect.get("digest").is_none())
        .count();
    assert!(
        negative >= 5 && pkgs.len() - negative >= 7,
        "positive={} negative={}",
        pkgs.len() - negative,
        negative
    );
}

#[test]
fn core_manifest_digest_and_verifier_match_every_package() {
    let mut checked = 0;
    for p in load() {
        let files: Vec<PackageFile<'_>> = p
            .files
            .iter()
            .map(|(path, bytes)| PackageFile { path, bytes })
            .collect();
        let (manifest, findings) = build_manifest(&files);
        let got: BTreeSet<String> = findings
            .iter()
            .map(|f| f.code.as_str().to_string())
            .collect();
        assert_eq!(
            got,
            strs(&p.expect["manifestCodes"]),
            "{}: manifest codes",
            p.id
        );

        if let Some((_, skill)) = p.files.iter().find(|(path, _)| path == SKILL_MD) {
            let check = check_skill_md(skill, Some(&p.container));
            let codes: BTreeSet<String> = check
                .findings
                .iter()
                .map(|f| f.code.as_str().to_string())
                .collect();
            assert_eq!(
                codes,
                strs(&p.expect["skillCodes"]),
                "{}: SKILL.md codes",
                p.id
            );
            assert_eq!(
                check.format_status(Profile::Import).as_str(),
                p.expect["import"].as_str().unwrap(),
                "{}: import status",
                p.id
            );
            assert_eq!(
                check.format_status(Profile::Author).as_str(),
                p.expect["author"].as_str().unwrap(),
                "{}: author status",
                p.id
            );
        }

        let skill_findings = p
            .files
            .iter()
            .find(|(path, _)| path == SKILL_MD)
            .map(|(_, skill)| check_skill_md(skill, Some(&p.container)).findings)
            .unwrap_or_default();
        let all: Vec<_> = skill_findings
            .iter()
            .chain(findings.iter())
            .cloned()
            .collect();
        assert_eq!(
            format_status(&all, Profile::Import).as_str(),
            p.expect["packageImport"].as_str().unwrap(),
            "{}: whole-package import status",
            p.id
        );
        if let Some(want_digest) = p.expect.get("digest") {
            let want_digest = want_digest.as_str().unwrap();
            assert!(
                findings.is_empty(),
                "{}: positive package has findings",
                p.id
            );
            let resources = p.expect["resources"].as_array().unwrap();
            assert_eq!(
                manifest.entries.len(),
                resources.len(),
                "{}: resource count",
                p.id
            );
            for (e, w) in manifest.entries.iter().zip(resources) {
                assert_eq!(
                    e.resource_ref,
                    w["path"].as_str().unwrap(),
                    "{}: order",
                    p.id
                );
                assert_eq!(
                    e.kind.as_str(),
                    w["kind"].as_str().unwrap(),
                    "{}: {}",
                    p.id,
                    e.resource_ref
                );
                assert_eq!(
                    e.bytes,
                    w["bytes"].as_u64().unwrap(),
                    "{}: {}",
                    p.id,
                    e.resource_ref
                );
            }
            let by_core = package_digest(p.files.iter().map(|(a, b)| (a.as_str(), b.as_slice())));
            assert_eq!(by_core, want_digest, "{}: core digest", p.id);
            assert_eq!(
                digest_by_formula(&p.files),
                want_digest,
                "{}: formula digest",
                p.id
            );
            assert_eq!(
                package_digest_of_entries(&manifest.entries),
                want_digest,
                "{}: entries digest",
                p.id
            );
            let reversed = p
                .files
                .iter()
                .rev()
                .map(|(a, b)| (a.as_str(), b.as_slice()));
            assert_eq!(
                package_digest(reversed),
                want_digest,
                "{}: order independent",
                p.id
            );
        }
        checked += 1;
    }
    assert!(checked >= 12, "only {checked} packages were checked");
}

/// Packages with a `SKILL.md`, as the cases document the CLI reads.
fn cases_document(pkgs: &[Pkg]) -> (String, usize) {
    let mut cases = Vec::new();
    for p in pkgs {
        if let Some((_, skill)) = p.files.iter().find(|(path, _)| path == SKILL_MD) {
            cases.push(serde_json::json!({
                "id": format!("conformance/{}", p.id),
                "containerName": p.container,
                "skillMd": String::from_utf8(skill.clone()).expect("fixture SKILL.md is UTF-8"),
                "expect": {
                    "import": p.expect["import"],
                    "author": p.expect["author"],
                    "codes": p.expect["skillCodes"],
                },
            }));
        }
    }
    let n = cases.len();
    (serde_json::json!({ "cases": cases }).to_string(), n)
}

/// The CLI (`bgx-agent-skills-verify`) is a thin stdin wrapper over `run_cases_json`; the
/// process-level run over this same fixture is `scripts/check-agent-skills-format.ps1` (this
/// pure-core crate may not spawn processes, see `check-workspace-crates.ps1`).
#[test]
fn verify_runner_accepts_the_same_packages_and_rejects_a_wrong_expectation() {
    let mut pkgs = load();
    let (doc, n) = cases_document(&pkgs);
    assert!(n >= 12, "only {n} SKILL.md cases");
    let lib = run_cases_json(&doc);
    assert!(lib.ok(), "{}", lib.render());
    assert!(
        lib.summary_line()
            .contains(&format!("cases={n} passed={n} failed=0")),
        "{}",
        lib.summary_line()
    );

    let p = pkgs
        .iter_mut()
        .find(|p| p.id == "name-directory-mismatch")
        .unwrap();
    p.expect["import"] = Value::String("valid".into());
    let (wrong, _) = cases_document(&pkgs);
    let bad = run_cases_json(&wrong);
    assert!(!bad.ok());
    assert!(
        bad.summary_line().contains("failed=1"),
        "{}",
        bad.summary_line()
    );
}
