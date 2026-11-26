//! Memory-only Agent Skills container validation, shared by both hosts.
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read};

use bgx_agent_skills_core::resources::{build_manifest, PackageFile, ResourceManifest};
use bgx_agent_skills_core::validation::{container_name, Container};
use bgx_agent_skills_core::{budgets, DiagnosticCode, Finding, Profile, Severity, SKILL_MD};

pub const MACOSX_DIR: &str = "__MACOSX";

/// How the package was submitted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContainerKind {
    Directory,
    Zip,
    SingleFile,
}

/// One package file: package-relative `/` path (verbatim, never renamed) and its bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackageFileBytes {
    pub path: String,
    pub bytes: Vec<u8>,
}

/// A package that passed every container, path and budget rule.
#[derive(Clone, Debug)]
pub struct ReadPackage {
    pub kind: ContainerKind,
    /// Container name used for the `name` / directory check (SPEC section 2).
    pub container_name: String,
    /// All package files sorted by path UTF-8 bytes.
    pub files: Vec<PackageFileBytes>,
    pub manifest: ResourceManifest,
    /// Non-error notes (for example `AGENT_SKILL_MACOSX_IGNORED`, `AGENT_SKILL_ZIP_BACKSLASH_PATH`).
    pub notes: Vec<Finding>,
}

impl ReadPackage {
    pub fn get(&self, path: &str) -> Option<&[u8]> {
        self.files
            .binary_search_by(|f| f.path.as_bytes().cmp(path.as_bytes()))
            .ok()
            .map(|i| self.files[i].bytes.as_slice())
    }
}

/// Rejected package: every finding collected (at least one is an error in both profiles).
#[derive(Clone, Debug, PartialEq)]
pub struct ReadError {
    pub findings: Vec<Finding>,
}

impl ReadError {
    pub fn one(f: Finding) -> Self {
        ReadError { findings: vec![f] }
    }

    pub fn codes(&self) -> Vec<&'static str> {
        self.findings.iter().map(|f| f.code.as_str()).collect()
    }

    pub fn has(&self, code: DiagnosticCode) -> bool {
        self.findings.iter().any(|f| f.code == code)
    }

    pub fn many(findings: Vec<Finding>) -> Self {
        ReadError { findings }
    }
}

impl From<Finding> for ReadError {
    fn from(f: Finding) -> Self {
        ReadError::one(f)
    }
}

impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "agent skill package rejected: {}",
            self.codes().join(", ")
        )
    }
}

impl std::error::Error for ReadError {}

pub fn limit(key: &str, limit: u64, actual: u64) -> Finding {
    Finding::new(DiagnosticCode::LimitExceeded)
        .with_detail("limitKey", key)
        .with_detail("limit", limit)
        .with_detail("actual", actual)
}

pub fn corrupt(reason: &str) -> Finding {
    Finding::new(DiagnosticCode::ArchiveCorrupt).with_detail("reason", reason)
}

pub fn is_error(f: &Finding) -> bool {
    // Container/path rules are errors in both profiles; the import profile is the lenient one.
    f.severity(Profile::Import) == Severity::Error
}

fn le16(b: &[u8], at: usize) -> Option<u64> {
    b.get(at..at + 2)
        .map(|s| u16::from_le_bytes([s[0], s[1]]) as u64)
}

fn le32(b: &[u8], at: usize) -> Option<u64> {
    b.get(at..at + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]) as u64)
}

fn le64(b: &[u8], at: usize) -> Option<u64> {
    b.get(at..at + 8).map(|s| {
        let mut a = [0u8; 8];
        a.copy_from_slice(s);
        u64::from_le_bytes(a)
    })
}

