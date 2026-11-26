use std::collections::BTreeSet;
use serde::Deserialize;

use super::{error, is_sha256, DistributionError, PACKAGE_MAX_BYTES};
use crate::index;

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Release {
    pub package_id: String,
    pub version: semver::Version,
    pub display_name: String,
    pub new_install: bool,
    pub host_version_range: semver::VersionReq,
    pub upgrade_from: Vec<SourceVersion>,
    pub file: String,
    pub bytes: u64,
    pub sha256: String,
    pub source_sha256: String,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceVersion {
    pub version: semver::Version,
    pub source_sha256: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Catalog {
    schema_version: u32,
    channel: String,
    pub sequence: u64,
    pub published_at: chrono::DateTime<chrono::Utc>,
    pub packages: Vec<Release>,
}

pub const OFFICIAL_INDEX: &str = "https://r.bgxiong.com/client/ai-story/bgxiong-content-skills/latest.json";
pub const OFFICIAL_KEY_ID: &str = "content-skill-catalog-20261119";
pub const OFFICIAL_KEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IGNvbnRlbnQtc2tpbGwgcHVibGljIGtleQpSV1RNTnpLUzVWdXNXK2lDSUZoMUJuMFFneE96U0lCeFhWRDlTbnhRNzZaOVVVVHhDUTVhU0FacQo=";

pub fn parse_catalog(envelope: &[u8], key_id: &str, key: &str) -> Result<(Catalog, String), DistributionError> {
    let (bytes, digest) = index::verify_envelope(envelope, &index::IndexTrust { key_id, pubkey_b64: key })?;
    let catalog: Catalog = serde_json::from_slice(&bytes).map_err(|err| error("ONLINE_INDEX_INVALID", err))?;
    let mut ids = BTreeSet::new();
    if catalog.schema_version != 1 || catalog.channel != "content-skill" || catalog.sequence == 0
        || catalog.packages.iter().any(|release| {
            let valid_id = bgx_agent_skills_core::identity::StableId::parse(&format!("builtin:{}", release.package_id)).is_ok();
            !valid_id || !ids.insert(&release.package_id)
                || release.display_name.trim().is_empty()
                || release.file != format!("{}-{}.zip", release.package_id, release.version)
                || release.bytes == 0 || release.bytes > PACKAGE_MAX_BYTES
                || !is_sha256(&release.sha256) || !is_sha256(&release.source_sha256)
                || !release.version.pre.is_empty() || !release.version.build.is_empty()
                || release.upgrade_from.iter().any(|old| !is_sha256(&old.source_sha256) || old.version >= release.version)
        })
    {
        return Err(error("ONLINE_INDEX_INVALID", "Invalid Skill catalog"));
    }
    Ok((catalog, digest))
}

impl Release {
    pub fn candidate_id(&self) -> String {
        format!("content-skill:{}@{}#{}", self.package_id, self.version, self.sha256)
    }

    pub fn allows_upgrade_from(&self, baseline: &str) -> bool {
        self.upgrade_from.iter().any(|old| {
            baseline.strip_prefix("sha256:") == Some(old.source_sha256.as_str())
                && old.version < self.version
        })
    }

    pub fn check_host(&self, host: &semver::Version) -> Result<(), DistributionError> {
        if self.host_version_range.matches(host) {
            return Ok(());
        }
        Err(error("ONLINE_SKILL_HOST_INCOMPATIBLE", &self.host_version_range))
    }
}
