//! P05: stableId and same-name policy.

use bgx_agent_skills_core::digest::package_digest;
use bgx_agent_skills_core::identity::{
    catalog_visible, check_expected_digest, confirm_replace, plan_install, InstallDecision,
    InstalledRef, StableId, StableIdError,
};
use bgx_agent_skills_core::validation::check_skill_md;
use bgx_agent_skills_core::{DiagnosticCode as D, Source};

const FIXTURES_VALID_LENIENT: &str = include_str!(
    "../../../../tests/fixtures/agent-skills/app-contract/valid/activate-input-lenient-name.json"
);
const FIXTURES_VALID_UNICODE: &str = include_str!(
    "../../../../tests/fixtures/agent-skills/app-contract/valid/activate-input-unicode-name.json"
);
const FIXTURES_VALID_PLAIN: &str =
    include_str!("../../../../tests/fixtures/agent-skills/app-contract/valid/activate-input.json");
const FIXTURES_INVALID: [&str; 4] = [
    include_str!("../../../../tests/fixtures/agent-skills/app-contract/invalid/activate-input-dotdot-name.json"),
    include_str!("../../../../tests/fixtures/agent-skills/app-contract/invalid/activate-input-path-as-id.json"),
    include_str!("../../../../tests/fixtures/agent-skills/app-contract/invalid/activate-input-project-source.json"),
    include_str!("../../../../tests/fixtures/agent-skills/app-contract/invalid/activate-input-raw-name.json"),
];

fn skill_id_of(fixture: &str) -> String {
    let v: serde_json::Value = serde_json::from_str(fixture).unwrap();
    v["instance"]["skillId"].as_str().unwrap().to_string()
}

fn digest(md: &str) -> String {
    package_digest([("SKILL.md", md.as_bytes())])
}

fn md(name: &str, desc: &str, extra: &str) -> String {
    format!("---\nname: {name}\ndescription: {desc}\n{extra}---\nbody\n")
}

#[test]
fn stable_id_round_trip_and_contract_fixture_parity() {
    for fx in [
        FIXTURES_VALID_LENIENT,
        FIXTURES_VALID_UNICODE,
        FIXTURES_VALID_PLAIN,
    ] {
        let s = skill_id_of(fx);
        let id = StableId::parse(&s).unwrap_or_else(|e| panic!("{s}: {e:?}"));
        assert_eq!(id.to_string(), s);
    }
    for fx in FIXTURES_INVALID {
        let s = skill_id_of(fx);
        assert!(
            StableId::parse(&s).is_err(),
            "{s} must be rejected like the contract schema"
        );
    }
    assert_eq!(
        StableId::parse("project:pdf"),
        Err(StableIdError::UnknownSource)
    );
    assert_eq!(StableId::parse("pdf"), Err(StableIdError::MissingSeparator));
    assert_eq!(StableId::parse("user:a:b").unwrap().name(), "a:b");
    assert_eq!(
        StableId::parse(&format!("user:{}", "a".repeat(129))),
        Err(StableIdError::NameTooLong)
    );
}

#[test]
fn same_name_different_content_requires_explicit_confirmation() {
    let v1 = digest(&md("pdf-tools", "v1", ""));
    let v2 = digest(&md("pdf-tools", "v2", ""));
    let installed = vec![InstalledRef {
        stable_id: StableId::new(Source::User, "pdf-tools").unwrap(),
        package_digest: v1.clone(),
    }];
    let plan = plan_install(&installed, Source::User, "pdf-tools", &v2).unwrap();
    assert_eq!(
        plan.decision,
        InstallDecision::ReplaceRequiresConfirmation {
            existing_digest: v1
        }
    );
    let err = confirm_replace(&plan, false).unwrap_err();
    assert_eq!(err.code, D::InstallConflict);
    assert!(confirm_replace(&plan, true).is_ok());
    assert_eq!(
        plan.stable_id.to_string(),
        "user:pdf-tools",
        "never auto-renamed"
    );
}

#[test]
fn identical_digest_is_a_no_op() {
    let d = digest(&md("pdf-tools", "same", ""));
    let installed = vec![InstalledRef {
        stable_id: StableId::new(Source::User, "pdf-tools").unwrap(),
        package_digest: d.clone(),
    }];
    let plan = plan_install(&installed, Source::User, "pdf-tools", &d).unwrap();
    assert_eq!(plan.decision, InstallDecision::Identical);
    assert!(confirm_replace(&plan, false).is_ok());
}