/// Raw entry names from the central directory, in order.
///
/// The `zip` crate keys entries by decoded name and silently keeps one of two duplicates, and it
/// decodes names without the UTF-8 flag as CP437. SPEC 3.2 needs the raw bytes (duplicates are
/// `AGENT_SKILL_DUPLICATE_ENTRY`, non-UTF-8 names are `AGENT_SKILL_PATH_ENCODING_UNSUPPORTED`),
/// so the central directory is walked here; the `zip` crate is used only to inflate data.
fn central_directory_names(b: &[u8]) -> Result<Vec<Vec<u8>>, &'static str> {
    const EOCD: u64 = 0x0605_4b50;
    const ZIP64_LOCATOR: u64 = 0x0706_4b50;
    const ZIP64_EOCD: u64 = 0x0606_4b50;
    const CDH: u64 = 0x0201_4b50;
    if b.len() < 22 {
        return Err("tooShort");
    }
    let lowest = b.len().saturating_sub(22 + 0xFFFF);
    let mut pos = b.len() - 22;
    let eocd = loop {
        if le32(b, pos) == Some(EOCD) {
            let comment = le16(b, pos + 20).unwrap_or(u64::MAX) as usize;
            if pos + 22 + comment <= b.len() {
                break pos;
            }
        }
        if pos == lowest {
            return Err("endOfCentralDirectoryMissing");
        }
        pos -= 1;
    };
    let mut count = le16(b, eocd + 10).ok_or("eocd")?;
    let mut cd_size = le32(b, eocd + 12).ok_or("eocd")?;
    let cd_offset = le32(b, eocd + 16).ok_or("eocd")?;
    let mut cd_end = eocd;
    if count == 0xFFFF || cd_size == 0xFFFF_FFFF || cd_offset == 0xFFFF_FFFF {
        let loc = eocd.checked_sub(20).ok_or("zip64LocatorMissing")?;
        if le32(b, loc) != Some(ZIP64_LOCATOR) {
            return Err("zip64LocatorMissing");
        }
        let rec = loc.checked_sub(56).ok_or("zip64RecordMissing")?;
        if le32(b, rec) != Some(ZIP64_EOCD) {
            return Err("zip64RecordMissing");
        }
        count = le64(b, rec + 32).ok_or("zip64Record")?;
        cd_size = le64(b, rec + 40).ok_or("zip64Record")?;
        cd_end = rec;
    }
    let cd_size = usize::try_from(cd_size).map_err(|_| "centralDirectoryOutOfRange")?;
    let cd_start = cd_end
        .checked_sub(cd_size)
        .ok_or("centralDirectoryOutOfRange")?;
    if count.saturating_mul(46) > cd_size as u64 {
        return Err("entryCountInconsistent");
    }
    let mut names = Vec::with_capacity(count as usize);
    let mut p = cd_start;
    for _ in 0..count {
        if le32(b, p) != Some(CDH) || p + 46 > cd_end {
            return Err("centralHeaderInvalid");
        }
        let n = le16(b, p + 28).ok_or("centralHeaderInvalid")? as usize;
        let e = le16(b, p + 30).ok_or("centralHeaderInvalid")? as usize;
        let c = le16(b, p + 32).ok_or("centralHeaderInvalid")? as usize;
        let end = p + 46 + n + e + c;
        if end > cd_end {
            return Err("centralHeaderInvalid");
        }
        names.push(b[p + 46..p + 46 + n].to_vec());
        p = end;
    }
    Ok(names)
}

struct ZipEntry {
    index: usize,
    name: String,
    is_dir: bool,
    size: u64,
}

