//! P05: installed-package identity and same-name policy (SPEC section 8.1, K02 `stableId`).
//!
//! `stableId = "<source>:<name>"` with `source` in the contract's closed set (`builtin`, `user`;
//! no project scope). The name is the frontmatter value (lenient import name). Identity never
//! comes from the description, the directory name or `metadata.version`; upgrades keep the
//! stableId and are told apart by `packageDigest`.

use std::fmt;

use crate::digest::is_package_digest;
use crate::{budgets, DiagnosticCode, Finding, Source};

/// Contract `stableId`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct StableId {
    source: Source,
    name: String,
}

/// Why a string is not a valid stableId / skill name value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StableIdError {
    MissingSeparator,
    UnknownSource,
    EmptyName,
    NameTooLong,
    DotName,
    ForbiddenCharacter,
}

/// Contract `skillName`: 1..=nameHardMaxChars chars, not `.`/`..`, no `/`, `\`, C0 controls or DEL.
pub fn check_skill_name_value(name: &str) -> Result<(), StableIdError> {
    if name.is_empty() {
        return Err(StableIdError::EmptyName);
    }
    if name.chars().count() as u64 > budgets::NAME_HARD_MAX_CHARS {
        return Err(StableIdError::NameTooLong);
    }
    if name == "." || name == ".." {
        return Err(StableIdError::DotName);
    }
    if name
        .chars()
        .any(|c| c == '/' || c == '\\' || (c as u32) < 0x20 || c as u32 == 0x7f)
    {
        return Err(StableIdError::ForbiddenCharacter);
    }
    Ok(())
}

impl StableId {
    pub fn new(source: Source, name: &str) -> Result<Self, StableIdError> {
        check_skill_name_value(name)?;
        Ok(StableId {
            source,
            name: name.to_string(),
        })
    }

    /// Parse `<source>:<name>`; the first `:` separates source and name.
    pub fn parse(s: &str) -> Result<Self, StableIdError> {
        let (src, name) = s.split_once(':').ok_or(StableIdError::MissingSeparator)?;
        let source = Source::parse(src).ok_or(StableIdError::UnknownSource)?;
        StableId::new(source, name)
    }

    pub fn source(&self) -> Source {
        self.source
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

impl fmt::Display for StableId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.source.as_str(), self.name)
    }
}

/// An installed package as seen by the identity policy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstalledRef {
    pub stable_id: StableId,
    pub package_digest: String,
}

/// What installing a candidate would mean.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstallDecision {
    /// No package with this stableId: install as new.
    New,
    /// Same stableId and same packageDigest: nothing to do.
    Identical,
    /// Same stableId, different content: replacing needs explicit user confirmation
    /// (never silently overwritten, never auto-renamed, never installed side by side).
    ReplaceRequiresConfirmation { existing_digest: String },
}

/// Install plan for one candidate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallPlan {
    pub stable_id: StableId,
    pub decision: InstallDecision,
    /// `builtin:<name>` hidden from the model catalog by this `user` package, if any.
    pub shadows: Option<StableId>,
    /// `user:<name>` that hides this `builtin` package from the model catalog, if any.
    pub shadowed_by: Option<StableId>,
}

/// Decide how a candidate `(source, name, packageDigest)` relates to installed packages.
/// `name` must be the frontmatter name and `package_digest` a value from [`crate::digest`];
/// nothing else (description, directory, metadata.version) is consulted.
pub fn plan_install(
    installed: &[InstalledRef],
    source: Source,
    name: &str,
    package_digest: &str,
) -> Result<InstallPlan, Finding> {
    let stable_id = StableId::new(source, name).map_err(|e| {
        Finding::new(DiagnosticCode::NameUnsafe)
            .with_field("name")
            .with_detail("reason", format!("{e:?}"))
    })?;
    debug_assert!(
        is_package_digest(package_digest),
        "plan_install expects a digest computed by crate::digest"
    );
    let decision = match installed.iter().find(|i| i.stable_id == stable_id) {
        None => InstallDecision::New,
        Some(i) if i.package_digest == package_digest => InstallDecision::Identical,
        Some(i) => InstallDecision::ReplaceRequiresConfirmation {
            existing_digest: i.package_digest.clone(),
        },
    };
    let other = |s: Source| {
        installed
            .iter()
            .find(|i| i.stable_id.source == s && i.stable_id.name == name)
            .map(|i| i.stable_id.clone())
    };
    let (shadows, shadowed_by) = match source {
        Source::User => (other(Source::Builtin), None),
        Source::Builtin => (None, other(Source::User)),
    };
    Ok(InstallPlan {
        stable_id,
        decision,
        shadows,
        shadowed_by,
    })
}

/// Apply the user's answer to a plan. Replacement without confirmation is
/// `AGENT_SKILL_INSTALL_CONFLICT`; the existing package stays untouched.
pub fn confirm_replace(plan: &InstallPlan, user_confirmed: bool) -> Result<(), Finding> {
    match &plan.decision {
        InstallDecision::ReplaceRequiresConfirmation { existing_digest } if !user_confirmed => {
            Err(Finding::new(DiagnosticCode::InstallConflict)
                .with_detail("stableId", plan.stable_id.to_string())
                .with_detail("existingDigest", existing_digest.as_str()))
        }
        _ => Ok(()),
    }
}

/// Packages visible in the model catalog: a `user` package hides the `builtin` one with the same
/// name (more specific scope wins). Explicit selection by stableId can still pick either.
pub fn catalog_visible(installed: &[InstalledRef]) -> Vec<&InstalledRef> {
    installed
        .iter()
        .filter(|i| {
            !(i.stable_id.source == Source::Builtin
                && installed.iter().any(|u| {
                    u.stable_id.source == Source::User && u.stable_id.name == i.stable_id.name
                }))
        })
        .collect()
}

/// Compare-and-swap check for edits/upgrades: the caller's expected digest must equal the
/// installed one (`AGENT_SKILL_DIGEST_MISMATCH` otherwise).
pub fn check_expected_digest(installed: &InstalledRef, expected: &str) -> Result<(), Finding> {
    if installed.package_digest == expected {
        Ok(())
    } else {
        Err(Finding::new(DiagnosticCode::DigestMismatch)
            .with_detail("stableId", installed.stable_id.to_string())
            .with_detail("expectedDigest", expected)
            .with_detail("actualDigest", installed.package_digest.as_str()))
    }
}
