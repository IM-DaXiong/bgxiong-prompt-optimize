pub mod archive;
pub mod catalog;
pub mod index;
pub mod package;

use sha2::{Digest, Sha256};

pub const INDEX_MAX_BYTES: u64 = 1 << 20;
pub const PACKAGE_MAX_BYTES: u64 = 256 << 20;

#[derive(Debug)]
pub struct DistributionError {
    pub code: &'static str,
    pub message: String,
}

pub(crate) fn error(code: &'static str, detail: impl std::fmt::Display) -> DistributionError {
    DistributionError { code, message: detail.to_string() }
}

pub fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
}
