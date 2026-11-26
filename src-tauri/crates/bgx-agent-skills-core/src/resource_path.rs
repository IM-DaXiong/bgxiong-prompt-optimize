//! P07: relative references and resource locations (SPEC sections 6.4 and 9.4, K02 `resourceRef`).
//!
//! * [`parse_resource_ref`]: the only path form path-restricted tools accept (package-root
//!   relative POSIX path; same rules as `agent-skills.json#/definitions/resourceRef`).
//! * [`scan_body_links`] / [`check_body_references`]: Markdown links in the `SKILL.md` body are
//!   resolved from the package root. Remote URLs are never treated as local files and are never
//!   downloaded; local links that do not resolve to a package file are reported with their line
//!   (`AGENT_SKILL_REFERENCE_MISSING`, warning only). The body is never rewritten.

use std::collections::BTreeSet;

use crate::{budgets, DiagnosticCode, Finding, SKILL_MD};

/// A validated package-root relative resource path.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ResourceRef(String);

impl ResourceRef {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Validate a `resourceRef` (read-time input from tools). Error: `AGENT_SKILL_RESOURCE_REF_INVALID`.
pub fn parse_resource_ref(s: &str) -> Result<ResourceRef, Finding> {
    let reason = resource_ref_problem(s);
    match reason {
        None => Ok(ResourceRef(s.to_string())),
        Some(r) => Err(Finding::new(DiagnosticCode::ResourceRefInvalid).with_detail("reason", r)),
    }
}

fn resource_ref_problem(s: &str) -> Option<&'static str> {
    let chars = s.chars().count() as u64;
    if chars == 0 {
        return Some("empty");
    }
    if chars > budgets::MAX_RELATIVE_PATH_BYTES || s.len() as u64 > budgets::MAX_RELATIVE_PATH_BYTES
    {
        return Some("tooLong");
    }
    let b = s.as_bytes();
    if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
        return Some("driveLetter");
    }
    if s.starts_with('/') {
        return Some("absolute");
    }
    if s.contains('\\') {
        return Some("backslash");
    }
    if s.chars().any(|c| (c as u32) < 0x20 || c as u32 == 0x7f) {
        return Some("controlCharacter");
    }
    if s.ends_with('/') || s.contains("//") {
        return Some("emptySegment");
    }
    if s.split('/').any(|seg| seg == "." || seg == "..") {
        return Some("dotSegment");
    }
    if s.split('/').count() as u64 > budgets::MAX_DIRECTORY_DEPTH {
        return Some("tooDeep");
    }
    None
}

/// Classification of one Markdown link destination.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkTarget {
    /// `scheme:` URL or protocol-relative `//host/...`: never a local file, never downloaded.
    Remote { scheme: String },
    /// `#fragment` within SKILL.md.
    Anchor,
    /// Package-local reference resolved from the package root.
    Local(LocalRef),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LocalRef {
    /// Normalized package-relative path (fragment/query removed, %-decoded).
    Resolved(String),
    /// Cannot be a file of this package; `reason` names why.
    Unresolvable { reason: &'static str },
}

/// One link found in the body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BodyLink {
    /// 1-based line in `SKILL.md`.
    pub line: usize,
    /// Destination exactly as written.
    pub raw: String,
    pub target: LinkTarget,
}

fn url_scheme(dest: &str) -> Option<&str> {
    let colon = dest.find(':')?;
    let scheme = &dest[..colon];
    let mut cs = scheme.chars();
    let first = cs.next()?;
    // A single letter before ':' is a Windows drive, not a URL scheme.
    if scheme.len() < 2 || !first.is_ascii_alphabetic() {
        return None;
    }
    if cs.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')) {
        Some(scheme)
    } else {
        None
    }
}

fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn unescape_markdown(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\\' {
            if let Some(n) = it.peek() {
                if n.is_ascii_punctuation() {
                    out.push(*n);
                    it.next();
                    continue;
                }
            }
        }
        out.push(c);
    }
    out
}

