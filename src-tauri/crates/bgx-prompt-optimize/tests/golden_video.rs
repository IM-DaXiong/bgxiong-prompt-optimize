//! T34: one offline golden per production projector (registry size = 8).
//!
//! Expected strings are derived from each projector's compile rules on the shared
//! fixture (mother shot order, expression lines within a shot, leftover quotes
//! never dropped). They are not first-compile copies and not non-empty checks.
//! LTX expected text is the existing projector test assertion.

use bgx_prompt_optimize::projectors::compiler_registry;
use bgx_prompt_optimize::projectors::port::{
    ProjectionCompileInput, ProjectionSpanOrigin, ReviewedMother,
};
use bgx_prompt_optimize::projectors::production_registry;
use bgx_prompt_optimize::projectors::ltx::LtxProjector;
use bgx_prompt_optimize::projectors::port::VideoPromptProjectionPort;
use bgx_video_prompt_contract::document::DocumentDraftV1;
use bgx_video_prompt_contract::projection::{
    VideoPromptProjectionDraft, VideoPromptTargetSnapshot,
};
use serde_json::json;

/// Neutral / Wan / Kling text_to_video syntax for this fixture: `Shot N` headers,
/// a blank line between shots, and English ASCII quotes with one leading space
/// after the expression line. No reference tags are written because the target
/// has an empty `referenceUseIds` list. The three Kling dialects only diverge
/// when tagged references are present, so they share this text here.
const SHOT_BLOCK_ASCII_QUOTES: &str = "Shot 1\n她轻声说。 \"你好，世界！\"\n\nShot 2\nThen she turns.";

/// Seedance spoken-role marks (`dialogue`) are `{{原文}}`, with one leading space
/// when the quote follows text on the same row. Headers stay `Shot N`.
const SEEDANCE_GOLDEN: &str = "Shot 1\n她轻声说。 {你好，世界！}\n\nShot 2\nThen she turns.";

/// H3 T2VA three-field layout from `minimax-h3-cir-field-keys.json`. Both shots
/// write the picture field; dialogue with Chinese exact text uses
/// `(S1) <d>[Chinese] …</d>` and empty audio/music take the official
/// unspecified / N/A placeholders. There is no alignment header in T2VA.
const H3_GOLDEN: &str = "integrated_multimodal_description: [Shot 1] 她轻声说。 (S1) <d>[Chinese] 你好，世界！</d> [Shot 2] Then she turns.\n\noverall_soundscape: No additional sound source is specified. This placeholder does not mean the scene is silent.\n\nnon_diegetic_music: N/A";

fn mother() -> DocumentDraftV1 {
    serde_json::from_value(json!({"shots":[
        {"shotId":"s1","facts":[{"factId":"f1","kind":"action","text":"She speaks.","semanticSlot":"action_interaction","polarity":"positive","provenance":"user_text","sourceFactId":null,"entityRefIds":[],"sourceUnitIds":["unit-f1"]}],"quotes":[{"quoteId":"q1","exactText":"你好，世界！","role":"dialogue","speakerId":null,"sourceUnitId":null}]},
        {"shotId":"s2","facts":[{"factId":"f2","kind":"action","text":"She turns.","semanticSlot":"action_interaction","polarity":"positive","provenance":"user_text","sourceFactId":null,"entityRefIds":[],"sourceUnitIds":["unit-f2"]}],"quotes":[]}
    ]}))
    .unwrap()
}

fn expression() -> VideoPromptProjectionDraft {
    serde_json::from_value(json!({"lines":[
        {"shotId":"s2","factIds":["f2"],"quoteIds":[],"referenceUseIds":[],"text":"Then she turns."},
        {"shotId":"s1","factIds":["f1"],"quoteIds":["q1"],"referenceUseIds":[],"text":"她轻声说。"}
    ],"conflicts":[]}))
    .unwrap()
}

