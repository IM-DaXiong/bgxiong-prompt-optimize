//! P03: resource manifest. Case IDs refer to SPEC section 13.

use bgx_agent_skills_core::resources::{
    build_manifest, check_package_path, classify, collision_key, media_type_for, PackageFile,
};
use bgx_agent_skills_core::{budgets, sha256_hex, DiagnosticCode as D, ResourceKind};

const SKILL: &[u8] = b"---\nname: a\ndescription: b\n---\nbody\n";
const PNG: &[u8] = &[
    0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0x0d,
];
const TTF: &[u8] = &[0x00, 0x01, 0x00, 0x00, 0x00, 0x0c, 0x00, 0x80];

fn f<'a>(path: &'a str, bytes: &'a [u8]) -> PackageFile<'a> {
    PackageFile { path, bytes }
}

fn codes(findings: &[bgx_agent_skills_core::Finding]) -> Vec<D> {
    findings.iter().map(|x| x.code).collect()
}

#[test]
fn p03_custom_dirs_binary_assets_and_legacy_files_are_all_kept() {
    let files = [
        f("SKILL.md", SKILL),
        f("assets/fonts/NotoSansSC.ttf", TTF),
        f("assets/logo.png", PNG),
        f("my-stuff/notes 中文 (1).md", "笔记".as_bytes()),
        f("scripts/run.py", b"print('hi')\n"),
        f("helper.wasm", b"\0asm\x01\0\0\0"),
        f("pipeline.json", b"{}"),
        f("references/empty.txt", b""),
    ];
    let (m, findings) = build_manifest(&files);
    assert!(findings.is_empty(), "{:?}", codes(&findings));
    assert_eq!(m.entries.len(), files.len(), "no whitelist drops a file");
    let refs: Vec<&str> = m.entries.iter().map(|e| e.resource_ref.as_str()).collect();
    let mut sorted = refs.clone();
    sorted.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    assert_eq!(refs, sorted, "sorted by UTF-8 bytes");
    let font = m.get("assets/fonts/NotoSansSC.ttf").unwrap();
    assert_eq!(font.kind, ResourceKind::Binary);
    assert_eq!(font.media_type, Some("font/ttf"));
    assert_eq!(m.get("assets/logo.png").unwrap().kind, ResourceKind::Binary);
    assert_eq!(m.get("helper.wasm").unwrap().kind, ResourceKind::Binary);
    let empty = m.get("references/empty.txt").unwrap();
    assert_eq!((empty.kind, empty.bytes), (ResourceKind::Text, 0));
    assert_eq!(empty.sha256, sha256_hex(b""));
    let zh = m.get("my-stuff/notes 中文 (1).md").unwrap();
    assert_eq!(zh.kind, ResourceKind::Text);
    assert_eq!(zh.bytes, "笔记".len() as u64);
    assert_eq!(
        m.total_bytes,
        files.iter().map(|x| x.bytes.len() as u64).sum::<u64>()
    );
}

#[test]
fn bad_utf8_and_nul_bytes_are_binary_never_text() {
    assert_eq!(classify(&[0xff, 0xfe, b'a']), ResourceKind::Binary);
    assert_eq!(
        classify(&[0xd6, 0xd0]),
        ResourceKind::Binary,
        "GBK bytes are not decoded"
    );
    assert_eq!(classify(b"a\0b"), ResourceKind::Binary);
    assert_eq!(classify("文本".as_bytes()), ResourceKind::Text);
    assert_eq!(classify(b""), ResourceKind::Text);
}

#[test]
fn entry_json_has_exact_contract_shape() {
    let (m, _) = build_manifest(&[f("SKILL.md", SKILL)]);
    let j = m.entries[0].to_json();
    let mut keys: Vec<&str> = j.as_object().unwrap().keys().map(|k| k.as_str()).collect();
    keys.sort_unstable();
    assert_eq!(keys, vec!["bytes", "kind", "resourceRef", "sha256"]);
    assert_eq!(j["kind"], "text");
    assert_eq!(j["sha256"].as_str().unwrap().len(), 64);
}

#[test]
fn n16_case_collision_after_nfc_and_lowercase() {
    let (_, findings) = build_manifest(&[f("SKILL.md", SKILL), f("A.md", b"1"), f("a.md", b"2")]);
    assert_eq!(codes(&findings), vec![D::PathCaseCollision]);
    assert_eq!(
        findings[0].details["paths"],
        serde_json::json!(["A.md", "a.md"])
    );
    // NFD vs NFC forms of "é" collide too.
    let nfc = "caf\u{e9}.md";
    let nfd = "cafe\u{301}.md";
    assert_eq!(collision_key(nfc), collision_key(nfd));
    let (m, findings) = build_manifest(&[f("SKILL.md", SKILL), f(nfc, b"1"), f(nfd, b"2")]);
    assert_eq!(codes(&findings), vec![D::PathCaseCollision]);
    assert!(
        m.get(nfd).is_some(),
        "paths are stored as given, never renamed"
    );
}

