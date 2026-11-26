//! P02: name rules, container name, combined SKILL.md check. Case IDs refer to SPEC section 13.

use bgx_agent_skills_core::frontmatter::FieldValue;
use bgx_agent_skills_core::validation::{check_skill_md, container_name, validate_name, Container};
use bgx_agent_skills_core::{DiagnosticCode as D, FormatStatus, Profile};

fn name_codes(name: &str, dir: Option<&str>) -> Vec<D> {
    let (_, f) = validate_name(&FieldValue::Str(name.to_string()), dir, Some(2));
    f.iter().map(|x| x.code).collect()
}

fn status(name: &str, dir: Option<&str>, p: Profile) -> FormatStatus {
    let (_, f) = validate_name(&FieldValue::Str(name.to_string()), dir, Some(2));
    bgx_agent_skills_core::format_status(&f, p)
}

/// Target names of the 26 migrated local samples (inventory section 3, K00/K03).
const MIGRATED_SAMPLE_NAMES: [&str; 26] = [
    "chapters-only",
    "bible-roles-chapters",
    "staged-to-shots",
    "generation-policy-copy-official",
    "generation-policy-eye-garment",
    "generation-policy-one-slot-framing",
    "generation-policy-storyboard-style",
    "invalid-no-anchor",
    "character-expression-nine-grid",
    "character-costume-reference",
    "generation-policy-imax-65mm-generic",
    "generation-policy-imax-65mm-large-scene",
    "generation-policy-imax-65mm-portrait-closeup",
    "scenes-only-wuxia-budget",
    "shots-six-fixed-schema",
    "generation-policy-imax-65mm-authentic-photochemical",
    "character-multi-angle-reference",
    "storyboard-action-fight",
    "storyboard-micro-expression",
    "mixed-scene-bind-guide",
    "storyboard-performance",
    "optimize-storyboard-fight",
    "optimize-storyboard-micro-cu",
    "optimize-segment-continuity",
    "viral-song-mv",
    "short-video-creative",
];

#[test]
fn migrated_sample_names_are_unique_and_valid_in_author_profile() {
    let mut seen = std::collections::BTreeSet::new();
    for n in MIGRATED_SAMPLE_NAMES {
        assert!(seen.insert(n), "duplicate target name {n}");
        assert_eq!(name_codes(n, Some(n)), vec![], "{n}");
        assert_eq!(
            status(n, Some(n), Profile::Author),
            FormatStatus::Valid,
            "{n}"
        );
    }
}

#[test]
fn length_64_ok_65_warns_in_import_and_fails_in_author() {
    let n64 = "a".repeat(64);
    assert_eq!(name_codes(&n64, None), vec![]);
    let n65 = "a".repeat(65);
    assert_eq!(name_codes(&n65, None), vec![D::NameTooLong]);
    assert_eq!(
        status(&n65, None, Profile::Import),
        FormatStatus::ValidWithWarnings
    );
    assert_eq!(status(&n65, None, Profile::Author), FormatStatus::Invalid);
}

#[test]
fn w01_uppercase_and_invalid_characters_are_charset() {
    assert_eq!(name_codes("PDF-Processing", None), vec![D::NameCharset]);
    assert_eq!(name_codes("pdf_processing", None), vec![D::NameCharset]);
    assert_eq!(name_codes("pdf tools", None), vec![D::NameCharset]);
    assert_eq!(
        name_codes("local.chapters-only", None),
        vec![D::NameCharset],
        "no local. namespace"
    );
    assert_eq!(
        status("PDF", None, Profile::Import),
        FormatStatus::ValidWithWarnings
    );
    assert_eq!(status("PDF", None, Profile::Author), FormatStatus::Invalid);
}

#[test]
fn w03_hyphen_rules() {
    for n in ["-pdf", "pdf-", "pdf--tools"] {
        assert_eq!(name_codes(n, None), vec![D::NameHyphen], "{n}");
        assert_eq!(status(n, None, Profile::Author), FormatStatus::Invalid);
    }
    assert_eq!(name_codes("pdf-tools-2", None), vec![]);
}

#[test]
fn w04_directory_mismatch_and_nfkc_equality() {
    assert_eq!(
        name_codes("pdf-tools", Some("pdf-tool")),
        vec![D::NameDirMismatch]
    );
    assert_eq!(
        status("pdf-tools", Some("x"), Profile::Import),
        FormatStatus::ValidWithWarnings
    );
    assert_eq!(
        status("pdf-tools", Some("x"), Profile::Author),
        FormatStatus::Invalid
    );
    // Fullwidth digits in the directory name are NFKC-equal to ASCII digits.
    assert_eq!(name_codes("tool-2", Some("tool-\u{ff12}")), vec![]);
}

