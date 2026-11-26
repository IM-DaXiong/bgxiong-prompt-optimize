//! P03: generic resource manifest (SPEC sections 3.2, 6 and K02 `resourceEntry`).
//!
//! Every regular file under the package root is a resource; `scripts/`, `references/` and
//! `assets/` are conventions, not a whitelist. A resource is described by its package-relative
//! path, size, text/binary kind and SHA-256. Binary content is never treated as instructions.

use std::collections::BTreeMap;

use unicode_normalization::UnicodeNormalization;

use crate::{budgets, sha256_hex, DiagnosticCode, Finding, ResourceKind, SKILL_MD};

/// One input file: package-relative path (already `/`-separated by the container layer) and bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PackageFile<'a> {
    pub path: &'a str,
    pub bytes: &'a [u8],
}

/// Contract `resourceEntry` plus an advisory media type (not part of the IPC shape).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourceEntry {
    pub resource_ref: String,
    pub kind: ResourceKind,
    pub bytes: u64,
    pub sha256: String,
    /// Guessed from the file extension only; `None` when unknown. Display hint, never a gate.
    pub media_type: Option<&'static str>,
}

impl ResourceEntry {
    /// Contract-shaped JSON (`agent-skills.json#/definitions/resourceEntry`).
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "resourceRef": self.resource_ref,
            "kind": self.kind.as_str(),
            "bytes": self.bytes,
            "sha256": self.sha256,
        })
    }
}

/// Manifest of a whole package, sorted by path UTF-8 bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourceManifest {
    pub entries: Vec<ResourceEntry>,
    pub total_bytes: u64,
}

impl ResourceManifest {
    pub fn get(&self, resource_ref: &str) -> Option<&ResourceEntry> {
        self.entries
            .binary_search_by(|e| e.resource_ref.as_bytes().cmp(resource_ref.as_bytes()))
            .ok()
            .map(|i| &self.entries[i])
    }
}

const RESERVED_DEVICE_NAMES: [&str; 4] = ["CON", "PRN", "AUX", "NUL"];

fn is_reserved_device(segment: &str) -> bool {
    let stem = segment
        .split('.')
        .next()
        .unwrap_or(segment)
        .trim_end_matches(' ');
    let upper = stem.to_ascii_uppercase();
    if RESERVED_DEVICE_NAMES.contains(&upper.as_str()) {
        return true;
    }
    for prefix in ["COM", "LPT"] {
        if let Some(d) = upper.strip_prefix(prefix) {
            if d.len() == 1 && matches!(d.as_bytes()[0], b'1'..=b'9') {
                return true;
            }
        }
    }
    false
}

fn path_finding(code: DiagnosticCode, raw: &str) -> Finding {
    // `path` must be a valid resourceRef, so unsafe raw paths only go into details.
    Finding::new(code).with_detail("rawPath", raw)
}

/// Validate one package-relative path (SPEC 3.2). Returns every violated rule.
pub fn check_package_path(path: &str) -> Vec<Finding> {
    let mut out = Vec::new();
    let unsafe_reason = if path.is_empty() {
        Some("empty")
    } else if path.starts_with('/') || path.starts_with('\\') {
        Some("absolute")
    } else if path.len() >= 2
        && path.as_bytes()[1] == b':'
        && path.as_bytes()[0].is_ascii_alphabetic()
    {
        Some("driveLetter")
    } else if path.contains('\\') {
        Some("backslash")
    } else if path.chars().any(|c| c.is_control()) {
        Some("controlCharacter")
    } else if path.split('/').any(|s| s.is_empty()) {
        Some("emptySegment")
    } else if path.split('/').any(|s| s == "." || s == "..") {
        Some("dotSegment")
    } else {
        None
    };
    if let Some(r) = unsafe_reason {
        out.push(path_finding(DiagnosticCode::PathUnsafe, path).with_detail("reason", r));
        return out;
    }
    for seg in path.split('/') {
        if is_reserved_device(seg) {
            out.push(
                path_finding(DiagnosticCode::PathReservedName, path)
                    .with_detail("segment", seg)
                    .with_detail("reason", "deviceName"),
            );
        } else if seg.ends_with('.') || seg.ends_with(' ') {
            out.push(
                path_finding(DiagnosticCode::PathReservedName, path)
                    .with_detail("segment", seg)
                    .with_detail("reason", "trailingDotOrSpace"),
            );
        }
        if seg.len() as u64 > budgets::MAX_PATH_SEGMENT_BYTES {
            out.push(
                limit(
                    budgets::MAX_PATH_SEGMENT_BYTES_KEY,
                    budgets::MAX_PATH_SEGMENT_BYTES,
                    seg.len() as u64,
                )
                .with_detail("rawPath", path),
            );
        }
    }
    let depth = path.split('/').count() as u64;
    if depth > budgets::MAX_DIRECTORY_DEPTH {
        out.push(
            limit(
                budgets::MAX_DIRECTORY_DEPTH_KEY,
                budgets::MAX_DIRECTORY_DEPTH,
                depth,
            )
            .with_detail("rawPath", path),
        );
    }
    if path.len() as u64 > budgets::MAX_RELATIVE_PATH_BYTES {
        out.push(
            limit(
                budgets::MAX_RELATIVE_PATH_BYTES_KEY,
                budgets::MAX_RELATIVE_PATH_BYTES,
                path.len() as u64,
            )
            .with_detail("rawPath", path),
        );
    }
    out
}

