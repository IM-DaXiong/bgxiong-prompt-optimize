//! P01: frontmatter parsing. Case IDs refer to SPEC section 13.

use bgx_agent_skills_core::frontmatter::{parse_skill_md, FieldValue, FrontmatterOutcome};
use bgx_agent_skills_core::{budgets, format_status, DiagnosticCode as D, FormatStatus, Profile};

fn md(fm: &str, body: &str) -> String {
    format!("---\n{fm}---\n{body}")
}

const OK_BODY: &str = "\n# Title\n\nDo the thing.\n";

fn codes(o: &FrontmatterOutcome) -> Vec<D> {
    o.findings.iter().map(|f| f.code).collect()
}

fn only(o: &FrontmatterOutcome, code: D) -> serde_json::Value {
    let hits: Vec<_> = o.findings.iter().filter(|f| f.code == code).collect();
    assert_eq!(
        hits.len(),
        1,
        "expected exactly one {code:?}, got {:?}",
        codes(o)
    );
    serde_json::Value::Object(hits[0].details.clone().into_iter().collect())
}

#[test]
fn p01_minimal_package_is_clean() {
    let s = md(
        "name: hello-skill\ndescription: Say hello. Use when greeted.\n",
        OK_BODY,
    );
    let o = parse_skill_md(s.as_bytes());
    assert!(o.findings.is_empty(), "{:?}", codes(&o));
    let p = o.parsed.unwrap();
    assert_eq!(p.frontmatter.name, FieldValue::Str("hello-skill".into()));
    assert_eq!(p.body_raw, OK_BODY);
    assert_eq!(p.body_for_model(), "# Title\n\nDo the thing.");
    assert_eq!(p.body_start_line, 5);
    assert!(!p.bom);
}

#[test]
fn p02_all_optional_fields_are_accepted() {
    let fm = "name: pdf-processing\ndescription: Extract PDF text.\nlicense: Apache-2.0\ncompatibility: Requires python3 and network access\nmetadata:\n  author: example-org\n  version: \"1.0\"\nallowed-tools: Bash(git:*) Read\n";
    let s = md(fm, OK_BODY);
    let o = parse_skill_md(s.as_bytes());
    assert!(o.findings.is_empty(), "{:?}", codes(&o));
    let f = o.parsed.unwrap().frontmatter;
    assert_eq!(f.license.as_str(), Some("Apache-2.0"));
    assert_eq!(f.allowed_tools.as_str(), Some("Bash(git:*) Read"));
    assert_eq!(
        f.metadata_pairs,
        vec![
            ("author".into(), "example-org".into()),
            ("version".into(), "1.0".into())
        ]
    );
    assert!(f.unknown_fields.is_empty());
}

#[test]
fn p06_bom_and_crlf_are_accepted_and_raw_text_is_kept() {
    let s = "\u{feff}---\r\nname: hello-skill\r\ndescription: Say hello.\r\n---\r\n\r\n# Hi\r\n";
    let o = parse_skill_md(s.as_bytes());
    assert!(o.findings.is_empty(), "{:?}", codes(&o));
    let p = o.parsed.unwrap();
    assert!(p.bom);
    assert_eq!(p.frontmatter.description.as_str(), Some("Say hello."));
    assert_eq!(
        p.frontmatter_raw,
        "name: hello-skill\r\ndescription: Say hello.\r\n"
    );
    assert_eq!(p.body_raw, "\r\n# Hi\r\n");
    assert_eq!(p.body_for_model(), "# Hi");
}

#[test]
fn p07_folded_multiline_chinese_description_is_readable() {
    let fm = "name: storyboard-helper\ndescription: >\n  把剧本拆成分镜。\n  Use when the user asks for shots.\n";
    let s = md(fm, OK_BODY);
    let o = parse_skill_md(s.as_bytes());
    assert!(o.findings.is_empty(), "{:?}", codes(&o));
    let d = o.parsed.unwrap().frontmatter.description;
    assert_eq!(
        d.as_str(),
        Some("把剧本拆成分镜。 Use when the user asks for shots.\n")
    );
}

#[test]
fn delimiter_rules_trailing_whitespace_ok_dots_not_a_closer() {
    let o = parse_skill_md(b"--- \t\nname: a\ndescription: b\n---  \nbody\n");
    assert!(o.findings.is_empty(), "{:?}", codes(&o));
    let o = parse_skill_md(b"---\nname: a\ndescription: b\n...\nbody\n");
    assert_eq!(codes(&o), vec![D::FrontmatterUnclosed]);
}

