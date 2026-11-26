//! R14a/R14b: video-prompt projection + identity-lock contract generation.
use std::{env, path::Path, path::PathBuf};

use bgx_contract_gen::*;

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let repo = manifest.join("../../..");
    let v1 = repo.join("packages/app-contracts/v1");
    let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));

    let mut docs = std::collections::BTreeMap::new();
    for name in [
        "agent-skills.json",
        "agent-skill-library.schema.json",
        "video-prompt-projection.schema.json",
        "video-prompt-projection-draft.schema.json",
        "video-prompt-channels.schema.json",
    ] {
        docs.insert(name.to_string(), read_contract_json(&v1.join(name)));
        println!("cargo:rerun-if-changed={}", v1.join(name).display());
    }
    let projection = schema_types::generate(
        &docs,
        "video-prompt-projection.schema.json",
        &[
            "pipelineMode",
            "projectionMethodSelection",
            "canonicalDocumentRef",
            "spanSourceRef",
            "frozenMethod",
            "targetSnapshot",
            "projectionReview",
            "projectionReceipt",
            "projectionQueryInput",
            "projectionQueryResult",
            "negativeInput",
            "negativeOperation",
            "expansionMode",
            "projectionDraft",
            "projectionReviewResult",
        ],
        "VideoPrompt",
    )
    .unwrap_or_else(|e| panic!("video-prompt-projection.schema.json types: {e}"));
    write_generated(
        &out.join("app_contracts_video_prompt_projection.rs"),
        &projection.rust,
    );

    let ts = repo.join("ui/src/domain/video-prompt-pipeline/projectionContract.generated.ts");
    println!("cargo:rerun-if-changed={}", ts.display());
    let expected = projection.typescript.replace("\r\n", "\n");
    let current = read_twin_text(&ts);
    if current != expected {
        panic!(
            "ui/src/domain/video-prompt-pipeline/projectionContract.generated.ts is stale against video-prompt-projection(-draft).schema.json; run gen-video-prompt-contracts --write"
        );
    }

    emit_video_prompt_identity_lock(&v1, &out);
}


fn count_placeholder(tpl: &str, needle: &str) -> usize {
    tpl.matches(needle).count()
}

fn require_lock_template(raw: &serde_json::Value, lang: &str) -> String {
    let tpl = raw
        .pointer(&format!("/identityLockTemplates/{lang}"))
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| {
            panic!("video-prompt-semantic-v1.json missing identityLockTemplates.{lang}")
        })
        .to_string();
    let tokens = usize::from(!lang.starts_with("neutral"));
    if count_placeholder(&tpl, "{name}") != 1 || count_placeholder(&tpl, "{token}") != tokens {
        panic!(
            "identityLockTemplates.{lang} must contain {{name}} once and {{token}} {tokens} time(s)"
        );
    }
    let mut rest = tpl.clone();
    rest = rest.replace("{name}", "");
    rest = rest.replace("{token}", "");
    if rest.contains('{') || rest.contains('}') {
        panic!("identityLockTemplates.{lang} has an unknown placeholder");
    }
    tpl
}

fn require_version(raw: &serde_json::Value, key: &str) -> u64 {
    raw.get(key)
        .and_then(|v| v.as_u64())
        .unwrap_or_else(|| panic!("video-prompt-semantic-v1.json missing {key}"))
}

fn emit_video_prompt_identity_lock(v1: &Path, out: &Path) {
    let file = "video-prompt-semantic-v1.json";
    let raw = read_contract_json(&v1.join(file));
    println!("cargo:rerun-if-changed={}", v1.join(file).display());
    let schema = require_version(&raw, "schemaVersion");
    let project = require_version(&raw, "projectFormatVersion");
    let projector = require_version(&raw, "projectorVersion");
    let mother = require_version(&raw, "motherPromptVersion");
    let review = require_version(&raw, "reviewPromptVersion");
    if schema != 4 || project != 3 || projector != 7 || mother != 9 || review != 10 {
        panic!("{file} versions must be schema 4, project 3, projector 7, mother 9, review 10");
    }
    let issue_codes: std::collections::BTreeSet<&str> = raw["issueCodes"]
        .as_array()
        .unwrap_or_else(|| panic!("{file} missing issueCodes"))
        .iter()
        .map(|value| {
            value
                .as_str()
                .unwrap_or_else(|| panic!("{file} issueCodes must contain strings"))
        })
        .collect();
    let review_codes = raw["motherReviewIssueCodes"]
        .as_array()
        .filter(|codes| !codes.is_empty())
        .unwrap_or_else(|| panic!("{file} missing nonempty motherReviewIssueCodes"));
    let mut seen_review_codes = std::collections::BTreeSet::new();
    for value in review_codes {
        let code = value
            .as_str()
            .unwrap_or_else(|| panic!("{file} motherReviewIssueCodes must contain strings"));
        if !issue_codes.contains(code) || !seen_review_codes.insert(code) {
            panic!("{file} motherReviewIssueCodes must be a unique subset of issueCodes");
        }
    }
    let kinds = raw
        .get("factKinds")
        .and_then(|v| v.as_array())
        .unwrap_or_else(|| panic!("{file} missing factKinds"));
    if kinds.iter().any(|v| v.as_str() == Some("subject_identity")) {
        panic!("{file} must not list subject_identity");
    }
    let zh = require_lock_template(&raw, "zh");
    let en = require_lock_template(&raw, "en");
    let neutral_zh = require_lock_template(&raw, "neutralZh");
    let neutral_en = require_lock_template(&raw, "neutralEn");
    write_generated(
        &out.join("app_contracts_video_prompt_identity_lock.rs"),
        &format!(
            "pub const SEMANTIC_CONTRACT_VERSION: u32 = {schema};\n\
             pub const PROJECT_FORMAT_VERSION: u32 = {project};\n\
             pub const PROJECTOR_VERSION: u32 = {projector};\n\
             pub const MOTHER_PROMPT_VERSION: u32 = {mother};\n\
             pub const REVIEW_PROMPT_VERSION: u32 = {review};\n\
             pub const IDENTITY_LOCK_TEMPLATE_ZH: &str = {};\n\
             pub const IDENTITY_LOCK_TEMPLATE_EN: &str = {};\n\
             pub const IDENTITY_LOCK_TEMPLATE_NEUTRAL_ZH: &str = {};\n\
             pub const IDENTITY_LOCK_TEMPLATE_NEUTRAL_EN: &str = {};\n",
            rust_string_lit(&zh),
            rust_string_lit(&en),
            rust_string_lit(&neutral_zh),
            rust_string_lit(&neutral_en),
        ),
    );
}