/// Classify a link destination relative to the package root (SKILL.md sits at the root).
pub fn classify_destination(raw: &str) -> LinkTarget {
    let dest = raw.trim();
    if dest.starts_with('#') {
        return LinkTarget::Anchor;
    }
    if dest.starts_with("//") {
        return LinkTarget::Remote {
            scheme: String::new(),
        };
    }
    if let Some(s) = url_scheme(dest) {
        if s.eq_ignore_ascii_case("file") {
            return LinkTarget::Local(LocalRef::Unresolvable { reason: "fileUrl" });
        }
        return LinkTarget::Remote {
            scheme: s.to_ascii_lowercase(),
        };
    }
    let path_part = dest.split(['#', '?']).next().unwrap_or("");
    let unescaped = unescape_markdown(path_part);
    let Some(decoded) = percent_decode(&unescaped) else {
        return LinkTarget::Local(LocalRef::Unresolvable {
            reason: "badPercentEncoding",
        });
    };
    let b = decoded.as_bytes();
    if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
        return LinkTarget::Local(LocalRef::Unresolvable {
            reason: "driveLetter",
        });
    }
    if decoded.starts_with('/') || decoded.starts_with('\\') {
        return LinkTarget::Local(LocalRef::Unresolvable { reason: "absolute" });
    }
    if decoded.contains('\\') {
        return LinkTarget::Local(LocalRef::Unresolvable {
            reason: "backslash",
        });
    }
    let mut segs: Vec<&str> = Vec::new();
    for seg in decoded.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                if segs.pop().is_none() {
                    return LinkTarget::Local(LocalRef::Unresolvable {
                        reason: "escapesPackage",
                    });
                }
            }
            s => segs.push(s),
        }
    }
    if segs.is_empty() {
        return LinkTarget::Local(LocalRef::Resolved(String::new()));
    }
    LinkTarget::Local(LocalRef::Resolved(segs.join("/")))
}

