//! P01: standard `SKILL.md` frontmatter parsing (SPEC sections 4-6).
//!
//! The YAML text between the two `---` delimiter lines is parsed with the pinned YAML library's
//! pull parser (`yaml-rust2`, SPEC section 4.4) into an array arena. No regex is used as a YAML
//! parser, nothing is auto-repaired, and the raw bytes are never rewritten: the parse result is
//! only an index into the original text.

use std::collections::{BTreeMap, HashSet};

use yaml_rust2::parser::{Event, Parser, Tag};
use yaml_rust2::scanner::{Marker, TScalarStyle};

use crate::{budgets, DiagnosticCode, Finding, SKILL_MD};

/// Index of a node in [`YamlDoc::nodes`].
pub type NodeId = usize;

/// A YAML 1.2 core-schema scalar.
#[derive(Clone, Debug, PartialEq)]
pub enum Scalar {
    Null,
    Bool(bool),
    Int(String),
    Float(String),
    Str(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum NodeKind {
    Scalar(Scalar),
    Seq(Vec<NodeId>),
    Map(Vec<(NodeId, NodeId)>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub kind: NodeKind,
    /// 1-based line in `SKILL.md` where the node starts.
    pub line: usize,
    /// Scalar source value (after YAML unescaping); `None` for collections.
    pub text: Option<String>,
}

/// Parsed frontmatter document. Built and dropped without recursion.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct YamlDoc {
    pub nodes: Vec<Node>,
    pub root: Option<NodeId>,
}

impl YamlDoc {
    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id]
    }

    pub fn as_str(&self, id: NodeId) -> Option<&str> {
        match &self.nodes[id].kind {
            NodeKind::Scalar(Scalar::Str(s)) => Some(s),
            _ => None,
        }
    }

    /// YAML type name used in diagnostics details.
    pub fn type_name(&self, id: NodeId) -> &'static str {
        match &self.nodes[id].kind {
            NodeKind::Scalar(Scalar::Null) => "null",
            NodeKind::Scalar(Scalar::Bool(_)) => "bool",
            NodeKind::Scalar(Scalar::Int(_)) => "int",
            NodeKind::Scalar(Scalar::Float(_)) => "float",
            NodeKind::Scalar(Scalar::Str(_)) => "string",
            NodeKind::Seq(_) => "sequence",
            NodeKind::Map(_) => "mapping",
        }
    }
}

/// Value of a known frontmatter field.
#[derive(Clone, Debug, PartialEq)]
pub enum FieldValue {
    Absent,
    /// Key present with a YAML null value (e.g. `license:`).
    Null,
    Str(String),
    /// Present with another YAML type (`int`, `bool`, `float`, `sequence`, `mapping`).
    NonString(&'static str),
}

impl FieldValue {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            FieldValue::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn is_present(&self) -> bool {
        !matches!(self, FieldValue::Absent)
    }
}

/// Frontmatter field names defined by the open specification.
pub const KNOWN_FIELDS: [&str; 6] = [
    "name",
    "description",
    "license",
    "compatibility",
    "metadata",
    "allowed-tools",
];

/// Typed view of the frontmatter mapping. Unknown fields are listed, never dropped from the raw text.
#[derive(Clone, Debug, PartialEq)]
pub struct Frontmatter {
    pub doc: YamlDoc,
    /// Top-level keys in source order (as text).
    pub keys: Vec<String>,
    /// 1-based `SKILL.md` line of each top-level key.
    pub key_lines: BTreeMap<String, usize>,
    pub name: FieldValue,
    pub description: FieldValue,
    pub license: FieldValue,
    pub compatibility: FieldValue,
    pub allowed_tools: FieldValue,
    /// `metadata` presence and type.
    pub metadata: FieldValue,
    /// String-to-string pairs of `metadata` (non-string entries are not exposed).
    pub metadata_pairs: Vec<(String, String)>,
    pub unknown_fields: Vec<String>,
}

impl Frontmatter {
    pub fn line_of(&self, field: &str) -> Option<usize> {
        self.key_lines.get(field).copied()
    }
}

/// Result of a successful structural parse of `SKILL.md`.
#[derive(Clone, Debug, PartialEq)]
pub struct ParsedSkillMd<'a> {
    /// The file started with a UTF-8 BOM (skipped for parsing only).
    pub bom: bool,
    /// YAML text between the delimiter lines (original bytes, original line endings).
    pub frontmatter_raw: &'a str,
    /// Body: everything after the closing delimiter line, unchanged.
    pub body_raw: &'a str,
    /// 1-based `SKILL.md` line where the body starts.
    pub body_start_line: usize,
    pub frontmatter: Frontmatter,
}