fn target(profile_id: &str, profile_version: u64) -> VideoPromptTargetSnapshot {
    serde_json::from_value(json!({
        "profileId": profile_id,
        "profileVersion": profile_version,
        "inputMode": "text_to_video",
        "durationSeconds": 5.0,
        "capsDigest": "a".repeat(64),
        "expressionLanguage": "en",
        "referenceUseIds": []
    }))
    .unwrap()
}

fn compile(profile_id: &str, profile_version: u64) -> String {
    let document = mother();
    let expression = expression();
    let target = target(profile_id, profile_version);
    let registry = compiler_registry().expect("compiler registry");
    let projector = registry
        .projector(profile_id, profile_version)
        .unwrap_or_else(|| panic!("missing compiler {profile_id}@{profile_version}"));
    let candidate = projector
        .compile(&ProjectionCompileInput {
            mother: ReviewedMother {
                document: &document,
                canonical_fingerprint: "b",
            },
            references: &[],
            identity_locks: &[],
            target: &target,
            method: None,
            expression: Some(&expression),
        })
        .unwrap_or_else(|error| panic!("{profile_id}@{profile_version}: {error}"));
    assert_eq!(candidate.projector.profile_id, profile_id);
    assert_eq!(candidate.projector.profile_version, profile_version);
    candidate.text
}

fn assert_golden(profile_id: &str, profile_version: u64, expected: &str) {
    assert_eq!(
        compile(profile_id, profile_version),
        expected,
        "{profile_id}@{profile_version} golden"
    );
}

#[test]
fn golden_registry_has_eight_projectors_and_no_happyhorse() {
    let catalog = production_registry().unwrap();
    let keys: Vec<String> = catalog.identities().iter().map(|i| i.key()).collect();
    assert_eq!(
        keys,
        [
            "h3@1",
            "seedance@1",
            "kling.classic@1",
            "kling.omni@1",
            "kling.omni@2",
            "wan@1",
            "ltx@1",
            "generic@1",
        ]
    );
    assert!(catalog.identity_of("happyhorse", 1).is_none());
    assert!(catalog.identity_of("ltx", 1).unwrap().default_method_name.is_none());
    assert!(!catalog.supports_input_mode("kling.omni", 2, "reference_to_video"));
    let compilers = compiler_registry().unwrap();
    assert_eq!(compilers.identities().len(), 8);
}

#[test]
fn golden_h3() {
    assert_golden("h3", 1, H3_GOLDEN);
}

#[test]
fn golden_seedance() {
    assert_golden("seedance", 1, SEEDANCE_GOLDEN);
}

#[test]
fn golden_kling_classic() {
    assert_golden("kling.classic", 1, SHOT_BLOCK_ASCII_QUOTES);
}

#[test]
fn golden_kling_omni_legacy() {
    assert_golden("kling.omni", 1, SHOT_BLOCK_ASCII_QUOTES);
}

#[test]
fn golden_kling_omni_nextgen() {
    assert_golden("kling.omni", 2, SHOT_BLOCK_ASCII_QUOTES);
}

#[test]
fn golden_wan() {
    assert_golden("wan", 1, SHOT_BLOCK_ASCII_QUOTES);
}

#[test]
fn golden_ltx_matches_existing_projector_assertion() {
    let text = compile("ltx", 1);
    assert_eq!(text, "她轻声说。 \"你好，世界！\" Then she turns.");
    let document = mother();
    let expression = expression();
    let target = target("ltx", 1);
    let candidate = LtxProjector
        .compile(&ProjectionCompileInput {
            mother: ReviewedMother {
                document: &document,
                canonical_fingerprint: "b",
            },
            references: &[],
            identity_locks: &[],
            target: &target,
            method: None,
            expression: Some(&expression),
        })
        .unwrap();
    let quote = candidate
        .source_map
        .iter()
        .find(|span| matches!(span.origin, ProjectionSpanOrigin::Quote { .. }))
        .unwrap();
    assert_eq!(&candidate.text[quote.start..quote.end], "你好，世界！");
}

#[test]
fn golden_generic() {
    assert_golden("generic", 1, SHOT_BLOCK_ASCII_QUOTES);
}