/// Remove inline code spans (backtick runs of equal length) from one line.
fn strip_inline_code(line: &str) -> String {
    let b: Vec<char> = line.chars().collect();
    let mut out = String::with_capacity(line.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == '`' {
            let mut n = 0;
            while i + n < b.len() && b[i + n] == '`' {
                n += 1;
            }
            // find a closing run of exactly n backticks
            let mut j = i + n;
            let mut close = None;
            while j < b.len() {
                if b[j] == '`' {
                    let mut m = 0;
                    while j + m < b.len() && b[j + m] == '`' {
                        m += 1;
                    }
                    if m == n {
                        close = Some(j + m);
                        break;
                    }
                    j += m;
                } else {
                    j += 1;
                }
            }
            match close {
                Some(end) => {
                    for _ in i..end {
                        out.push(' ');
                    }
                    i = end;
                }
                None => {
                    for _ in 0..n {
                        out.push('`');
                    }
                    i += n;
                }
            }
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    out
}

/// Parse a link destination starting right after `(`. Returns the raw destination.
fn parse_inline_destination(rest: &[char]) -> Option<String> {
    let mut i = 0;
    while i < rest.len() && rest[i] == ' ' {
        i += 1;
    }
    if i < rest.len() && rest[i] == '<' {
        let start = i + 1;
        let mut j = start;
        while j < rest.len() && rest[j] != '>' {
            if rest[j] == '\\' {
                j += 1;
            }
            j += 1;
        }
        if j >= rest.len() {
            return None;
        }
        return Some(rest[start..j].iter().collect());
    }
    // Bare destination with balanced parentheses; stops at whitespace (CommonMark).
    let start = i;
    let mut depth = 0i32;
    let mut j = start;
    while j < rest.len() {
        let c = rest[j];
        if c == '\\' && j + 1 < rest.len() {
            j += 2;
            continue;
        }
        if c == '(' {
            depth += 1;
        } else if c == ')' {
            if depth == 0 {
                break;
            }
            depth -= 1;
        } else if c.is_whitespace() {
            break;
        }
        j += 1;
    }
    let dest: String = rest[start..j].iter().collect();
    // What follows must be `)` or a title; otherwise authors wrote a path with raw spaces
    // (common on Windows): take everything up to the matching `)` so it is still checked.
    let mut k = j;
    while k < rest.len() && rest[k] == ' ' {
        k += 1;
    }
    if k < rest.len() && (rest[k] == ')' || rest[k] == '"' || rest[k] == '\'' || rest[k] == '(') {
        return Some(dest);
    }
    let mut depth = 0i32;
    let mut m = start;
    while m < rest.len() {
        match rest[m] {
            '(' => depth += 1,
            ')' if depth == 0 => {
                return Some(
                    rest[start..m]
                        .iter()
                        .collect::<String>()
                        .trim_end()
                        .to_string(),
                )
            }
            ')' => depth -= 1,
            _ => {}
        }
        m += 1;
    }
    None
}

/// Find Markdown links (`[t](dest)`, `![a](dest)`, `[id]: dest`) in the body, skipping fenced
/// code blocks and inline code. `body_start_line` is the 1-based SKILL.md line of the body.
pub fn scan_body_links(body: &str, body_start_line: usize) -> Vec<BodyLink> {
    let mut out = Vec::new();
    let mut fence: Option<(char, usize)> = None;
    for (idx, raw_line) in body.lines().enumerate() {
        let line_no = body_start_line + idx;
        let trimmed = raw_line.trim_start();
        let indent = raw_line.len() - trimmed.len();
        let fence_char = trimmed.chars().next().filter(|c| *c == '`' || *c == '~');
        if indent <= 3 {
            if let Some(fc) = fence_char {
                let n = trimmed.chars().take_while(|c| *c == fc).count();
                if n >= 3 {
                    match fence {
                        None => {
                            fence = Some((fc, n));
                            continue;
                        }
                        Some((open_c, open_n))
                            if open_c == fc && n >= open_n && trimmed[n..].trim().is_empty() =>
                        {
                            fence = None;
                            continue;
                        }
                        _ => {}
                    }
                }
            }
        }
        if fence.is_some() {
            continue;
        }
        // Reference definition: up to 3 spaces, `[label]: destination`.
        if indent <= 3 && trimmed.starts_with('[') {
            if let Some(close) = trimmed.find("]:") {
                if !trimmed[1..close].contains(']') && close > 1 {
                    let rest = trimmed[close + 2..].trim_start();
                    let dest = if let Some(r) = rest.strip_prefix('<') {
                        r.split('>').next().unwrap_or("").to_string()
                    } else {
                        rest.split_whitespace().next().unwrap_or("").to_string()
                    };
                    if !dest.is_empty() {
                        out.push(BodyLink {
                            line: line_no,
                            target: classify_destination(&dest),
                            raw: dest,
                        });
                    }
                    continue;
                }
            }
        }
        let line = strip_inline_code(raw_line);
        let chars: Vec<char> = line.chars().collect();
        let mut i = 0;
        while i + 1 < chars.len() {
            if chars[i] == ']' && chars[i + 1] == '(' && i > 0 && chars[i - 1] != '\\' {
                if let Some(dest) = parse_inline_destination(&chars[i + 2..]) {
                    if !dest.trim().is_empty() {
                        out.push(BodyLink {
                            line: line_no,
                            target: classify_destination(&dest),
                            raw: dest,
                        });
                    }
                }
            }
            i += 1;
        }
    }
    out
}

/// Check body links against the package file list (package-relative paths). Returns one
/// `AGENT_SKILL_REFERENCE_MISSING` warning per local link that does not resolve to a package
/// file or directory. Remote URLs and anchors are not checked.
pub fn check_body_references<'a, I>(
    body: &str,
    body_start_line: usize,
    package_paths: I,
) -> Vec<Finding>
where
    I: IntoIterator<Item = &'a str>,
{
    let files: BTreeSet<&str> = package_paths.into_iter().collect();
    let mut dirs: BTreeSet<String> = BTreeSet::new();
    for f in &files {
        let mut acc = String::new();
        let parts: Vec<&str> = f.split('/').collect();
        for p in &parts[..parts.len().saturating_sub(1)] {
            if !acc.is_empty() {
                acc.push('/');
            }
            acc.push_str(p);
            dirs.insert(acc.clone());
        }
    }
    let mut out = Vec::new();
    for link in scan_body_links(body, body_start_line) {
        let LinkTarget::Local(local) = &link.target else {
            continue;
        };
        let problem = match local {
            LocalRef::Resolved(p) if p.is_empty() => None,
            LocalRef::Resolved(p) if files.contains(p.as_str()) || dirs.contains(p) => None,
            LocalRef::Resolved(p) => Some(("notFound", Some(p.clone()))),
            LocalRef::Unresolvable { reason } => Some((*reason, None)),
        };
        if let Some((reason, resolved)) = problem {
            let mut f = Finding::new(DiagnosticCode::ReferenceMissing)
                .with_path(SKILL_MD)
                .with_detail("line", link.line as u64)
                .with_detail("target", link.raw.as_str())
                .with_detail("reason", reason);
            if let Some(r) = resolved {
                f = f.with_detail("resolved", r);
            }
            out.push(f);
        }
    }
    out
}
