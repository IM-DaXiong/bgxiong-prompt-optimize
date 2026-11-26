//! P04: packageDigest v1. Golden vectors are read from the K02 contract (single source).

use bgx_agent_skills_core::digest::{is_package_digest, package_digest, package_digest_of_entries};
use bgx_agent_skills_core::resources::{build_manifest, PackageFile};

const CONTRACT: &str = include_str!("../../../../packages/app-contracts/v1/agent-skills.json");
const SKILL: &str = "---\nname: hello-skill\ndescription: Say hello in the user's language. Use when the user greets you.\n---\n\n# Hello\n\nReply with a short greeting.\n";

#[test]
fn reproduces_contract_golden_vectors() {
    let c: serde_json::Value = serde_json::from_str(CONTRACT).unwrap();
    let vectors = c["packageDigest"]["goldenVectors"].as_array().unwrap();
    assert!(vectors.len() >= 2);
    for v in vectors {
        let files: Vec<(String, String)> = v["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| {
                (
                    f["path"].as_str().unwrap().to_string(),
                    f["utf8"].as_str().unwrap().to_string(),
                )
            })
            .collect();
        let got = package_digest(files.iter().map(|(p, b)| (p.as_str(), b.as_bytes())));
        assert_eq!(got, v["digest"].as_str().unwrap());
        assert!(is_package_digest(&got));
    }
}

#[test]
fn enumeration_order_does_not_matter_and_manifest_path_agrees() {
    let a: [(&str, &[u8]); 3] = [
        ("SKILL.md", SKILL.as_bytes()),
        ("references/tone.md", b"Keep it warm and brief.\n"),
        ("assets/x.png", &[0x89, b'P', b'N', b'G']),
    ];
    let mut b = a;
    b.reverse();
    assert_eq!(package_digest(a), package_digest(b));
    let files: Vec<PackageFile> = b
        .iter()
        .map(|(p, x)| PackageFile { path: p, bytes: x })
        .collect();
    let (m, findings) = build_manifest(&files);
    assert!(findings.is_empty());
    assert_eq!(package_digest_of_entries(&m.entries), package_digest(a));
}

#[test]
fn changing_any_attachment_changes_the_digest() {
    let base = package_digest([
        ("SKILL.md", SKILL.as_bytes()),
        ("assets/a.bin", &[1u8, 2, 3][..]),
    ]);
    let changed = package_digest([
        ("SKILL.md", SKILL.as_bytes()),
        ("assets/a.bin", &[1u8, 2, 4][..]),
    ]);
    assert_ne!(base, changed);
    let renamed = package_digest([
        ("SKILL.md", SKILL.as_bytes()),
        ("assets/b.bin", &[1u8, 2, 3][..]),
    ]);
    assert_ne!(base, renamed, "paths are part of the digest");
    let added = package_digest([
        ("SKILL.md", SKILL.as_bytes()),
        ("assets/a.bin", &[1u8, 2, 3][..]),
        ("empty.txt", &[][..]),
    ]);
    assert_ne!(base, added, "an empty file still counts");
}

#[test]
fn crlf_bytes_differ_from_lf_bytes_because_text_is_never_normalized() {
    let crlf = SKILL.replace('\n', "\r\n");
    assert_ne!(
        package_digest([("SKILL.md", SKILL.as_bytes())]),
        package_digest([("SKILL.md", crlf.as_bytes())])
    );
}

#[test]
fn same_package_imported_twice_has_same_digest() {
    let first = package_digest([
        ("SKILL.md", SKILL.as_bytes()),
        ("references/tone.md", &b"Keep it warm and brief.\n"[..]),
    ]);
    let again = package_digest([
        ("references/tone.md", &b"Keep it warm and brief.\n"[..]),
        ("SKILL.md", SKILL.as_bytes()),
    ]);
    assert_eq!(first, again);
}

#[test]
fn sort_is_by_utf8_bytes_not_locale_or_case() {
    // "B" (0x42) < "a" (0x61) < "中" (0xE4..) by bytes.
    let x = package_digest([
        ("a.md", &b"1"[..]),
        ("B.md", &b"2"[..]),
        ("中.md", &b"3"[..]),
        ("SKILL.md", &b"s"[..]),
    ]);
    let y = package_digest([
        ("中.md", &b"3"[..]),
        ("SKILL.md", &b"s"[..]),
        ("B.md", &b"2"[..]),
        ("a.md", &b"1"[..]),
    ]);
    assert_eq!(x, y);
}

#[test]
fn digest_format_check() {
    assert!(!is_package_digest("sha256:ABC"));
    assert!(!is_package_digest(&format!("sha256:{}", "A".repeat(64))));
    assert!(!is_package_digest(&"0".repeat(64)));
    assert!(is_package_digest(&format!("sha256:{}", "0".repeat(64))));
}