/// Read a ZIP archive held in memory (SPEC 3.1 rule 2 and 3.2).
pub fn read_zip_bytes(file_name: &str, bytes: &[u8]) -> Result<ReadPackage, ReadError> {
    if bytes.len() as u64 > budgets::MAX_ARCHIVE_BYTES {
        return Err(ReadError::one(limit(
            budgets::MAX_ARCHIVE_BYTES_KEY,
            budgets::MAX_ARCHIVE_BYTES,
            bytes.len() as u64,
        )));
    }
    let raw_names = central_directory_names(bytes).map_err(|r| ReadError::one(corrupt(r)))?;
    let mut findings = Vec::new();
    let mut notes = Vec::new();
    let mut names = Vec::with_capacity(raw_names.len());
    let mut backslash = 0u64;
    for raw in &raw_names {
        match std::str::from_utf8(raw) {
            Ok(s) => {
                if s.contains('\\') {
                    backslash += 1;
                }
                names.push(Some(s.replace('\\', "/")));
            }
            Err(_) => {
                findings.push(
                    Finding::new(DiagnosticCode::PathEncodingUnsupported)
                        .with_detail("rawPath", String::from_utf8_lossy(raw).into_owned()),
                );
                names.push(None);
            }
        }
    }
    if backslash > 0 {
        notes.push(Finding::new(DiagnosticCode::ZipBackslashPath).with_detail("count", backslash));
    }
    let mut seen = BTreeSet::new();
    for n in names.iter().flatten() {
        if !seen.insert(n.as_str()) {
            findings.push(
                Finding::new(DiagnosticCode::DuplicateEntry).with_detail("rawPath", n.clone()),
            );
        }
    }
    if findings.iter().any(is_error) {
        return Err(ReadError { findings });
    }
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| ReadError::one(corrupt("open").with_detail("error", e.to_string())))?;
    if archive.len() != names.len() {
        return Err(ReadError::one(corrupt("entryIndexMismatch")));
    }
    let mut entries = Vec::new();
    let mut macosx = false;
    let mut file_count = 0u64;
    let mut total = 0u64;
    let mut compressed_total = 0u64;
    for (index, name) in names.into_iter().enumerate() {
        let name = name.unwrap_or_default();
        if name.split('/').next() == Some(MACOSX_DIR) {
            macosx = true;
            continue;
        }
        let zf = archive.by_index_raw(index).map_err(|e| {
            ReadError::one(corrupt("entryHeader").with_detail("error", e.to_string()))
        })?;
        let is_dir = name.ends_with('/');
        if zf.encrypted() {
            findings.push(corrupt("encrypted").with_detail("rawPath", name.clone()));
            continue;
        }
        if zf.is_symlink() {
            findings.push(
                Finding::new(DiagnosticCode::SymlinkForbidden).with_detail("rawPath", name.clone()),
            );
            continue;
        }
        if is_dir {
            entries.push(ZipEntry {
                index,
                name,
                is_dir,
                size: 0,
            });
            continue;
        }
        match zf.compression() {
            zip::CompressionMethod::Stored | zip::CompressionMethod::Deflated => {}
            other => {
                findings.push(
                    corrupt("unsupportedCompression")
                        .with_detail("rawPath", name.clone())
                        .with_detail("method", format!("{other:?}")),
                );
                continue;
            }
        }
        file_count += 1;
        let size = zf.size();
        if size > budgets::MAX_SINGLE_FILE_BYTES {
            findings.push(
                limit(
                    budgets::MAX_SINGLE_FILE_BYTES_KEY,
                    budgets::MAX_SINGLE_FILE_BYTES,
                    size,
                )
                .with_detail("rawPath", name.clone()),
            );
        }
        total = total.saturating_add(size);
        compressed_total = compressed_total.saturating_add(zf.compressed_size());
        entries.push(ZipEntry {
            index,
            name,
            is_dir,
            size,
        });
    }
    if macosx {
        notes.push(Finding::new(DiagnosticCode::MacosxIgnored));
    }
    if file_count > budgets::MAX_FILE_COUNT {
        findings.push(limit(
            budgets::MAX_FILE_COUNT_KEY,
            budgets::MAX_FILE_COUNT,
            file_count,
        ));
    }
    if total > budgets::MAX_TOTAL_UNCOMPRESSED_BYTES {
        findings.push(limit(
            budgets::MAX_TOTAL_UNCOMPRESSED_BYTES_KEY,
            budgets::MAX_TOTAL_UNCOMPRESSED_BYTES,
            total,
        ));
    }
    let ratio = total / compressed_total.max(1);
    if total > 0 && ratio > budgets::MAX_COMPRESSION_RATIO {
        findings.push(limit(
            budgets::MAX_COMPRESSION_RATIO_KEY,
            budgets::MAX_COMPRESSION_RATIO,
            ratio,
        ));
    }
    if findings.iter().any(is_error) {
        findings.extend(notes);
        return Err(ReadError { findings });
    }

    // Package root (SPEC 3.1 rule 2): archive root, else the unique top directory holding
    // SKILL.md that contains every entry.
    let has_file = |p: &str| entries.iter().any(|e| !e.is_dir && e.name == p);
    let wrapper = if has_file(SKILL_MD) {
        None
    } else {
        let mut tops: BTreeMap<&str, bool> = BTreeMap::new();
        let mut loose = false;
        for e in &entries {
            match e.name.split_once('/') {
                Some((top, _)) => {
                    tops.entry(top).or_insert(false);
                }
                None => loose = true,
            }
        }
        let candidates: Vec<String> = tops
            .keys()
            .filter(|t| has_file(&format!("{t}/{SKILL_MD}")))
            .map(|t| t.to_string())
            .collect();
        if candidates.len() > 1 {
            return Err(ReadError::one(
                Finding::new(DiagnosticCode::MultipleRoots).with_detail("roots", candidates),
            ));
        }
        match candidates.first() {
            Some(c) if !loose && tops.len() == 1 => Some(c.clone()),
            Some(c) => {
                return Err(ReadError::one(
                    Finding::new(DiagnosticCode::SkillMdMissing)
                        .with_detail("reason", "entriesOutsideWrapper")
                        .with_detail("wrapper", c.clone()),
                ))
            }
            None => return Err(ReadError::one(Finding::new(DiagnosticCode::SkillMdMissing))),
        }
    };
    let prefix = wrapper.as_ref().map(|w| format!("{w}/"));
    let mut files = Vec::with_capacity(entries.len());
    for e in entries.iter().filter(|e| !e.is_dir) {
        let mut zf = archive.by_index(e.index).map_err(|err| {
            ReadError::one(
                corrupt("entryData")
                    .with_detail("rawPath", e.name.clone())
                    .with_detail("error", err.to_string()),
            )
        })?;
        let mut buf = Vec::with_capacity(e.size as usize);
        // Never inflate more than the budgeted (declared) size, whatever the stream claims.
        (&mut zf)
            .take(e.size + 1)
            .read_to_end(&mut buf)
            .map_err(|err| {
                ReadError::one(
                    corrupt("entryData")
                        .with_detail("rawPath", e.name.clone())
                        .with_detail("error", err.to_string()),
                )
            })?;
        if buf.len() as u64 != e.size {
            return Err(ReadError::one(
                corrupt("sizeMismatch").with_detail("rawPath", e.name.clone()),
            ));
        }
        let rel = match &prefix {
            Some(p) => e
                .name
                .strip_prefix(p.as_str())
                .unwrap_or(&e.name)
                .to_string(),
            None => e.name.clone(),
        };
        files.push((rel, buf));
    }
    let name = container_name(&Container::Zip {
        file_name,
        wrapper: wrapper.as_deref(),
    });
    finish(ContainerKind::Zip, name, files, notes)
}

// ---------------------------------------------------------------------------------------------

/// Run the core path / budget / collision rules over the collected files.
pub fn finish(
    kind: ContainerKind,
    container_name: String,
    mut raw: Vec<(String, Vec<u8>)>,
    mut notes: Vec<Finding>,
) -> Result<ReadPackage, ReadError> {
    raw.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    let package_files: Vec<PackageFile<'_>> = raw
        .iter()
        .map(|(p, b)| PackageFile {
            path: p.as_str(),
            bytes: b.as_slice(),
        })
        .collect();
    let (manifest, findings) = build_manifest(&package_files);
    if findings.iter().any(is_error) {
        let mut all = findings;
        all.append(&mut notes);
        return Err(ReadError { findings: all });
    }
    notes.extend(findings);
    let files = raw
        .into_iter()
        .map(|(path, bytes)| PackageFileBytes { path, bytes })
        .collect();
    Ok(ReadPackage {
        kind,
        container_name,
        files,
        manifest,
        notes,
    })
}