#[test]
fn n02_only_lowercase_skill_md_is_missing() {
    let (_, findings) = build_manifest(&[f("skill.md", SKILL)]);
    assert_eq!(codes(&findings), vec![D::SkillMdMissing]);
}

#[test]
fn n14_unsafe_paths_are_rejected_and_excluded() {
    for (p, reason) in [
        ("../x", "dotSegment"),
        ("/etc/x", "absolute"),
        ("C:/x", "driveLetter"),
        ("\\\\server\\share", "absolute"),
        ("a\\b", "backslash"),
        ("a//b", "emptySegment"),
        ("a/", "emptySegment"),
        ("./a", "dotSegment"),
        ("a\u{1}b", "controlCharacter"),
        ("", "empty"),
    ] {
        let found = check_package_path(p);
        assert_eq!(codes(&found), vec![D::PathUnsafe], "{p:?}");
        assert_eq!(found[0].details["reason"], reason, "{p:?}");
        assert!(
            found[0].path.is_none(),
            "unsafe raw path never goes into `path`"
        );
    }
    let (m, findings) = build_manifest(&[f("SKILL.md", SKILL), f("../evil", b"x")]);
    assert_eq!(codes(&findings), vec![D::PathUnsafe]);
    assert!(m.get("../evil").is_none());
}

#[test]
fn n15_windows_reserved_names_and_trailing_dot_or_space() {
    for p in [
        "CON.txt",
        "assets/nul",
        "lpt9.md",
        "com1",
        "a/AUX.tar.gz",
        "dir./x",
        "x ",
    ] {
        assert_eq!(
            codes(&check_package_path(p)),
            vec![D::PathReservedName],
            "{p:?}"
        );
    }
    for p in ["CONSOLE.md", "com10.md", "lpt0", "auxiliary/x.md"] {
        assert!(check_package_path(p).is_empty(), "{p:?}");
    }
}

#[test]
fn n18_path_and_package_budgets() {
    let deep16 = format!("{}x.md", "d/".repeat(15));
    assert!(check_package_path(&deep16).is_empty());
    let deep17 = format!("{}x.md", "d/".repeat(16));
    assert_eq!(
        check_package_path(&deep17)[0].details["limitKey"],
        "maxDirectoryDepth"
    );
    let seg = "s".repeat(budgets::MAX_PATH_SEGMENT_BYTES as usize + 1);
    assert_eq!(
        check_package_path(&seg)[0].details["limitKey"],
        "maxPathSegmentBytes"
    );
    let long = format!("{}/{}", "a".repeat(120), "b".repeat(120));
    assert_eq!(long.len(), 241);
    assert_eq!(
        check_package_path(&long)[0].details["limitKey"],
        "maxRelativePathBytes"
    );

    let names: Vec<String> = (0..budgets::MAX_FILE_COUNT)
        .map(|i| format!("r/{i}.txt"))
        .collect();
    let mut files: Vec<PackageFile> = names.iter().map(|n| f(n, b"")).collect();
    files.push(f("SKILL.md", SKILL));
    let (_, findings) = build_manifest(&files);
    assert_eq!(codes(&findings), vec![D::LimitExceeded]);
    assert_eq!(findings[0].details["limitKey"], "maxFileCount");

    let big = vec![b'x'; budgets::MAX_SINGLE_FILE_BYTES as usize + 1];
    let (_, findings) = build_manifest(&[f("SKILL.md", SKILL), f("big.bin", &big)]);
    assert_eq!(findings[0].details["limitKey"], "maxSingleFileBytes");
    assert_eq!(findings[0].path.as_deref(), Some("big.bin"));

    let half = vec![b'y'; budgets::MAX_SINGLE_FILE_BYTES as usize];
    let (_, findings) =
        build_manifest(&[f("SKILL.md", SKILL), f("a.bin", &half), f("b.bin", &half)]);
    assert_eq!(codes(&findings), vec![D::LimitExceeded]);
    assert_eq!(findings[0].details["limitKey"], "maxTotalUncompressedBytes");
}

#[test]
fn duplicate_input_path_is_reported() {
    let (_, findings) = build_manifest(&[f("SKILL.md", SKILL), f("x.md", b"1"), f("x.md", b"2")]);
    assert_eq!(codes(&findings), vec![D::DuplicateEntry]);
}

#[test]
fn media_type_is_advisory_and_unknown_extensions_are_fine() {
    assert_eq!(media_type_for("a/B.PNG"), Some("image/png"));
    assert_eq!(media_type_for("data.custom"), None);
    assert_eq!(media_type_for("Makefile"), None);
    let (m, findings) = build_manifest(&[f("SKILL.md", SKILL), f("data.custom", b"\x01\x02")]);
    assert!(findings.is_empty());
    assert_eq!(m.get("data.custom").unwrap().media_type, None);
}
