use bgx_agent_skills_core::{
    SKILL_MD, Profile, FormatStatus, digest::package_digest_of_entries,
    frontmatter::parse_skill_md, validation::check_skill_md,
};

use crate::{DistributionError, archive::{ReadPackage, read_zip_bytes}, catalog::Release, error, sha256};

pub fn check_identity(
    release: &Release,
    name: &str,
    digest: &str,
    version: Option<&str>,
) -> Result<(), DistributionError> {
    if name != release.package_id
        || digest.strip_prefix("sha256:") != Some(release.source_sha256.as_str())
        || version != Some(release.version.to_string().as_str())
    {
        return Err(error("ONLINE_PACKAGE_IDENTITY_MISMATCH", "Skill package differs from signed candidate"));
    }
    Ok(())
}

pub fn verify_zip(release: &Release, bytes: &[u8]) -> Result<ReadPackage, DistributionError> {
    if bytes.len() as u64 != release.bytes || sha256(bytes) != release.sha256 {
        return Err(error("ONLINE_PACKAGE_IDENTITY_MISMATCH", "ZIP bytes differ from signed candidate"));
    }
    let read = read_zip_bytes(&release.file, bytes).map_err(|err| error("ONLINE_SKILL_PACKAGE_INVALID", err))?;
    let body = read.get(SKILL_MD).ok_or_else(|| error("ONLINE_SKILL_PACKAGE_INVALID", "Missing SKILL.md"))?;
    let checked = check_skill_md(body, Some(&read.container_name));
    if !matches!(checked.format_status(Profile::Import), FormatStatus::Valid | FormatStatus::ValidWithWarnings) {
        return Err(error("ONLINE_SKILL_PACKAGE_INVALID", "Invalid SKILL.md"));
    }
    let parsed = parse_skill_md(body).parsed
        .ok_or_else(|| error("ONLINE_SKILL_PACKAGE_INVALID", "Invalid frontmatter"))?;
    let value = |key: &str| parsed.frontmatter.metadata_pairs.iter()
        .find(|(name, _)| name == key).map(|(_, value)| value.trim());
    let digest = package_digest_of_entries(&read.manifest.entries);
    check_identity(release, checked.name.as_deref().unwrap_or_default(), &digest, value("version"))?;
    let contract: serde_json::Value = serde_json::from_str(include_str!("../../../../packages/app-contracts/v1/agent-skills.json"))
        .expect("agent-skills contract is valid");
    let workflow = &contract["skillMetadataExtensions"]["workflow"];
    let key = workflow["key"].as_str()
        .ok_or_else(|| error("ONLINE_SKILL_PACKAGE_INVALID", "Missing workflow policy"))?;
    if value(key) != workflow["currentValue"].as_str() {
        return Err(error("ONLINE_SKILL_NEEDS_REVIEW", "Skill workflow is incompatible"));
    }
    Ok(read)
}