fn limit(key: &str, limit: u64, actual: u64) -> Finding {
    Finding::new(DiagnosticCode::LimitExceeded)
        .with_detail("limitKey", key)
        .with_detail("limit", limit)
        .with_detail("actual", actual)
}

/// Key used for case-collision detection: NFC + lowercase (paths themselves are never renamed).
pub fn collision_key(path: &str) -> String {
    path.nfc().collect::<String>().to_lowercase()
}

/// `text` when the bytes are valid UTF-8 without NUL bytes, otherwise `binary`.
pub fn classify(bytes: &[u8]) -> ResourceKind {
    if !bytes.contains(&0) && std::str::from_utf8(bytes).is_ok() {
        ResourceKind::Text
    } else {
        ResourceKind::Binary
    }
}

/// Advisory media type from the extension (case-insensitive). Not a whitelist.
pub fn media_type_for(path: &str) -> Option<&'static str> {
    let name = path.rsplit('/').next().unwrap_or(path);
    let ext = name.rsplit_once('.')?.1.to_ascii_lowercase();
    Some(match ext.as_str() {
        "md" | "markdown" => "text/markdown",
        "txt" => "text/plain",
        "json" => "application/json",
        "yaml" | "yml" => "application/yaml",
        "toml" => "application/toml",
        "csv" => "text/csv",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "xml" => "application/xml",
        "js" | "mjs" | "cjs" => "text/javascript",
        "ts" => "text/typescript",
        "py" => "text/x-python",
        "sh" => "text/x-shellscript",
        "ps1" => "text/plain",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "wasm" => "application/wasm",
        _ => return None,
    })
}

/// Build the manifest for a whole package and report path/budget/collision violations.
/// Files with unsafe paths are reported and left out of the manifest.
pub fn build_manifest(files: &[PackageFile<'_>]) -> (ResourceManifest, Vec<Finding>) {
    let mut findings = Vec::new();
    if files.len() as u64 > budgets::MAX_FILE_COUNT {
        findings.push(limit(
            budgets::MAX_FILE_COUNT_KEY,
            budgets::MAX_FILE_COUNT,
            files.len() as u64,
        ));
    }
    let mut by_path: BTreeMap<&[u8], &PackageFile<'_>> = BTreeMap::new();
    let mut collisions: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    let mut total: u64 = 0;
    for f in files {
        let path_findings = check_package_path(f.path);
        let path_ok = path_findings.is_empty();
        let unsafe_path = path_findings
            .iter()
            .any(|x| x.code == DiagnosticCode::PathUnsafe);
        findings.extend(path_findings);
        if unsafe_path {
            continue;
        }
        let locate = |x: Finding| {
            if path_ok {
                x.with_path(f.path)
            } else {
                x.with_detail("rawPath", f.path)
            }
        };
        if by_path.insert(f.path.as_bytes(), f).is_some() {
            findings.push(locate(Finding::new(DiagnosticCode::DuplicateEntry)));
            continue;
        }
        collisions
            .entry(collision_key(f.path))
            .or_default()
            .push(f.path);
        let n = f.bytes.len() as u64;
        if n > budgets::MAX_SINGLE_FILE_BYTES {
            findings.push(locate(limit(
                budgets::MAX_SINGLE_FILE_BYTES_KEY,
                budgets::MAX_SINGLE_FILE_BYTES,
                n,
            )));
        }
        total = total.saturating_add(n);
    }
    if total > budgets::MAX_TOTAL_UNCOMPRESSED_BYTES {
        findings.push(limit(
            budgets::MAX_TOTAL_UNCOMPRESSED_BYTES_KEY,
            budgets::MAX_TOTAL_UNCOMPRESSED_BYTES,
            total,
        ));
    }
    for (key, paths) in &collisions {
        if paths.len() > 1 {
            let mut sorted: Vec<&str> = paths.clone();
            sorted.sort_unstable();
            findings.push(
                Finding::new(DiagnosticCode::PathCaseCollision)
                    .with_detail(
                        "paths",
                        sorted.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
                    )
                    .with_detail("collisionKey", key.as_str()),
            );
        }
    }
    if !by_path.contains_key(SKILL_MD.as_bytes()) {
        findings.push(Finding::new(DiagnosticCode::SkillMdMissing));
    }
    let entries = by_path
        .values()
        .map(|f| ResourceEntry {
            resource_ref: f.path.to_string(),
            kind: classify(f.bytes),
            bytes: f.bytes.len() as u64,
            sha256: sha256_hex(f.bytes),
            media_type: media_type_for(f.path),
        })
        .collect();
    (
        ResourceManifest {
            entries,
            total_bytes: total,
        },
        findings,
    )
}