impl ParsedSkillMd<'_> {
    /// Body as given to the model: leading blank lines and trailing whitespace removed (SPEC 4.6).
    pub fn body_for_model(&self) -> &str {
        body_for_model(self.body_raw)
    }
}

/// Outcome of [`parse_skill_md`]: `parsed` is `None` when a parse-phase error stopped parsing.
#[derive(Clone, Debug, PartialEq)]
pub struct FrontmatterOutcome<'a> {
    pub parsed: Option<ParsedSkillMd<'a>>,
    pub findings: Vec<Finding>,
}

/// Strip leading blank lines and trailing whitespace.
pub fn body_for_model(body: &str) -> &str {
    let mut rest = body;
    while let Some(p) = rest.find('\n') {
        if !rest[..p].trim().is_empty() {
            break;
        }
        rest = &rest[p + 1..];
    }
    rest.trim_end()
}

struct Line<'a> {
    /// Byte offset of the line start in the text.
    start: usize,
    /// Byte offset just past the line terminator (or text end).
    next: usize,
    /// Line content without `\n` / `\r\n`.
    content: &'a str,
}

fn split_lines(text: &str) -> Vec<Line<'_>> {
    let mut out = Vec::new();
    let mut start = 0usize;
    while start < text.len() {
        let (end, next) = match text[start..].find('\n') {
            Some(p) => (start + p, start + p + 1),
            None => (text.len(), text.len()),
        };
        let content = text[start..end]
            .strip_suffix('\r')
            .unwrap_or(&text[start..end]);
        out.push(Line {
            start,
            next,
            content,
        });
        start = next;
    }
    out
}

fn is_delimiter(line: &str) -> bool {
    line.trim_end_matches([' ', '\t']) == "---"
}

fn limit_finding(key: &str, limit: u64, actual: usize) -> Finding {
    Finding::new(DiagnosticCode::LimitExceeded)
        .with_path(SKILL_MD)
        .with_detail("limitKey", key)
        .with_detail("limit", limit)
        .with_detail("actual", actual as u64)
}