#[test]
fn n04_missing_opening_delimiter() {
    for s in [
        "name: a\n---\n",
        "\n---\nname: a\n---\n",
        " ---\nname: a\n---\n",
        "",
    ] {
        let o = parse_skill_md(s.as_bytes());
        assert_eq!(codes(&o), vec![D::FrontmatterMissing], "{s:?}");
        assert!(o.parsed.is_none());
    }
}

#[test]
fn n05_unclosed_frontmatter() {
    let o = parse_skill_md(b"---\nname: a\ndescription: b\n");
    assert_eq!(codes(&o), vec![D::FrontmatterUnclosed]);
}

#[test]
fn n06_unquoted_colon_is_yaml_invalid_not_repaired() {
    let o = parse_skill_md(b"---\nname: a\ndescription: Use when: x\n---\nbody\n");
    let d = only(&o, D::FrontmatterYamlInvalid);
    assert_eq!(d["line"], 3, "line is the SKILL.md line");
    assert!(o.parsed.is_none());
}

#[test]
fn n07_frontmatter_must_be_a_mapping() {
    let o = parse_skill_md(b"---\n- a\n- b\n---\nbody\n");
    assert_eq!(only(&o, D::FrontmatterNotMapping)["yamlType"], "sequence");
    let o = parse_skill_md(b"---\njust text\n---\nbody\n");
    assert_eq!(only(&o, D::FrontmatterNotMapping)["yamlType"], "string");
}

#[test]
fn n08_duplicate_keys_fail_at_any_level() {
    let o = parse_skill_md(b"---\nname: a\ndescription: b\nname: c\n---\nbody\n");
    let d = only(&o, D::FrontmatterDuplicateKey);
    assert_eq!(d["field"], "name");
    assert_eq!(d["line"], 4);
    let o = parse_skill_md(b"---\nname: a\n\"name\": c\ndescription: b\n---\nbody\n");
    only(&o, D::FrontmatterDuplicateKey);
    let o = parse_skill_md(b"---\nname: a\ndescription: b\nmetadata:\n  k: 1\n  k: 2\n---\nbody\n");
    assert_eq!(only(&o, D::FrontmatterDuplicateKey)["key"], "k");
}

#[test]
fn n09_anchor_alias_merge_and_alias_bomb_are_rejected_without_expansion() {
    let o = parse_skill_md(b"---\nname: &n a\ndescription: *n\n---\nbody\n");
    assert_eq!(
        only(&o, D::FrontmatterAliasForbidden)["construct"],
        "anchor"
    );
    let o = parse_skill_md(b"---\nname: a\ndescription: b\nmetadata:\n  <<: {x: y}\n---\nbody\n");
    assert_eq!(
        only(&o, D::FrontmatterAliasForbidden)["construct"],
        "mergeKey"
    );
    // "billion laughs": nine levels of 9 aliases each; rejected at the first anchor.
    let mut bomb = String::from("---\nname: a\ndescription: b\nl0: &l0 [lol]\n");
    for i in 1..10 {
        let refs = vec![format!("*l{}", i - 1); 9].join(", ");
        bomb.push_str(&format!("l{i}: &l{i} [{refs}]\n"));
    }
    bomb.push_str("---\nbody\n");
    let t = std::time::Instant::now();
    let o = parse_skill_md(bomb.as_bytes());
    assert_eq!(codes(&o), vec![D::FrontmatterAliasForbidden]);
    assert!(t.elapsed().as_secs() < 2);
    let o = parse_skill_md(b"---\nname: a\ndescription: b\nx: *undefined\n---\nbody\n");
    assert!(matches!(
        codes(&o)[..],
        [D::FrontmatterAliasForbidden] | [D::FrontmatterYamlInvalid]
    ));
}

#[test]
fn n10_non_utf8_is_rejected_with_offset() {
    let mut b = b"---\nname: a\ndescription: ".to_vec();
    b.extend_from_slice(&[0xd6, 0xd0, 0xce, 0xc4]); // GBK bytes
    b.extend_from_slice(b"\n---\nbody\n");
    let o = parse_skill_md(&b);
    assert_eq!(only(&o, D::SkillMdNotUtf8)["byteOffset"], 25);
}

