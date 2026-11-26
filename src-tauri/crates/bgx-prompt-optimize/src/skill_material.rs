use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::{error_codes, OptimizeError};

/// Frozen user Skill or builtin method bytes. Paths in `resources` must stay inside the package.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FrozenSkillBytes {
    pub stable_id: String,
    pub package_digest: String,
    pub body: String,
    pub resources: BTreeMap<String, Vec<u8>>,
}

/// Host may send a bare 64-hex digest or the contract `sha256:` form.
pub fn normalize_package_digest(raw: &str) -> Result<String, OptimizeError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(OptimizeError::new(
            error_codes::AGENT_SKILL_DIGEST_REQUIRED,
            "AGENT_SKILL_DIGEST_REQUIRED",
        ));
    }
    if bgx_agent_skills_core::digest::is_package_digest(trimmed) {
        return Ok(trimmed.to_owned());
    }
    if trimmed.len() == 64
        && trimmed
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Ok(format!("sha256:{trimmed}"));
    }
    Err(OptimizeError::new(
        error_codes::AGENT_SKILL_DIGEST_MISMATCH,
        "AGENT_SKILL_DIGEST_MISMATCH: packageDigest is not a sha256 digest",
    ))
}

pub fn validate_resource_paths(skill: &FrozenSkillBytes) -> Result<(), OptimizeError> {
    for path in skill.resources.keys() {
        bgx_agent_skills_core::resource_path::parse_resource_ref(path).map_err(|finding| {
            OptimizeError::new(
                error_codes::INTERNAL_PROMPT_CONFIG,
                format!(
                    "INTERNAL_PROMPT_CONFIG: path-rejected: {path} ({})",
                    finding
                        .details
                        .get("reason")
                        .and_then(|v| v.as_str())
                        .unwrap_or("invalid")
                ),
            )
        })?;
    }
    Ok(())
}

/// Verify `packageDigest` against body + resources.
/// Convention: `resources` is the complete package; if `SKILL.md` is absent, `body` is used as that file.
pub fn verify_method_digest(skill: &FrozenSkillBytes) -> Result<String, OptimizeError> {
    validate_resource_paths(skill)?;
    let expected = normalize_package_digest(&skill.package_digest)?;
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    if !skill.resources.contains_key("SKILL.md") {
        files.push(("SKILL.md".into(), skill.body.as_bytes().to_vec()));
    }
    for (path, bytes) in &skill.resources {
        if path == "SKILL.md" && bytes.as_slice() != skill.body.as_bytes() {
            return Err(OptimizeError::new(
                error_codes::AGENT_SKILL_DIGEST_MISMATCH,
                "AGENT_SKILL_DIGEST_MISMATCH: SKILL.md does not match body",
            ));
        }
        files.push((path.clone(), bytes.clone()));
    }
    let actual = bgx_agent_skills_core::digest::package_digest(
        files.iter().map(|(path, bytes)| (path.as_str(), bytes.as_slice())),
    );
    if actual != expected {
        return Err(OptimizeError::new(
            error_codes::AGENT_SKILL_DIGEST_MISMATCH,
            "AGENT_SKILL_DIGEST_MISMATCH: packageDigest does not match body and resources",
        ));
    }
    Ok(actual)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_paths_cannot_escape_the_package() {
        let skill = FrozenSkillBytes {
            stable_id: "s".into(),
            package_digest: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            body: "# x".into(),
            resources: BTreeMap::from([("../outside.md".into(), b"x".to_vec())]),
        };
        let err = validate_resource_paths(&skill).unwrap_err();
        assert_eq!(err.code, error_codes::INTERNAL_PROMPT_CONFIG);
        let absolute = FrozenSkillBytes {
            resources: BTreeMap::from([("/tmp/x.md".into(), b"x".to_vec())]),
            ..skill.clone()
        };
        assert!(validate_resource_paths(&absolute).is_err());
    }

    #[test]
    fn digest_form_and_body_mismatch() {
        let err = normalize_package_digest("").unwrap_err();
        assert_eq!(err.code, error_codes::AGENT_SKILL_DIGEST_REQUIRED);
        let err = normalize_package_digest("not-a-digest").unwrap_err();
        assert_eq!(err.code, error_codes::AGENT_SKILL_DIGEST_MISMATCH);
        let skill = FrozenSkillBytes {
            stable_id: "s".into(),
            package_digest: "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            body: "---\nname: s\ndescription: d\n---\nbody\n".into(),
            resources: BTreeMap::new(),
        };
        let err = verify_method_digest(&skill).unwrap_err();
        assert_eq!(err.code, error_codes::AGENT_SKILL_DIGEST_MISMATCH);
        let digest = bgx_agent_skills_core::digest::package_digest([(
            "SKILL.md",
            skill.body.as_bytes(),
        )]);
        let ok = FrozenSkillBytes {
            package_digest: digest.clone(),
            ..skill
        };
        assert_eq!(verify_method_digest(&ok).unwrap(), digest);
    }

    #[test]
    fn frontmatter_skill_md_as_body_without_resource_copy_verifies() {
        let skill_md = "---\nname: s\ndescription: d\n---\nWrite a complete image prompt.\n";
        let notes = b"# Notes\n";
        let digest = bgx_agent_skills_core::digest::package_digest([
            ("SKILL.md", skill_md.as_bytes()),
            ("references/notes.md", notes.as_slice()),
        ]);
        let skill = FrozenSkillBytes {
            stable_id: "s".into(),
            package_digest: digest.clone(),
            body: skill_md.into(),
            resources: BTreeMap::from([("references/notes.md".into(), notes.to_vec())]),
        };
        assert_eq!(verify_method_digest(&skill).unwrap(), digest);
        let stripped_body = FrozenSkillBytes {
            body: "Write a complete image prompt.".into(),
            resources: BTreeMap::from([("SKILL.md".into(), skill_md.as_bytes().to_vec())]),
            ..skill.clone()
        };
        let err = verify_method_digest(&stripped_body).unwrap_err();
        assert_eq!(err.code, error_codes::AGENT_SKILL_DIGEST_MISMATCH);
        assert!(err.message.contains("SKILL.md does not match body"), "{err:?}");
    }
}