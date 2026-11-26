//! P07: resourceRef parsing (K02 fixture parity) and body reference resolution.

use bgx_agent_skills_core::frontmatter::parse_skill_md;
use bgx_agent_skills_core::resource_path::{
    check_body_references, classify_destination, parse_resource_ref, scan_body_links, LinkTarget,
    LocalRef,
};
use bgx_agent_skills_core::DiagnosticCode as D;

macro_rules! fx {
    ($kind:literal, $name:literal) => {
        (
            $name,
            include_str!(concat!(
                "../../../../tests/fixtures/agent-skills/app-contract/",
                $kind,
                "/",
                $name,
                ".json"
            )),
        )
    };
}

fn resource_ref_of(fixture: &str) -> String {
    let v: serde_json::Value = serde_json::from_str(fixture).unwrap();
    v["instance"]["resourceRef"].as_str().unwrap().to_string()
}

#[test]
fn resource_ref_parity_with_k02_fixtures() {
    let valid = [
        fx!("valid", "read-resource-input-min"),
        fx!("valid", "read-resource-input-paged"),
    ];
    for (name, f) in valid {
        let r = resource_ref_of(f);
        assert!(parse_resource_ref(&r).is_ok(), "{name}: {r}");
    }
    let invalid = [
        fx!("invalid", "read-input-absolute"),
        fx!("invalid", "read-input-backslash"),
        fx!("invalid", "read-input-control-char"),
        fx!("invalid", "read-input-dot"),
        fx!("invalid", "read-input-dotdot"),
        fx!("invalid", "read-input-drive"),
        fx!("invalid", "read-input-empty-segment"),
        fx!("invalid", "read-input-too-deep"),
        fx!("invalid", "read-input-trailing-slash"),
    ];
    for (name, f) in invalid {
        let r = resource_ref_of(f);
        let e = parse_resource_ref(&r).expect_err(name);
        assert_eq!(e.code, D::ResourceRefInvalid, "{name}");
    }
    // Chinese, spaces and parentheses are fine in a resourceRef.
    assert!(parse_resource_ref("references/中文 指南 (v2).md").is_ok());
    assert!(parse_resource_ref("SKILL.md").is_ok());
    assert!(parse_resource_ref(&"a".repeat(241)).is_err());
    assert!(
        parse_resource_ref(&"中".repeat(81)).is_err(),
        "243 UTF-8 bytes > maxRelativePathBytes"
    );
}

const FILES: [&str; 6] = [
    "SKILL.md",
    "references/中文 指南 (v2).md",
    "references/a.md",
    "scripts/run.py",
    "assets/logo.png",
    "references/deep/b.md",
];

fn body_findings(body: &str) -> Vec<(u64, String, String)> {
    check_body_references(body, 5, FILES)
        .into_iter()
        .map(|f| {
            assert_eq!(f.code, D::ReferenceMissing);
            assert_eq!(f.path.as_deref(), Some("SKILL.md"));
            (
                f.details["line"].as_u64().unwrap(),
                f.details["target"].as_str().unwrap().to_string(),
                f.details["reason"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

#[test]
fn chinese_space_and_parenthesis_paths_resolve() {
    let body = "\
[angle](<references/中文 指南 (v2).md>)
[encoded](references/%E4%B8%AD%E6%96%87%20%E6%8C%87%E5%8D%97%20(v2).md)
[raw spaces](references/中文 指南 (v2).md)
[titled](references/a.md \"Title\")
![logo](assets/logo.png)
[script](scripts/run.py) and [dot](./references/a.md) and [dir](references/)
[up-and-back](references/deep/../a.md#section)
";
    assert_eq!(body_findings(body), vec![]);
    let links = scan_body_links(body, 5);
    assert_eq!(links.len(), 9);
    assert_eq!(
        links[0].target,
        LinkTarget::Local(LocalRef::Resolved("references/中文 指南 (v2).md".into()))
    );
    assert_eq!(links[2].target, links[0].target);
}

#[test]
fn remote_urls_are_not_local_files_and_anchors_are_skipped() {
    let body = "[site](https://example.com/references/a.md)\n[mail](mailto:a@b.c)\n[cdn](//cdn.example.com/x.js)\n[top](#usage)\n";
    assert_eq!(body_findings(body), vec![]);
    let links = scan_body_links(body, 1);
    assert!(matches!(&links[0].target, LinkTarget::Remote { scheme } if scheme == "https"));
    assert!(matches!(&links[2].target, LinkTarget::Remote { .. }));
    assert_eq!(links[3].target, LinkTarget::Anchor);
}

#[test]
fn bad_references_are_located_not_silently_ignored() {
    let body = "\
Intro
[missing](references/missing.md)
[drive](C:/Users/me/x.md) [drive2](C:\\x.md)
[up](../other-skill/SKILL.md)
[abs](/etc/passwd)
[back](references\\a.md)
[file](file:///etc/passwd)
[pct](references/%ZZ.md)

[guide]: references/nope.md
";
    let got = body_findings(body);
    let want: Vec<(u64, &str, &str)> = vec![
        (6, "references/missing.md", "notFound"),
        (7, "C:/Users/me/x.md", "driveLetter"),
        (7, "C:\\x.md", "driveLetter"),
        (8, "../other-skill/SKILL.md", "escapesPackage"),
        (9, "/etc/passwd", "absolute"),
        (10, "references\\a.md", "backslash"),
        (11, "file:///etc/passwd", "fileUrl"),
        (12, "references/%ZZ.md", "badPercentEncoding"),
        (14, "references/nope.md", "notFound"),
    ];
    let want: Vec<(u64, String, String)> = want
        .into_iter()
        .map(|(l, t, r)| (l, t.to_string(), r.to_string()))
        .collect();
    assert_eq!(got, want);
}

#[test]
fn code_blocks_and_inline_code_are_not_links() {
    let body = "\
```markdown
[x](references/missing-in-fence.md)
```
~~~
[y](missing2.md)
~~~
Use `[z](missing3.md)` literally. Real: [ok](references/a.md)
";
    assert_eq!(body_findings(body), vec![]);
    assert_eq!(scan_body_links(body, 1).len(), 1);
}

#[test]
fn line_numbers_come_from_the_real_skill_md() {
    let md =
        "---\nname: refs\ndescription: d\n---\n\n# Title\n\nSee [guide](references/missing.md).\n";
    let p = parse_skill_md(md.as_bytes()).parsed.unwrap();
    let f = check_body_references(p.body_raw, p.body_start_line, ["SKILL.md"]);
    assert_eq!(f.len(), 1);
    assert_eq!(f[0].details["line"], 8);
    assert_eq!(f[0].details["resolved"], "references/missing.md");
}

#[test]
fn destinations_are_classified_from_the_package_root() {
    assert_eq!(
        classify_destination("a/b/../c.md"),
        LinkTarget::Local(LocalRef::Resolved("a/c.md".into()))
    );
    assert_eq!(
        classify_destination("../x"),
        LinkTarget::Local(LocalRef::Unresolvable {
            reason: "escapesPackage"
        })
    );
    assert_eq!(
        classify_destination("D:x"),
        LinkTarget::Local(LocalRef::Unresolvable {
            reason: "driveLetter"
        })
    );
    assert!(
        matches!(classify_destination("HTTPS://x"), LinkTarget::Remote { scheme } if scheme == "https")
    );
}