#[test]
fn n13_description_missing_blank_null_or_not_string() {
    for (fm, code) in [
        ("name: a\n", D::DescriptionMissing),
        ("name: a\ndescription:\n", D::DescriptionMissing),
        ("name: a\ndescription: \"   \"\n", D::DescriptionMissing),
        ("name: a\ndescription: [x]\n", D::DescriptionNotString),
        ("name: a\ndescription: true\n", D::DescriptionNotString),
        ("name: a\ndescription: 1.5\n", D::DescriptionNotString),
    ] {
        let s = md(fm, OK_BODY);
        let o = parse_skill_md(s.as_bytes());
        assert_eq!(codes(&o), vec![code], "{fm:?}");
        let f = &o.findings[0];
        assert_eq!(f.field(), Some("description"));
        assert_eq!(
            format_status(&o.findings, Profile::Import),
            FormatStatus::Invalid
        );
    }
}

#[test]
fn yaml_1_2_core_schema_yes_is_a_string() {
    let o = parse_skill_md(b"---\nname: a\ndescription: yes\n---\nbody\n");
    assert!(o.findings.is_empty(), "{:?}", codes(&o));
    let o = parse_skill_md(b"---\nname: a\ndescription: !!str 123\n---\nbody\n");
    assert!(o.findings.is_empty(), "{:?}", codes(&o));
    assert_eq!(
        o.parsed.unwrap().frontmatter.description.as_str(),
        Some("123")
    );
}

#[test]
fn non_core_or_mismatched_tags_are_yaml_invalid() {
    let o = parse_skill_md(b"---\nname: a\ndescription: !custom b\n---\nbody\n");
    assert_eq!(only(&o, D::FrontmatterYamlInvalid)["reason"], "nonCoreTag");
    let o = parse_skill_md(b"---\nname: a\ndescription: b\nx: !!int abc\n---\nbody\n");
    assert_eq!(only(&o, D::FrontmatterYamlInvalid)["reason"], "tagMismatch");
}

#[test]
fn multi_document_and_collection_keys_are_yaml_invalid() {
    let o = parse_skill_md(b"---\nname: a\n...\ndescription: b\n---\nbody\n");
    assert_eq!(
        only(&o, D::FrontmatterYamlInvalid)["reason"],
        "multipleDocuments"
    );
    let o = parse_skill_md(b"---\nname: a\ndescription: b\n? [x]\n: y\n---\nbody\n");
    assert_eq!(
        only(&o, D::FrontmatterYamlInvalid)["reason"],
        "collectionKey"
    );
}

#[test]
fn empty_frontmatter_is_an_empty_mapping() {
    let o = parse_skill_md(b"---\n---\nbody\n");
    assert_eq!(codes(&o), vec![D::DescriptionMissing]);
    assert_eq!(o.parsed.unwrap().frontmatter.name, FieldValue::Absent);
    let o = parse_skill_md(b"---\n# only a comment\n---\nbody\n");
    assert_eq!(codes(&o), vec![D::DescriptionMissing]);
}

#[test]
fn w05_description_length_counts_unicode_scalars() {
    let ok = format!("name: a\ndescription: \"{}\"\n", "中".repeat(1024));
    assert!(parse_skill_md(md(&ok, OK_BODY).as_bytes())
        .findings
        .is_empty());
    let long = format!("name: a\ndescription: {}\n", "x".repeat(1025));
    let s = md(&long, OK_BODY);
    let o = parse_skill_md(s.as_bytes());
    let d = only(&o, D::DescriptionTooLong);
    assert_eq!(d["chars"], 1025);
    assert_eq!(
        format_status(&o.findings, Profile::Import),
        FormatStatus::ValidWithWarnings
    );
    assert_eq!(
        format_status(&o.findings, Profile::Author),
        FormatStatus::Invalid
    );
}

#[test]
fn w06_compatibility_empty_or_over_500_or_not_string() {
    let base = "name: a\ndescription: b\n";
    let ok = format!("{base}compatibility: {}\n", "c".repeat(500));
    assert!(parse_skill_md(md(&ok, OK_BODY).as_bytes())
        .findings
        .is_empty());
    for (extra, reason) in [
        (format!("compatibility: {}\n", "c".repeat(501)), "tooLong"),
        ("compatibility: \"\"\n".to_string(), "empty"),
        ("compatibility: [a]\n".to_string(), "notString"),
    ] {
        let s = md(&format!("{base}{extra}"), OK_BODY);
        let o = parse_skill_md(s.as_bytes());
        assert_eq!(only(&o, D::CompatibilityInvalid)["reason"], reason);
    }
}

