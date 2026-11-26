//! P02: standard `name` rules, container-name derivation and the combined `SKILL.md` check
//! (SPEC sections 3.1, 5 and 7).
//!
//! Character rule matches the reference validator `skills-ref`: after NFKC every character is a
//! Unicode letter/number equal to its lowercase form, or `-`. The installed package identity
//! (stableId, see [`crate::identity`]) is separate from these checks; nothing here invents a
//! `local.` namespace or derives a name from anything but the frontmatter.

use unicode_normalization::char::is_combining_mark;
use unicode_normalization::UnicodeNormalization;

use crate::frontmatter::{parse_skill_md, FieldValue, FrontmatterOutcome, ParsedSkillMd};
use crate::{budgets, format_status, DiagnosticCode, Finding, FormatStatus, Profile, SKILL_MD};

/// How the user submitted the package; used only to derive the container name (SPEC section 2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Container<'a> {
    /// A selected directory; `wrapper` is the single sub-directory holding `SKILL.md`, if any.
    Directory {
        selected: &'a str,
        wrapper: Option<&'a str>,
    },
    /// A ZIP archive file name; `wrapper` is the single top-level directory, if any.
    Zip {
        file_name: &'a str,
        wrapper: Option<&'a str>,
    },
    /// A single `SKILL.md` file and the name of the directory containing it.
    SingleFile { parent_dir: &'a str },
}

/// Container name: wrapper dir > ZIP file stem > selected dir > single file's parent dir.
pub fn container_name(c: &Container<'_>) -> String {
    match c {
        Container::Directory {
            wrapper: Some(w), ..
        }
        | Container::Zip {
            wrapper: Some(w), ..
        } => (*w).to_string(),
        Container::Zip { file_name, .. } => match file_name.rfind('.') {
            Some(p) if p > 0 => file_name[..p].to_string(),
            _ => (*file_name).to_string(),
        },
        Container::Directory { selected, .. } => (*selected).to_string(),
        Container::SingleFile { parent_dir } => (*parent_dir).to_string(),
    }
}

/// Python `str.isalnum()` equivalent used by skills-ref: Unicode letters (L*) and numbers (N*).
/// Rust `is_alphabetic` also accepts Other_Alphabetic marks; those are excluded here.
fn is_letter_or_number(c: char) -> bool {
    (c.is_alphabetic() && !is_combining_mark(c)) || c.is_numeric()
}

fn name_finding(code: DiagnosticCode, line: Option<usize>) -> Finding {
    let mut f = Finding::new(code).with_path(SKILL_MD).with_field("name");
    if let Some(l) = line {
        f = f.with_detail("line", l as u64);
    }
    f
}

/// Validate the frontmatter `name` value. Returns the name (original text) when it is a usable,
/// safe string (possibly with warnings), plus findings.
pub fn validate_name(
    value: &FieldValue,
    container: Option<&str>,
    line: Option<usize>,
) -> (Option<String>, Vec<Finding>) {
    let mut out = Vec::new();
    let raw = match value {
        FieldValue::Absent | FieldValue::Null => {
            out.push(name_finding(DiagnosticCode::NameMissing, line));
            return (None, out);
        }
        FieldValue::NonString(t) => {
            out.push(name_finding(DiagnosticCode::NameNotString, line).with_detail("yamlType", *t));
            return (None, out);
        }
        FieldValue::Str(s) => s.as_str(),
    };
    if raw.trim().is_empty() {
        out.push(name_finding(DiagnosticCode::NameMissing, line).with_detail("reason", "blank"));
        return (None, out);
    }
    let chars = raw.chars().count();
    let unsafe_reason = if raw.contains('/') || raw.contains('\\') {
        Some("pathSeparator")
    } else if raw.chars().any(char::is_control) {
        Some("controlCharacter")
    } else if raw == "." || raw == ".." {
        Some("dotName")
    } else if chars as u64 > budgets::NAME_HARD_MAX_CHARS {
        Some("hardLimit")
    } else {
        None
    };
    if let Some(reason) = unsafe_reason {
        out.push(name_finding(DiagnosticCode::NameUnsafe, line).with_detail("reason", reason));
        return (None, out);
    }

    let nfkc: String = raw.nfkc().collect();
    let n = nfkc.chars().count();
    if n as u64 > budgets::NAME_SPEC_MAX_CHARS {
        out.push(
            name_finding(DiagnosticCode::NameTooLong, line)
                .with_detail("chars", n as u64)
                .with_detail("limit", budgets::NAME_SPEC_MAX_CHARS),
        );
    }
    let not_lower = nfkc.to_lowercase() != nfkc;
    let bad_chars: Vec<String> = nfkc
        .chars()
        .filter(|c| !(*c == '-' || is_letter_or_number(*c)))
        .map(|c| c.to_string())
        .collect();
    if not_lower || !bad_chars.is_empty() {
        let mut f = name_finding(DiagnosticCode::NameCharset, line);
        if not_lower {
            f = f.with_detail("uppercase", true);
        }
        if !bad_chars.is_empty() {
            f = f.with_detail("invalidCharacters", bad_chars);
        }
        out.push(f);
    }
    let edge = nfkc.starts_with('-') || nfkc.ends_with('-');
    let double = nfkc.contains("--");
    if edge || double {
        out.push(
            name_finding(DiagnosticCode::NameHyphen, line)
                .with_detail("edgeHyphen", edge)
                .with_detail("consecutiveHyphens", double),
        );
    }
    if let Some(dir) = container {
        let dir_nfkc: String = dir.nfkc().collect();
        if dir_nfkc != nfkc {
            out.push(
                name_finding(DiagnosticCode::NameDirMismatch, line)
                    .with_detail("containerName", dir),
            );
        }
    }
    if !raw.is_ascii() {
        out.push(name_finding(DiagnosticCode::NameNonAscii, line));
    }
    (Some(raw.to_string()), out)
}

/// Result of checking one `SKILL.md` (parse + fields + name + body).
#[derive(Clone, Debug, PartialEq)]
pub struct SkillMdCheck<'a> {
    pub parsed: Option<ParsedSkillMd<'a>>,
    /// Frontmatter `name` when usable (import profile may still carry warnings).
    pub name: Option<String>,
    pub findings: Vec<Finding>,
}

impl SkillMdCheck<'_> {
    pub fn format_status(&self, profile: Profile) -> FormatStatus {
        format_status(&self.findings, profile)
    }
    pub fn diagnostics_json(&self, profile: Profile) -> Vec<serde_json::Value> {
        self.findings
            .iter()
            .map(|f| f.to_diagnostic_json(profile))
            .collect()
    }
}

/// Check `SKILL.md` bytes against SPEC sections 4-6 for the given container name.
pub fn check_skill_md<'a>(bytes: &'a [u8], container: Option<&str>) -> SkillMdCheck<'a> {
    let FrontmatterOutcome {
        parsed,
        mut findings,
    } = parse_skill_md(bytes);
    let mut name = None;
    if let Some(p) = &parsed {
        let line = p.frontmatter.line_of("name");
        let (n, mut f) = validate_name(&p.frontmatter.name, container, line);
        name = n;
        // name findings first: they are the most actionable for authors.
        f.append(&mut findings);
        findings = f;
    }
    SkillMdCheck {
        parsed,
        name,
        findings,
    }
}
