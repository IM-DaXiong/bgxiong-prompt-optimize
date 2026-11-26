//! P04: whole-package fingerprint `packageDigest` v1 (SPEC section 8.2, K02 `packageDigest`).
//!
//! ```text
//! lines  = for each regular file (sorted by relpath UTF-8 bytes):
//!            lowercase_hex(sha256(bytes)) + "  " + relpath + "\n"
//! digest = "sha256:" + lowercase_hex(sha256(domainSeparator + concat(lines)))
//! ```
//!
//! Only file paths and contents count: no mtime, permissions, archive parameters, registration
//! data or derived indexes. The name is not an install key and does not enter the digest except
//! as part of `SKILL.md` content.

use sha2::{Digest, Sha256};

use crate::resources::ResourceEntry;
use crate::{sha256_hex, DIGEST_DOMAIN_SEPARATOR, DIGEST_PREFIX};

fn finish(mut lines: Vec<(Vec<u8>, String)>) -> String {
    lines.sort_by(|a, b| a.0.cmp(&b.0));
    let mut h = Sha256::new();
    h.update(DIGEST_DOMAIN_SEPARATOR.as_bytes());
    for (path, file_hex) in &lines {
        h.update(file_hex.as_bytes());
        h.update(b"  ");
        h.update(path);
        h.update(b"\n");
    }
    let d = h.finalize();
    let mut s = String::with_capacity(DIGEST_PREFIX.len() + 64);
    s.push_str(DIGEST_PREFIX);
    for b in d.iter() {
        s.push(char::from_digit((b >> 4) as u32, 16).unwrap_or('0'));
        s.push(char::from_digit((b & 0x0f) as u32, 16).unwrap_or('0'));
    }
    s
}

/// packageDigest of `(relpath, bytes)` pairs in any order. Paths must already be normalized
/// package-relative paths (SPEC 3.2); duplicates are the caller's error (see `build_manifest`).
pub fn package_digest<'a, I>(files: I) -> String
where
    I: IntoIterator<Item = (&'a str, &'a [u8])>,
{
    finish(
        files
            .into_iter()
            .map(|(p, b)| (p.as_bytes().to_vec(), sha256_hex(b)))
            .collect(),
    )
}

/// packageDigest from manifest entries (reuses their per-file sha256; no re-hashing).
pub fn package_digest_of_entries(entries: &[ResourceEntry]) -> String {
    finish(
        entries
            .iter()
            .map(|e| (e.resource_ref.as_bytes().to_vec(), e.sha256.clone()))
            .collect(),
    )
}

/// `true` when `s` has the contract form `sha256:<64 lowercase hex>`.
pub fn is_package_digest(s: &str) -> bool {
    match s.strip_prefix(DIGEST_PREFIX) {
        Some(h) => h.len() == 64 && h.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')),
        None => false,
    }
}