#[test]
fn p09_chinese_name_is_valid_with_portability_warning_in_both_profiles() {
    assert_eq!(
        name_codes("中文技能", Some("中文技能")),
        vec![D::NameNonAscii]
    );
    assert_eq!(
        status("中文技能", None, Profile::Import),
        FormatStatus::ValidWithWarnings
    );
    assert_eq!(
        status("中文技能", None, Profile::Author),
        FormatStatus::ValidWithWarnings
    );
    // Combining marks are not letters (skills-ref uses str.isalnum).
    // "e" + U+0301 composes under NFKC to a letter; "x" + U+0301 has no precomposed form.
    assert_eq!(name_codes("e\u{0301}x", None), vec![D::NameNonAscii]);
    assert_eq!(
        name_codes("x\u{0301}", None),
        vec![D::NameCharset, D::NameNonAscii]
    );
}

#[test]
fn n11_missing_blank_and_non_string_names() {
    let (n, f) = validate_name(&FieldValue::Absent, None, None);
    assert!(n.is_none());
    assert_eq!(f[0].code, D::NameMissing);
    assert_eq!(
        validate_name(&FieldValue::Null, None, Some(2)).1[0].code,
        D::NameMissing
    );
    assert_eq!(name_codes("   ", None), vec![D::NameMissing]);
    let (_, f) = validate_name(&FieldValue::NonString("int"), None, Some(2));
    assert_eq!(f[0].code, D::NameNotString);
    assert_eq!(f[0].details["yamlType"], "int");
    assert_eq!(f[0].field(), Some("name"));
    assert_eq!(f[0].details["line"], 2);
}

#[test]
fn n12_unsafe_names_are_errors_in_both_profiles() {
    for n in ["a/b", "a\\b", "a\u{0007}b", "..", ".", &"a".repeat(129)] {
        assert_eq!(name_codes(n, None), vec![D::NameUnsafe], "{n:?}");
        assert_eq!(status(n, None, Profile::Import), FormatStatus::Invalid);
    }
    // 128 chars is not unsafe, only too long.
    assert_eq!(name_codes(&"a".repeat(128), None), vec![D::NameTooLong]);
}

#[test]
fn container_name_precedence() {
    assert_eq!(
        container_name(&Container::Zip {
            file_name: "pdf.zip",
            wrapper: Some("pdf-tools")
        }),
        "pdf-tools"
    );
    assert_eq!(
        container_name(&Container::Zip {
            file_name: "pdf-tools.zip",
            wrapper: None
        }),
        "pdf-tools"
    );
    assert_eq!(
        container_name(&Container::Zip {
            file_name: "my.pack.zip",
            wrapper: None
        }),
        "my.pack"
    );
    assert_eq!(
        container_name(&Container::Directory {
            selected: "outer",
            wrapper: Some("inner")
        }),
        "inner"
    );
    assert_eq!(
        container_name(&Container::Directory {
            selected: "outer",
            wrapper: None
        }),
        "outer"
    );
    assert_eq!(
        container_name(&Container::SingleFile {
            parent_dir: "hello-skill"
        }),
        "hello-skill"
    );
}

#[test]
fn zip_wrapper_directory_is_compared_not_the_archive_name() {
    let md = b"---\nname: pdf-tools\ndescription: d\n---\nbody\n";
    let dir = container_name(&Container::Zip {
        file_name: "download (1).zip",
        wrapper: Some("pdf-tools"),
    });
    let c = check_skill_md(md, Some(&dir));
    assert!(c.findings.is_empty(), "{:?}", c.findings);
    let dir = container_name(&Container::Zip {
        file_name: "download (1).zip",
        wrapper: None,
    });
    let c = check_skill_md(md, Some(&dir));
    assert_eq!(
        c.findings.iter().map(|f| f.code).collect::<Vec<_>>(),
        vec![D::NameDirMismatch]
    );
}

#[test]
fn check_skill_md_combines_name_field_and_body_findings_with_locations() {
    let md = "---\ndescription: d\nname: Bad--Name\nversion: 1\n---\n\n".as_bytes();
    let c = check_skill_md(md, Some("bad-name"));
    let got: Vec<(D, Option<&str>)> = c.findings.iter().map(|f| (f.code, f.field())).collect();
    assert_eq!(
        got,
        vec![
            (D::NameCharset, Some("name")),
            (D::NameHyphen, Some("name")),
            (D::NameDirMismatch, Some("name")),
            (D::UnknownField, Some("version")),
            (D::BodyEmpty, None),
        ]
    );
    assert!(c
        .findings
        .iter()
        .all(|f| f.path.as_deref() == Some("SKILL.md")));
    assert_eq!(c.findings[0].details["line"], 3);
    assert_eq!(
        c.name.as_deref(),
        Some("Bad--Name"),
        "import keeps the lenient name"
    );
    assert_eq!(
        c.format_status(Profile::Import),
        FormatStatus::ValidWithWarnings
    );
    assert_eq!(c.format_status(Profile::Author), FormatStatus::Invalid);
    let j = c.diagnostics_json(Profile::Author);
    assert_eq!(j[0]["severity"], "error");
    assert_eq!(j[4]["severity"], "warning");
}

#[test]
fn parse_errors_stop_before_name_checks() {
    let c = check_skill_md(b"no frontmatter\n", Some("x"));
    assert_eq!(
        c.findings.iter().map(|f| f.code).collect::<Vec<_>>(),
        vec![D::FrontmatterMissing]
    );
    assert!(c.name.is_none());
}
