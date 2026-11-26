//! Ceilings for one frozen closure.
//!
//! Measured 2026-11-15 on the author tree before source-map/README/CATALOG:
//! 220 files and 572,356 bytes including `source-map.json` (270,910 bytes).
//! The largest file that can enter a render closure is `shared/data/all_styles.json`
//! at 23,176 bytes. These ceilings sit above that closure and still reject
//! a multi-megabyte or unbounded read. Callers do not pick their own numbers.

/// One resource file, in bytes.
pub const MAX_FILE_BYTES: u64 = 262_144;
/// Distinct relative paths stored in one frozen set.
pub const MAX_FILES: usize = 512;
/// Sum of stored file bytes in one frozen set.
pub const MAX_TOTAL_BYTES: u64 = 1_048_576;