#[test]
fn missing_metadata_version_is_fine_and_upgrade_keeps_the_id() {
    let old = md("pdf-tools", "Extract PDF text.", "");
    let new = md(
        "pdf-tools",
        "Extract PDF text and tables.",
        "metadata:\n  version: \"2.0\"\n",
    );
    let c_old = check_skill_md(old.as_bytes(), Some("pdf-tools"));
    assert!(c_old.findings.is_empty(), "no version metadata needed");
    let c_new = check_skill_md(new.as_bytes(), Some("pdf-tools"));
    assert!(c_new.findings.is_empty());
    let installed = vec![InstalledRef {
        stable_id: StableId::new(Source::User, c_old.name.as_deref().unwrap()).unwrap(),
        package_digest: digest(&old),
    }];
    let plan = plan_install(
        &installed,
        Source::User,
        c_new.name.as_deref().unwrap(),
        &digest(&new),
    )
    .unwrap();
    assert_eq!(
        plan.stable_id, installed[0].stable_id,
        "upgrade keeps the stableId"
    );
    assert!(matches!(
        plan.decision,
        InstallDecision::ReplaceRequiresConfirmation { .. }
    ));
}

#[test]
fn renamed_package_or_moved_directory_never_points_at_another_package() {
    let a = md("pdf-tools", "A", "");
    let installed = vec![
        InstalledRef {
            stable_id: StableId::new(Source::User, "pdf-tools").unwrap(),
            package_digest: digest(&a),
        },
        InstalledRef {
            stable_id: StableId::new(Source::User, "doc-tools").unwrap(),
            package_digest: digest(&md("doc-tools", "B", "")),
        },
    ];
    // Same content moved to another directory: the identity still comes from `name`.
    let moved = check_skill_md(a.as_bytes(), Some("some-other-dir"));
    assert_eq!(moved.name.as_deref(), Some("pdf-tools"));
    let plan = plan_install(
        &installed,
        Source::User,
        moved.name.as_deref().unwrap(),
        &digest(&a),
    )
    .unwrap();
    assert_eq!(plan.decision, InstallDecision::Identical);
    // Renamed in frontmatter: a new identity, never the other installed package.
    let renamed = md("pdf-tools-2", "A", "");
    let plan = plan_install(&installed, Source::User, "pdf-tools-2", &digest(&renamed)).unwrap();
    assert_eq!(plan.decision, InstallDecision::New);
    assert_eq!(plan.stable_id.to_string(), "user:pdf-tools-2");
}

#[test]
fn identity_is_never_guessed_from_the_description() {
    let installed = vec![InstalledRef {
        stable_id: StableId::new(Source::User, "pdf-tools").unwrap(),
        package_digest: digest(&md("pdf-tools", "Extract PDF text.", "")),
    }];
    // Same description, different name: unrelated new package.
    let other = md("pdf-reader", "Extract PDF text.", "");
    let plan = plan_install(&installed, Source::User, "pdf-reader", &digest(&other)).unwrap();
    assert_eq!(plan.decision, InstallDecision::New);
}

#[test]
fn user_shadows_builtin_and_both_stay_installed() {
    let b = InstalledRef {
        stable_id: StableId::new(Source::Builtin, "screenwriter").unwrap(),
        package_digest: digest(&md("screenwriter", "b", "")),
    };
    let u_digest = digest(&md("screenwriter", "u", ""));
    let plan = plan_install(
        std::slice::from_ref(&b),
        Source::User,
        "screenwriter",
        &u_digest,
    )
    .unwrap();
    assert_eq!(
        plan.decision,
        InstallDecision::New,
        "different source: both installed"
    );
    assert_eq!(plan.shadows, Some(b.stable_id.clone()));
    let u = InstalledRef {
        stable_id: plan.stable_id.clone(),
        package_digest: u_digest,
    };
    let all = vec![b.clone(), u.clone()];
    let visible: Vec<String> = catalog_visible(&all)
        .iter()
        .map(|i| i.stable_id.to_string())
        .collect();
    assert_eq!(visible, vec!["user:screenwriter"]);
    let plan_b = plan_install(
        std::slice::from_ref(&u),
        Source::Builtin,
        "screenwriter",
        &b.package_digest,
    )
    .unwrap();
    assert_eq!(plan_b.shadowed_by, Some(u.stable_id));
}

#[test]
fn cas_check_for_edits() {
    let d1 = digest(&md("x", "1", ""));
    let d2 = digest(&md("x", "2", ""));
    let i = InstalledRef {
        stable_id: StableId::new(Source::User, "x").unwrap(),
        package_digest: d1.clone(),
    };
    assert!(check_expected_digest(&i, &d1).is_ok());
    assert_eq!(
        check_expected_digest(&i, &d2).unwrap_err().code,
        D::DigestMismatch
    );
}