/// Parse `SKILL.md` bytes: encoding, delimiters, YAML, typed fields and field/body rules
/// (all SPEC section 5 rules except `name`, which [`crate::validation`] checks).
pub fn parse_skill_md(bytes: &[u8]) -> FrontmatterOutcome<'_> {
    let mut findings = Vec::new();
    if bytes.len() as u64 > budgets::MAX_SKILL_MD_BYTES {
        findings.push(limit_finding(
            budgets::MAX_SKILL_MD_BYTES_KEY,
            budgets::MAX_SKILL_MD_BYTES,
            bytes.len(),
        ));
        return FrontmatterOutcome {
            parsed: None,
            findings,
        };
    }
    let text = match std::str::from_utf8(bytes) {
        Ok(t) => t,
        Err(e) => {
            findings.push(
                Finding::new(DiagnosticCode::SkillMdNotUtf8)
                    .with_path(SKILL_MD)
                    .with_detail("byteOffset", e.valid_up_to() as u64),
            );
            return FrontmatterOutcome {
                parsed: None,
                findings,
            };
        }
    };
    let (bom, text) = match text.strip_prefix('\u{feff}') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let lines = split_lines(text);
    if lines
        .first()
        .map(|l| !is_delimiter(l.content))
        .unwrap_or(true)
    {
        findings.push(
            Finding::new(DiagnosticCode::FrontmatterMissing)
                .with_path(SKILL_MD)
                .with_detail("line", 1u64),
        );
        return FrontmatterOutcome {
            parsed: None,
            findings,
        };
    }
    let close = lines
        .iter()
        .enumerate()
        .skip(1)
        .find(|(_, l)| is_delimiter(l.content));
    let Some((close_idx, close_line)) = close else {
        findings.push(
            Finding::new(DiagnosticCode::FrontmatterUnclosed)
                .with_path(SKILL_MD)
                .with_detail("line", 1u64),
        );
        return FrontmatterOutcome {
            parsed: None,
            findings,
        };
    };
    let fm_raw = &text[lines[0].next..close_line.start];
    if fm_raw.len() as u64 > budgets::MAX_FRONTMATTER_BYTES {
        findings.push(limit_finding(
            budgets::MAX_FRONTMATTER_BYTES_KEY,
            budgets::MAX_FRONTMATTER_BYTES,
            fm_raw.len(),
        ));
        return FrontmatterOutcome {
            parsed: None,
            findings,
        };
    }
    let body_raw = &text[close_line.next..];
    let body_start_line = close_idx + 2;

    // YAML lines start at SKILL.md line 2 (line 1 is the opening delimiter).
    let doc = match build_doc(fm_raw, 1) {
        Ok(d) => d,
        Err(f) => {
            findings.push(f);
            return FrontmatterOutcome {
                parsed: None,
                findings,
            };
        }
    };
    let root_map: Vec<(NodeId, NodeId)> = match doc.root {
        None => Vec::new(),
        Some(r) => match &doc.node(r).kind {
            NodeKind::Map(pairs) => pairs.clone(),
            _ => {
                findings.push(
                    Finding::new(DiagnosticCode::FrontmatterNotMapping)
                        .with_path(SKILL_MD)
                        .with_detail("line", doc.node(r).line as u64)
                        .with_detail("yamlType", doc.type_name(r)),
                );
                return FrontmatterOutcome {
                    parsed: None,
                    findings,
                };
            }
        },
    };

    let fm = extract_fields(doc, &root_map);
    field_findings(&fm, &mut findings);
    body_findings(body_raw, body_start_line, &mut findings);
    FrontmatterOutcome {
        parsed: Some(ParsedSkillMd {
            bom,
            frontmatter_raw: fm_raw,
            body_raw,
            body_start_line,
            frontmatter: fm,
        }),
        findings,
    }
}

fn field_value(doc: &YamlDoc, id: NodeId) -> FieldValue {
    match &doc.node(id).kind {
        NodeKind::Scalar(Scalar::Str(s)) => FieldValue::Str(s.clone()),
        NodeKind::Scalar(Scalar::Null) => FieldValue::Null,
        _ => FieldValue::NonString(doc.type_name(id)),
    }
}

fn extract_fields(doc: YamlDoc, root_map: &[(NodeId, NodeId)]) -> Frontmatter {
    let mut fm = Frontmatter {
        doc: YamlDoc::default(),
        keys: Vec::new(),
        key_lines: BTreeMap::new(),
        name: FieldValue::Absent,
        description: FieldValue::Absent,
        license: FieldValue::Absent,
        compatibility: FieldValue::Absent,
        allowed_tools: FieldValue::Absent,
        metadata: FieldValue::Absent,
        metadata_pairs: Vec::new(),
        unknown_fields: Vec::new(),
    };
    for (k, v) in root_map {
        let key_text = doc.node(*k).text.clone().unwrap_or_default();
        let is_str_key = doc.as_str(*k).is_some();
        fm.keys.push(key_text.clone());
        fm.key_lines.insert(key_text.clone(), doc.node(*k).line);
        let known = if is_str_key { key_text.as_str() } else { "" };
        match known {
            "name" => fm.name = field_value(&doc, *v),
            "description" => fm.description = field_value(&doc, *v),
            "license" => fm.license = field_value(&doc, *v),
            "compatibility" => fm.compatibility = field_value(&doc, *v),
            "allowed-tools" => fm.allowed_tools = field_value(&doc, *v),
            "metadata" => {
                fm.metadata = field_value(&doc, *v);
                if let NodeKind::Map(pairs) = &doc.node(*v).kind {
                    for (mk, mv) in pairs {
                        if let (Some(a), Some(b)) = (doc.as_str(*mk), doc.as_str(*mv)) {
                            fm.metadata_pairs.push((a.to_string(), b.to_string()));
                        }
                    }
                }
            }
            _ => fm.unknown_fields.push(key_text),
        }
    }
    fm.doc = doc;
    fm
}

fn field_finding(fm: &Frontmatter, code: DiagnosticCode, field: &str) -> Finding {
    let mut f = Finding::new(code).with_path(SKILL_MD).with_field(field);
    if let Some(line) = fm.line_of(field) {
        f = f.with_detail("line", line as u64);
    }
    f
}