#[test]
fn w07_metadata_must_map_strings_to_strings_and_view_exposes_strings_only() {
    let s = md(
        "name: a\ndescription: b\nmetadata:\n  author: me\n  rev: 3\n  tags: [x]\n",
        OK_BODY,
    );
    let o = parse_skill_md(s.as_bytes());
    assert_eq!(only(&o, D::MetadataInvalid)["nonStringEntries"], 2);
    assert_eq!(
        o.parsed.unwrap().frontmatter.metadata_pairs,
        vec![("author".into(), "me".into())]
    );
    let s = md("name: a\ndescription: b\nmetadata: plain\n", OK_BODY);
    assert_eq!(
        only(&parse_skill_md(s.as_bytes()), D::MetadataInvalid)["reason"],
        "notMapping"
    );
}

#[test]
fn w08_allowed_tools_list_and_w10_license_number() {
    let s = md(
        "name: a\ndescription: b\nallowed-tools: [Bash, Read]\nlicense: 2\n",
        OK_BODY,
    );
    let o = parse_skill_md(s.as_bytes());
    let mut c = codes(&o);
    c.sort();
    let mut want = vec![D::AllowedToolsInvalid, D::LicenseNotString];
    want.sort();
    assert_eq!(c, want);
}

#[test]
fn w09_unknown_fields_are_warned_and_kept_in_raw_text() {
    let s = md(
        "name: a\ndescription: b\nversion: 1.2.0\ndisable-model-invocation: true\n",
        OK_BODY,
    );
    let o = parse_skill_md(s.as_bytes());
    let fields: Vec<_> = o
        .findings
        .iter()
        .map(|f| {
            (
                f.code,
                f.field().unwrap().to_string(),
                f.details["line"].clone(),
            )
        })
        .collect();
    assert_eq!(
        fields,
        vec![
            (D::UnknownField, "version".to_string(), serde_json::json!(4)),
            (
                D::UnknownField,
                "disable-model-invocation".to_string(),
                serde_json::json!(5)
            ),
        ]
    );
    let p = o.parsed.unwrap();
    assert!(p.frontmatter_raw.contains("disable-model-invocation: true"));
    assert_eq!(
        p.frontmatter.unknown_fields,
        vec!["version", "disable-model-invocation"]
    );
    assert_eq!(
        format_status(&o.findings, Profile::Import),
        FormatStatus::ValidWithWarnings
    );
}

#[test]
fn p10_body_large_and_empty_body_are_warnings() {
    let body: String = (0..501).map(|i| format!("line {i}\n")).collect();
    let s = md("name: a\ndescription: b\n", &body);
    let o = parse_skill_md(s.as_bytes());
    assert_eq!(only(&o, D::BodyLarge)["lines"], 501);
    let body: String = (0..500).map(|i| format!("line {i}\n")).collect();
    assert!(
        parse_skill_md(md("name: a\ndescription: b\n", &body).as_bytes())
            .findings
            .is_empty()
    );
    let o = parse_skill_md(b"---\nname: a\ndescription: b\n---\n\n  \n");
    assert_eq!(codes(&o), vec![D::BodyEmpty]);
    assert_eq!(
        format_status(&o.findings, Profile::Author),
        FormatStatus::ValidWithWarnings
    );
}

#[test]
fn n18_size_budgets_skill_md_and_frontmatter() {
    let big = vec![b'a'; budgets::MAX_SKILL_MD_BYTES as usize + 1];
    let o = parse_skill_md(&big);
    assert_eq!(only(&o, D::LimitExceeded)["limitKey"], "maxSkillMdBytes");
    let fm = format!(
        "name: a\ndescription: b\nx: \"{}\"\n",
        "y".repeat(budgets::MAX_FRONTMATTER_BYTES as usize)
    );
    let s = md(&fm, OK_BODY);
    let o = parse_skill_md(s.as_bytes());
    assert_eq!(
        only(&o, D::LimitExceeded)["limitKey"],
        "maxFrontmatterBytes"
    );
}

#[test]
fn deeply_nested_frontmatter_does_not_crash() {
    let fm = format!("name: a\ndescription: b\nx: {}z\n", "- ".repeat(20_000));
    let s = md(&fm, OK_BODY);
    let o = parse_skill_md(s.as_bytes());
    // Either the library rejects it or it parses as an unknown field; never a stack overflow.
    assert!(
        matches!(
            codes(&o)[..],
            [D::UnknownField] | [D::FrontmatterYamlInvalid]
        ),
        "{:?}",
        codes(&o)
    );
    let fm = format!("name: a\ndescription: b\nx: {}\n", "[".repeat(300));
    let s = md(&fm, OK_BODY);
    let o = parse_skill_md(s.as_bytes());
    assert_eq!(codes(&o), vec![D::FrontmatterYamlInvalid]);
}
