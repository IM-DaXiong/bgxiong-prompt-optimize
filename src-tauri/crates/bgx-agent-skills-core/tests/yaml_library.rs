//! P00: pinned YAML library behaviour (SPEC section 4.4). yaml-rust2 =0.13.0 is used ONLY through
//! its pull parser (`Parser::next_token`). These cases fail loudly if an upgrade changes any
//! behaviour the frontmatter parser relies on.

use yaml_rust2::parser::{Event, Parser};
use yaml_rust2::scanner::TScalarStyle;

fn events(src: &str) -> Result<Vec<Event>, String> {
    let mut p = Parser::new_from_str(src);
    let mut out = Vec::new();
    loop {
        match p.next_token() {
            Ok((Event::StreamEnd, _)) => return Ok(out),
            Ok((ev, _)) => out.push(ev),
            Err(e) => return Err(e.to_string()),
        }
    }
}

#[test]
fn duplicate_keys_are_reported_as_separate_events_not_merged() {
    let ev = events("name: a\nname: b\n").unwrap();
    let keys: Vec<_> = ev
        .iter()
        .filter_map(|e| match e {
            Event::Scalar(v, _, _, _) if v == "name" => Some(v.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        keys.len(),
        2,
        "library must not deduplicate keys; the core detects duplicates"
    );
}

#[test]
fn anchors_and_aliases_are_visible_as_events() {
    let ev = events("a: &x 1\nb: *x\n").unwrap();
    assert!(ev
        .iter()
        .any(|e| matches!(e, Event::Scalar(_, _, id, _) if *id > 0)));
    assert!(ev.iter().any(|e| matches!(e, Event::Alias(_))));
}

#[test]
fn tags_are_resolved_to_handle_and_suffix() {
    let ev = events("a: !!str 1\nb: !foo x\n").unwrap();
    let tags: Vec<(String, String)> = ev
        .iter()
        .filter_map(|e| match e {
            Event::Scalar(_, _, _, Some(t)) => Some((t.handle.clone(), t.suffix.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(
        tags[0],
        ("tag:yaml.org,2002:".to_string(), "str".to_string())
    );
    assert_eq!(tags[1], ("!".to_string(), "foo".to_string()));
}

#[test]
fn unquoted_colon_value_is_a_parse_error() {
    let err = events("name: a\ndescription: Use when: x\n").unwrap_err();
    assert!(err.contains("mapping values are not allowed"), "{err}");
}

#[test]
fn scalar_styles_distinguish_plain_from_quoted_and_block() {
    let ev = events("a: 1\nb: \"1\"\nc: >\n  x\n  y\n").unwrap();
    let styles: Vec<TScalarStyle> = ev
        .iter()
        .filter_map(|e| match e {
            Event::Scalar(_, s, _, _) => Some(*s),
            _ => None,
        })
        .collect();
    assert_eq!(styles[1], TScalarStyle::Plain);
    assert_eq!(styles[3], TScalarStyle::DoubleQuoted);
    assert_eq!(styles[5], TScalarStyle::Folded);
    assert!(ev
        .iter()
        .any(|e| matches!(e, Event::Scalar(v, _, _, _) if v == "x y\n")));
}

#[test]
fn document_end_marker_starts_a_second_document() {
    let ev = events("a: 1\n...\nb: 2\n").unwrap();
    let docs = ev
        .iter()
        .filter(|e| matches!(e, Event::DocumentStart))
        .count();
    assert_eq!(docs, 2, "core must reject multi-document frontmatter");
}

#[test]
fn deep_block_nesting_does_not_overflow_with_pull_parser() {
    // `Parser::load` recurses per nesting level and overflows the stack around 5000 levels
    // (verified on box, 8 MiB stack); the pull API is iterative.
    let src = format!("{}x\n", "- ".repeat(30_000));
    let ev = events(&src).unwrap();
    assert!(ev.len() > 30_000);
}

#[test]
fn deep_flow_nesting_is_a_parse_error() {
    let err = events(&"[".repeat(1000)).unwrap_err();
    assert!(err.contains("recursion limit exceeded"), "{err}");
}

#[test]
fn empty_or_comment_only_stream_has_no_document() {
    assert!(events("")
        .unwrap()
        .iter()
        .all(|e| matches!(e, Event::StreamStart)));
    assert!(events("# c\n")
        .unwrap()
        .iter()
        .all(|e| matches!(e, Event::StreamStart)));
}