fn type_of(v: &FieldValue) -> &'static str {
    match v {
        FieldValue::Absent => "absent",
        FieldValue::Null => "null",
        FieldValue::Str(_) => "string",
        FieldValue::NonString(t) => t,
    }
}

fn field_findings(fm: &Frontmatter, out: &mut Vec<Finding>) {
    // description: required, 1-1024 chars.
    match &fm.description {
        FieldValue::Absent | FieldValue::Null => out.push(field_finding(
            fm,
            DiagnosticCode::DescriptionMissing,
            "description",
        )),
        FieldValue::Str(s) if s.trim().is_empty() => out.push(
            field_finding(fm, DiagnosticCode::DescriptionMissing, "description")
                .with_detail("reason", "blank"),
        ),
        FieldValue::Str(s) => {
            let n = s.chars().count();
            if n as u64 > budgets::DESCRIPTION_SPEC_MAX_CHARS {
                out.push(
                    field_finding(fm, DiagnosticCode::DescriptionTooLong, "description")
                        .with_detail("chars", n as u64)
                        .with_detail("limit", budgets::DESCRIPTION_SPEC_MAX_CHARS),
                );
            }
        }
        FieldValue::NonString(t) => out.push(
            field_finding(fm, DiagnosticCode::DescriptionNotString, "description")
                .with_detail("yamlType", *t),
        ),
    }
    // license: optional string.
    if fm.license.is_present() && fm.license.as_str().is_none() {
        out.push(
            field_finding(fm, DiagnosticCode::LicenseNotString, "license")
                .with_detail("yamlType", type_of(&fm.license)),
        );
    }
    // compatibility: optional, 1-500 chars.
    match &fm.compatibility {
        FieldValue::Absent => {}
        FieldValue::Str(s) if s.is_empty() => out.push(
            field_finding(fm, DiagnosticCode::CompatibilityInvalid, "compatibility")
                .with_detail("reason", "empty"),
        ),
        FieldValue::Str(s) => {
            let n = s.chars().count();
            if n as u64 > budgets::COMPATIBILITY_SPEC_MAX_CHARS {
                out.push(
                    field_finding(fm, DiagnosticCode::CompatibilityInvalid, "compatibility")
                        .with_detail("reason", "tooLong")
                        .with_detail("chars", n as u64)
                        .with_detail("limit", budgets::COMPATIBILITY_SPEC_MAX_CHARS),
                );
            }
        }
        other => out.push(
            field_finding(fm, DiagnosticCode::CompatibilityInvalid, "compatibility")
                .with_detail("reason", "notString")
                .with_detail("yamlType", type_of(other)),
        ),
    }
    // metadata: optional string-to-string mapping.
    match &fm.metadata {
        FieldValue::Absent => {}
        FieldValue::NonString("mapping") => {
            let total = match fm.metadata_node().map(|id| &fm.doc.node(id).kind) {
                Some(NodeKind::Map(p)) => p.len(),
                _ => 0,
            };
            if total != fm.metadata_pairs.len() {
                out.push(
                    field_finding(fm, DiagnosticCode::MetadataInvalid, "metadata")
                        .with_detail("reason", "nonStringEntry")
                        .with_detail("nonStringEntries", (total - fm.metadata_pairs.len()) as u64),
                );
            }
        }
        other => out.push(
            field_finding(fm, DiagnosticCode::MetadataInvalid, "metadata")
                .with_detail("reason", "notMapping")
                .with_detail("yamlType", type_of(other)),
        ),
    }
    // allowed-tools: optional string (space separated, experimental).
    // 应用扩写键的合法值只读机读合同；未知值必须诊断，不能静默视作未声明。
    let contract: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../packages/app-contracts/v1/agent-skills.json"
    ))
    .expect("agent-skills contract");
    let declaration = &contract["expansionMetadata"];
    let key = declaration["key"].as_str().expect("expansion metadata key");
    let values_ref = declaration["allowedValuesRef"]
        .as_str()
        .expect("expansion values ref");
    let allowed = contract
        .pointer(
            values_ref
                .strip_prefix('#')
                .expect("local expansion values ref"),
        )
        .expect("expansion metadata values pointer")
        .as_array()
        .expect("expansion metadata values");
    for (name, value) in &fm.metadata_pairs {
        if name == key
            && !allowed
                .iter()
                .any(|item| item.as_str() == Some(value.as_str()))
        {
            out.push(
                field_finding(fm, DiagnosticCode::MetadataInvalid, "metadata")
                    .with_detail("key", key)
                    .with_detail("value", value.clone())
                    .with_message("扩写声明必须使用合同支持的值，请修正技能元数据。"),
            );
        }
    }
    if fm.allowed_tools.is_present() && fm.allowed_tools.as_str().is_none() {
        out.push(
            field_finding(fm, DiagnosticCode::AllowedToolsInvalid, "allowed-tools")
                .with_detail("yamlType", type_of(&fm.allowed_tools)),
        );
    }
    for k in &fm.unknown_fields {
        out.push(field_finding(fm, DiagnosticCode::UnknownField, k));
    }
}

impl Frontmatter {
    /// Node id of the `metadata` value, if present.
    pub fn metadata_node(&self) -> Option<NodeId> {
        let root = self.doc.root?;
        if let NodeKind::Map(pairs) = &self.doc.node(root).kind {
            for (k, v) in pairs {
                if self.doc.as_str(*k) == Some("metadata") {
                    return Some(*v);
                }
            }
        }
        None
    }
}

fn body_findings(body_raw: &str, body_start_line: usize, out: &mut Vec<Finding>) {
    let model = body_for_model(body_raw);
    if model.is_empty() {
        out.push(
            Finding::new(DiagnosticCode::BodyEmpty)
                .with_path(SKILL_MD)
                .with_detail("line", body_start_line as u64),
        );
        return;
    }
    let lines = model.lines().count();
    if lines as u64 > budgets::BODY_ADVISORY_MAX_LINES {
        out.push(
            Finding::new(DiagnosticCode::BodyLarge)
                .with_path(SKILL_MD)
                .with_detail("lines", lines as u64)
                .with_detail("limit", budgets::BODY_ADVISORY_MAX_LINES),
        );
    }
}

// ---------------------------------------------------------------------------------------------
// YAML events -> arena (iterative; SPEC 4.4)
// ---------------------------------------------------------------------------------------------

const CORE_TAG_PREFIX: &str = "tag:yaml.org,2002:";

enum Frame {
    Seq(NodeId),
    Map {
        id: NodeId,
        pending_key: Option<NodeId>,
        seen: HashSet<String>,
    },
}

fn yaml_invalid(message: &str, line: usize) -> Finding {
    Finding::new(DiagnosticCode::FrontmatterYamlInvalid)
        .with_path(SKILL_MD)
        .with_detail("line", line as u64)
        .with_detail("reason", message)
}

/// Core tag suffix (`str`, `int`, ...) of a tag, `Some("")` for the non-specific `!`, `None` otherwise.
fn core_tag_suffix(tag: &Tag) -> Option<&str> {
    if tag.handle == CORE_TAG_PREFIX {
        return Some(tag.suffix.as_str());
    }
    if tag.handle.is_empty() {
        if tag.suffix == "!" {
            return Some("");
        }
        if let Some(s) = tag.suffix.strip_prefix(CORE_TAG_PREFIX) {
            return Some(s);
        }
    }
    None
}

fn tag_text(tag: &Tag) -> String {
    format!("{}{}", tag.handle, tag.suffix)
}

fn is_dec_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

fn is_core_int(v: &str) -> bool {
    let unsigned = v.strip_prefix(['-', '+']).unwrap_or(v);
    if is_dec_digits(unsigned) {
        return true;
    }
    if let Some(o) = v.strip_prefix("0o") {
        return !o.is_empty() && o.bytes().all(|b| (b'0'..=b'7').contains(&b));
    }
    if let Some(h) = v.strip_prefix("0x") {
        return !h.is_empty() && h.bytes().all(|b| b.is_ascii_hexdigit());
    }
    false
}

fn is_core_float(v: &str) -> bool {
    let unsigned = v.strip_prefix(['-', '+']).unwrap_or(v);
    if matches!(unsigned, ".inf" | ".Inf" | ".INF") || matches!(v, ".nan" | ".NaN" | ".NAN") {
        return true;
    }
    // [-+]? ( \.[0-9]+ | [0-9]+ ( \.[0-9]* )? ) ( [eE] [-+]? [0-9]+ )?
    let (mantissa, exp) = match unsigned.find(['e', 'E']) {
        Some(p) => (&unsigned[..p], Some(&unsigned[p + 1..])),
        None => (unsigned, None),
    };
    if let Some(e) = exp {
        let e = e.strip_prefix(['-', '+']).unwrap_or(e);
        if !is_dec_digits(e) {
            return false;
        }
    }
    match mantissa.split_once('.') {
        None => is_dec_digits(mantissa),
        Some(("", frac)) => is_dec_digits(frac),
        Some((int, frac)) => is_dec_digits(int) && (frac.is_empty() || is_dec_digits(frac)),
    }
}

/// YAML 1.2 core schema resolution of a plain scalar.
pub fn resolve_plain(v: &str) -> Scalar {
    match v {
        "" | "~" | "null" | "Null" | "NULL" => Scalar::Null,
        "true" | "True" | "TRUE" => Scalar::Bool(true),
        "false" | "False" | "FALSE" => Scalar::Bool(false),
        _ if is_core_int(v) => Scalar::Int(v.to_string()),
        _ if is_core_float(v) => Scalar::Float(v.to_string()),
        _ => Scalar::Str(v.to_string()),
    }
}

fn resolve_scalar(
    value: &str,
    style: TScalarStyle,
    tag: Option<&Tag>,
    line: usize,
) -> Result<Scalar, Finding> {
    let natural = if style == TScalarStyle::Plain {
        resolve_plain(value)
    } else {
        Scalar::Str(value.to_string())
    };
    let Some(tag) = tag else {
        return Ok(natural);
    };
    let Some(suffix) = core_tag_suffix(tag) else {
        return Err(yaml_invalid("nonCoreTag", line).with_detail("tag", tag_text(tag)));
    };
    let ok = match suffix {
        "" | "str" => return Ok(Scalar::Str(value.to_string())),
        "null" => matches!(resolve_plain(value), Scalar::Null),
        "bool" => matches!(resolve_plain(value), Scalar::Bool(_)),
        "int" => matches!(resolve_plain(value), Scalar::Int(_)),
        "float" => matches!(resolve_plain(value), Scalar::Float(_) | Scalar::Int(_)),
        _ => false,
    };
    if !ok {
        return Err(yaml_invalid("tagMismatch", line).with_detail("tag", tag_text(tag)));
    }
    Ok(match suffix {
        "float" => Scalar::Float(value.to_string()),
        _ => resolve_plain(value),
    })
}

fn key_identity(node: &Node) -> String {
    match &node.kind {
        NodeKind::Scalar(Scalar::Str(s)) => format!("s:{s}"),
        NodeKind::Scalar(Scalar::Null) => "null".to_string(),
        NodeKind::Scalar(Scalar::Bool(b)) => format!("b:{b}"),
        NodeKind::Scalar(Scalar::Int(r)) | NodeKind::Scalar(Scalar::Float(r)) => format!("n:{r}"),
        _ => String::new(),
    }
}

/// Build the arena from YAML text. `line_offset` is added to library line numbers (1-based).
pub fn build_doc(src: &str, line_offset: usize) -> Result<YamlDoc, Finding> {
    let mut doc = YamlDoc::default();
    let mut stack: Vec<Frame> = Vec::new();
    let mut documents = 0usize;
    let mut parser = Parser::new_from_str(src);
    let line_of = |m: &Marker| m.line() + line_offset;
    loop {
        let (ev, mark) = match parser.next_token() {
            Ok(x) => x,
            Err(e) => {
                return Err(yaml_invalid("syntax", e.marker().line() + line_offset)
                    .with_detail("column", (e.marker().col() + 1) as u64)
                    .with_detail("parserMessage", e.info().to_string()));
            }
        };
        let line = line_of(&mark);
        let is_map_start = matches!(ev, Event::MappingStart(..));
        let new_node: Option<(Node, bool)> =
            match ev {
                Event::StreamEnd => break,
                Event::StreamStart | Event::Nothing | Event::DocumentEnd => None,
                Event::DocumentStart => {
                    documents += 1;
                    if documents > 1 {
                        return Err(yaml_invalid("multipleDocuments", line));
                    }
                    None
                }
                Event::Alias(_) => {
                    return Err(Finding::new(DiagnosticCode::FrontmatterAliasForbidden)
                        .with_path(SKILL_MD)
                        .with_detail("line", line as u64)
                        .with_detail("construct", "alias"));
                }
                Event::Scalar(value, style, anchor, tag) => {
                    if anchor > 0 {
                        return Err(Finding::new(DiagnosticCode::FrontmatterAliasForbidden)
                            .with_path(SKILL_MD)
                            .with_detail("line", line as u64)
                            .with_detail("construct", "anchor"));
                    }
                    let scalar = resolve_scalar(&value, style, tag.as_ref(), line)?;
                    Some((
                        Node {
                            kind: NodeKind::Scalar(scalar),
                            line,
                            text: Some(value),
                        },
                        false,
                    ))
                }
                Event::SequenceStart(anchor, tag) | Event::MappingStart(anchor, tag) => {
                    let is_map = is_map_start;
                    if anchor > 0 {
                        return Err(Finding::new(DiagnosticCode::FrontmatterAliasForbidden)
                            .with_path(SKILL_MD)
                            .with_detail("line", line as u64)
                            .with_detail("construct", "anchor"));
                    }
                    if let Some(t) = tag.as_ref() {
                        let want = if is_map { "map" } else { "seq" };
                        match core_tag_suffix(t) {
                            Some(s) if s == want || s.is_empty() => {}
                            Some(_) => {
                                return Err(yaml_invalid("tagMismatch", line)
                                    .with_detail("tag", tag_text(t)))
                            }
                            None => {
                                return Err(yaml_invalid("nonCoreTag", line)
                                    .with_detail("tag", tag_text(t)))
                            }
                        }
                    }
                    let kind = if is_map {
                        NodeKind::Map(Vec::new())
                    } else {
                        NodeKind::Seq(Vec::new())
                    };
                    Some((
                        Node {
                            kind,
                            line,
                            text: None,
                        },
                        true,
                    ))
                }
                Event::SequenceEnd | Event::MappingEnd => {
                    stack.pop();
                    None
                }
            };
        let Some((node, is_collection)) = new_node else {
            continue;
        };
        let id = doc.nodes.len();
        let node_line = node.line;
        doc.nodes.push(node);
        match stack.last_mut() {
            None => {
                if doc.root.is_some() {
                    return Err(yaml_invalid("multipleDocuments", node_line));
                }
                doc.root = Some(id);
            }
            Some(Frame::Seq(sid)) => {
                if let NodeKind::Seq(items) = &mut doc.nodes[*sid].kind {
                    items.push(id);
                }
            }
            Some(Frame::Map {
                id: mid,
                pending_key,
                seen,
            }) => match pending_key.take() {
                None => {
                    if is_collection {
                        return Err(yaml_invalid("collectionKey", node_line));
                    }
                    let key_node = &doc.nodes[id];
                    if key_node.text.as_deref() == Some("<<")
                        && matches!(key_node.kind, NodeKind::Scalar(Scalar::Str(_)))
                    {
                        return Err(Finding::new(DiagnosticCode::FrontmatterAliasForbidden)
                            .with_path(SKILL_MD)
                            .with_detail("line", node_line as u64)
                            .with_detail("construct", "mergeKey"));
                    }
                    let ident = key_identity(key_node);
                    if !seen.insert(ident) {
                        let mut f = Finding::new(DiagnosticCode::FrontmatterDuplicateKey)
                            .with_path(SKILL_MD)
                            .with_detail("line", node_line as u64)
                            .with_detail("key", key_node.text.clone().unwrap_or_default());
                        if stack.len() == 1 {
                            f = f.with_field(&doc.nodes[id].text.clone().unwrap_or_default());
                        }
                        return Err(f);
                    }
                    *pending_key = Some(id);
                }
                Some(k) => {
                    if let NodeKind::Map(pairs) = &mut doc.nodes[*mid].kind {
                        pairs.push((k, id));
                    }
                }
            },
        }
        if is_collection {
            let frame = if matches!(doc.nodes[id].kind, NodeKind::Map(_)) {
                Frame::Map {
                    id,
                    pending_key: None,
                    seen: HashSet::new(),
                }
            } else {
                Frame::Seq(id)
            };
            stack.push(frame);
        }
    }
    Ok(doc)
}
