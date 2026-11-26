//! T30: public `run_video_optimize` stage labels and expression-schema handoff.
//! Compress-only coverage stays in the existing lib tests and is cited, not copied.

mod common;

use std::collections::BTreeMap;

use bgx_prompt_optimize::error::error_codes;
use bgx_prompt_optimize::projectors::compiler_registry;
use bgx_prompt_optimize::projectors::port::{ProjectionCompileInput, ReviewedMother};
use bgx_prompt_optimize::video_run::{
    run_video_optimize, MotherAuthorBind, VideoOptimizeRequest, STAGE_CONTENT, STAGE_EXPRESSION,
};
use bgx_prompt_optimize::{draft_schema_for, FrozenSkillBytes};
use bgx_video_prompt_contract::ProjectionClosedSet;
use common::{
    content_template, expected_expression_schema, expression_request, expression_template,
    format_method, frozen_method, mother, ok_reply, target_value, RecordingCompleter, MOTHER_AUTHOR_REPLY,
    VALID_EXPRESSION_REPLY,
};

#[test]
fn run_video_optimize_runs_content_then_expression_with_closed_set_schema() {
    let mut templates: BTreeMap<String, FrozenSkillBytes> = BTreeMap::new();
    templates.insert(STAGE_CONTENT.to_owned(), content_template());
    templates.insert(STAGE_EXPRESSION.to_owned(), expression_template());

    let request = VideoOptimizeRequest {
        source_text: "She walks.".into(),
        change_request: None,
        target_snapshot: target_value("generic", 1, "text_to_video"),
        format_method: format_method(),
        content_method: None,
        templates,
        expansion_policy: None,
        negative_context: None,
        mother_author: Some(MotherAuthorBind {
            task_context: serde_json::json!({"sourceText": "She walks."}),
            expected_shot_ids: vec!["s1".into()],
            source_unit_ids: vec!["u1".into()],
            quote_candidates: vec![],
            source_scope: None,
            source_mode: "optimize_existing".into(),
            subject_ids: vec![],
        }),
        mother_review_base: None,
        method: frozen_method("generic", 1),
        identity_locks: Vec::new(),
        allow_inferred: false,
        existing_mother: None,
        reference_inventory: Vec::new(),
    };
    let mut completer = RecordingCompleter::new(vec![
        ok_reply(MOTHER_AUTHOR_REPLY, "stop"),
        ok_reply(VALID_EXPRESSION_REPLY, "stop"),
    ]);
    let result = run_video_optimize(request, &mut completer).expect("scripted mother and draft");

    assert_eq!(
        completer
            .calls
            .iter()
            .map(|call| call.step_label.as_str())
            .collect::<Vec<_>>(),
        vec![STAGE_CONTENT, STAGE_EXPRESSION]
    );
    let content_schema = completer.calls[0]
        .schema
        .as_ref()
        .expect("content stage must send the official mother schema");
    assert_eq!(
        content_schema["properties"]["shots"]["items"]["properties"]["shotId"]["enum"],
        serde_json::json!(["s1"])
    );
    let expression_call = &completer.calls[1];
    let expected = expected_expression_schema(&expression_request("generic", 1, "text_to_video"));
    assert_eq!(expression_call.schema.as_ref(), Some(&expected));
    // Cross-check against the mother actually authored by the content stage.
    let authored = mother();
    let expr_request = expression_request("generic", 1, "text_to_video");
    let target = bgx_prompt_optimize::video_job::target_from_request(&expr_request).unwrap();
    let closed = ProjectionClosedSet::new(&authored, &target);
    assert_eq!(
        expression_call.schema.as_ref(),
        Some(&draft_schema_for(&closed))
    );

    let draft = result
        .draft
        .expect("optimize success must retain the draft");
    let prompt = result.final_prompt.expect("optimize success must compile");
    assert!(result.rejected.is_none());
    let compiled = compiler_registry()
        .unwrap()
        .projector("generic", 1)
        .unwrap()
        .compile(&ProjectionCompileInput {
            mother: ReviewedMother {
                document: &authored,
                canonical_fingerprint: "unused-for-generic-text",
            },
            references: &[],
            identity_locks: &[],
            target: &target,
            method: Some(&frozen_method("generic", 1)),
            expression: Some(&draft),
        })
        .unwrap();
    assert_eq!(prompt, compiled.text);
    assert_eq!(result.source_map, compiled.source_map);
    assert_eq!(result.diagnostics, compiled.diagnostics);
    assert!(result.mother_review_raw.is_none());
    assert!(result.expression_review_raw.is_none());
}

#[test]
fn run_video_optimize_refuses_unregistered_projector_before_any_complete() {
    let mut templates = BTreeMap::new();
    templates.insert(STAGE_CONTENT.to_owned(), content_template());
    templates.insert(STAGE_EXPRESSION.to_owned(), expression_template());
    let request = VideoOptimizeRequest {
        source_text: "She walks.".into(),
        change_request: None,
        target_snapshot: target_value("HappyHorse", 1, "text_to_video"),
        format_method: format_method(),
        content_method: None,
        templates,
        expansion_policy: None,
        negative_context: None,
        mother_author: Some(MotherAuthorBind {
            task_context: serde_json::json!({"sourceText": "She walks."}),
            expected_shot_ids: vec!["s1".into()],
            source_unit_ids: vec!["u1".into()],
            quote_candidates: vec![],
            source_scope: None,
            source_mode: "optimize_existing".into(),
            subject_ids: vec![],
        }),
        mother_review_base: None,
        method: frozen_method("generic", 1),
        identity_locks: Vec::new(),
        allow_inferred: false,
        existing_mother: None,
        reference_inventory: Vec::new(),
    };
    let mut completer = RecordingCompleter::new(vec![
        ok_reply(MOTHER_AUTHOR_REPLY, "stop"),
        ok_reply(VALID_EXPRESSION_REPLY, "stop"),
    ]);
    let err = run_video_optimize(request, &mut completer).expect_err("unregistered");
    assert_eq!(err.code, error_codes::VIDEO_PROMPT_PROJECTOR_MISSING);
    assert!(completer.calls.is_empty());
}

fn stage_templates() -> BTreeMap<String, FrozenSkillBytes> {
    let mut templates = BTreeMap::new();
    templates.insert(STAGE_CONTENT.to_owned(), content_template());
    templates.insert(STAGE_EXPRESSION.to_owned(), expression_template());
    templates.insert(
        bgx_prompt_optimize::video_run::STAGE_EXPRESSION_REVIEW.to_owned(),
        common::expression_review_template(),
    );
    templates
}

fn image_row(id: &str, ordinal: u32) -> bgx_video_prompt_contract::inventory::ReferenceMediaInventoryItem {
    bgx_video_prompt_contract::inventory::ReferenceMediaInventoryItem {
        token: format!("tok-{id}"),
        media_kind: "reference_image".into(),
        label: format!("label-{id}"),
        item_id: id.into(),
        ordinal_within_kind: ordinal,
        ..Default::default()
    }
}

fn target_with(profile_id: &str, profile_version: u64, input_mode: &str, ids: &[&str]) -> serde_json::Value {
    let mut value = target_value(profile_id, profile_version, input_mode);
    value["referenceUseIds"] = serde_json::json!(ids);
    value
}

fn citing_reply(ids: &[&str]) -> String {
    serde_json::json!({
        "lines": [{
            "shotId": "s1",
            "factIds": ["f1"],
            "quoteIds": [],
            "referenceUseIds": ids,
            "text": "She walks."
        }],
        "conflicts": []
    })
    .to_string()
}

fn optimize_case(
    profile_id: &str,
    profile_version: u64,
    input_mode: &str,
    ids: &[&str],
    inventory: Vec<bgx_video_prompt_contract::inventory::ReferenceMediaInventoryItem>,
) -> VideoOptimizeRequest {
    VideoOptimizeRequest {
        source_text: "She walks.".into(),
        change_request: None,
        target_snapshot: target_with(profile_id, profile_version, input_mode, ids),
        format_method: format_method(),
        content_method: None,
        templates: stage_templates(),
        expansion_policy: None,
        negative_context: None,
        mother_author: Some(MotherAuthorBind {
            task_context: serde_json::json!({"sourceText": "She walks."}),
            expected_shot_ids: vec!["s1".into()],
            source_unit_ids: vec!["u1".into()],
            quote_candidates: vec![],
            source_scope: None,
            source_mode: "optimize_existing".into(),
            subject_ids: vec![],
        }),
        mother_review_base: None,
        method: frozen_method(profile_id, profile_version),
        identity_locks: Vec::new(),
        allow_inferred: false,
        existing_mother: None,
        reference_inventory: inventory,
    }
}

fn schema_reference_ids(schema: &serde_json::Value) -> Vec<String> {
    schema["properties"]["lines"]["items"]["properties"]["referenceUseIds"]["items"]["enum"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str().map(str::to_string))
        .collect()
}

fn payload_references(call: &common::CapturedCall) -> Vec<serde_json::Value> {
    for text in &call.contents {
        let mut values = serde_json::Deserializer::from_str(text).into_iter::<serde_json::Value>();
        let Some(Ok(value)) = values.next() else {
            continue;
        };
        if let Some(rows) = value.get("references").and_then(|item| item.as_array()) {
            return rows.clone();
        }
    }
    panic!("expression payload did not include references: {:?}", call.contents);
}

fn reference_identity(rows: &[serde_json::Value]) -> Vec<(String, u64)> {
    rows.iter()
        .map(|row| {
            (
                row["referenceUseId"].as_str().unwrap().to_string(),
                row["ordinalWithinKind"].as_u64().unwrap(),
            )
        })
        .collect()
}

const REVIEW_FINDING: &str = r#"{"findings":[{"code":"STYLE_NOTE","severity":"minor","detail":"tight"}]}"#;

#[test]
fn frozen_reference_rows_are_shared_by_expression_and_review() {
    use bgx_prompt_optimize::projectors::port::ProjectionSpanOrigin;
    use bgx_prompt_optimize::video_run::STAGE_EXPRESSION_REVIEW;

    let inventory = vec![
        image_row("img-c", 3),
        image_row("img-a", 1),
        image_row("img-b", 2),
    ];
    let frozen_order = ["img-c", "img-a", "img-b"];
    let model_order = ["image3", "image1", "image2"];
    let review_reply = r#"{"findings":[{"code":"STYLE_NOTE","severity":"minor","referenceUseIds":["image3"],"detail":"image3"}]}"#;
    let mut completer = RecordingCompleter::new(vec![
        ok_reply(MOTHER_AUTHOR_REPLY, "stop"),
        ok_reply(&citing_reply(&model_order), "stop"),
        ok_reply(review_reply, "stop"),
    ]);
    let result = run_video_optimize(
        optimize_case(
            "seedance",
            1,
            "multi_image_to_video",
            &frozen_order,
            inventory.clone(),
        ),
        &mut completer,
    )
    .expect("seedance three-image optimize");
    assert_eq!(
        completer.calls.iter().map(|call| call.step_label.as_str()).collect::<Vec<_>>(),
        vec![STAGE_CONTENT, STAGE_EXPRESSION, STAGE_EXPRESSION_REVIEW]
    );
    let expression = &completer.calls[1];
    let review = &completer.calls[2];
    let schema_ids = schema_reference_ids(expression.schema.as_ref().unwrap());
    assert_eq!(schema_ids.len(), 3);
    for id in &model_order {
        assert!(schema_ids.iter().any(|item| item == id), "{id} missing from schema");
    }
    assert_ne!(
        expression.schema.as_ref().unwrap()["properties"]["lines"]["items"]["properties"]["referenceUseIds"]["maxItems"],
        serde_json::json!(0)
    );
    let expression_rows = payload_references(expression);
    let review_rows = payload_references(review);
    let expected = vec![
        ("image3".into(), 3u64),
        ("image1".into(), 1),
        ("image2".into(), 2),
    ];
    assert_eq!(reference_identity(&expression_rows), expected);
    assert_eq!(reference_identity(&review_rows), expected);
    assert!(result.rejected.is_none());
    let prompt = result.final_prompt.expect("compiled");
    let projector = compiler_registry().unwrap().projector("seedance", 1).unwrap();
    for ordinal in [1u32, 2, 3] {
        let token = projector.reference_token("reference_image", ordinal).expect("seedance image token");
        assert!(prompt.contains(&token), "{token} missing from {prompt}");
    }
    let mapped: Vec<String> = result
        .source_map
        .iter()
        .filter_map(|span| match &span.origin {
            ProjectionSpanOrigin::ReferenceUse { reference_use_id } => Some(reference_use_id.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(mapped, frozen_order);
    assert_eq!(result.expression_review_raw.as_deref(), Some(review_reply));
    let resolved: serde_json::Value = serde_json::from_str(
        &result.expression_review_resolved.unwrap().unwrap()
    ).unwrap();
    assert_eq!(resolved["findings"][0]["referenceUseIds"], serde_json::json!(["img-c"]));
    assert_eq!(resolved["findings"][0]["detail"], "image3");

    let mut generic = RecordingCompleter::new(vec![
        ok_reply(MOTHER_AUTHOR_REPLY, "stop"),
        ok_reply(&citing_reply(&["image1"]), "stop"),
        ok_reply(REVIEW_FINDING, "stop"),
    ]);
    let generic_result = run_video_optimize(
        optimize_case("generic", 1, "multi_image_to_video", &["img-a"], vec![image_row("img-a", 1)]),
        &mut generic,
    )
    .expect("generic single image");
    assert_eq!(generic.calls.len(), 3);
    assert!(schema_reference_ids(generic.calls[1].schema.as_ref().unwrap()).contains(&"image1".to_string()));
    assert!(generic_result.final_prompt.is_some());
    assert!(generic_result.expression_review_raw.is_some());

    let mut h3 = RecordingCompleter::new(vec![
        ok_reply(MOTHER_AUTHOR_REPLY, "stop"),
        ok_reply(&citing_reply(&["image1"]), "stop"),
        ok_reply(REVIEW_FINDING, "stop"),
    ]);
    let h3_result = run_video_optimize(
        optimize_case("h3", 1, "multi_image_to_video", &["img-a"], vec![image_row("img-a", 1)]),
        &mut h3,
    )
    .expect("h3 single image");
    let h3_projector = compiler_registry().unwrap().projector("h3", 1).unwrap();
    let h3_token = h3_projector.reference_token("reference_image", 1).unwrap();
    assert!(h3_result.final_prompt.unwrap().contains(&h3_token));
    assert_eq!(h3.calls.len(), 3);
}

#[test]
fn empty_line_references_still_keep_the_frozen_rows_for_review() {
    use bgx_prompt_optimize::video_run::STAGE_EXPRESSION_REVIEW;
    let mut completer = RecordingCompleter::new(vec![
        ok_reply(MOTHER_AUTHOR_REPLY, "stop"),
        ok_reply(VALID_EXPRESSION_REPLY, "stop"),
        ok_reply(r#"{"findings":[]}"#, "stop"),
    ]);
    let result = run_video_optimize(
        optimize_case("generic", 1, "multi_image_to_video", &["img-a"], vec![image_row("img-a", 1)]),
        &mut completer,
    )
    .expect("omitted line reference is legal");
    assert_eq!(completer.calls.last().unwrap().step_label, STAGE_EXPRESSION_REVIEW);
    assert_eq!(reference_identity(&payload_references(&completer.calls[1])), vec![("image1".into(), 1)]);
    assert_eq!(reference_identity(&payload_references(&completer.calls[2])), vec![("image1".into(), 1)]);
    assert!(result.final_prompt.is_some());
    assert!(result.expression_review_raw.is_some());

    let mut plain = RecordingCompleter::new(vec![
        ok_reply(MOTHER_AUTHOR_REPLY, "stop"),
        ok_reply(VALID_EXPRESSION_REPLY, "stop"),
        ok_reply(r#"{"findings":[]}"#, "stop"),
    ]);
    run_video_optimize(
        optimize_case("generic", 1, "text_to_video", &[], Vec::new()),
        &mut plain,
    )
    .expect("text mode without references");
    assert!(payload_references(&plain.calls[1]).is_empty());
    assert_eq!(
        plain.calls[1].schema.as_ref().unwrap()["properties"]["lines"]["items"]["properties"]["referenceUseIds"]["maxItems"],
        serde_json::json!(0)
    );
}

#[test]
fn unknown_reference_and_missing_row_stop_before_format_review() {
    use bgx_prompt_optimize::video_run::STAGE_EXPRESSION_REVIEW;
    let mut unknown = RecordingCompleter::new(vec![
        ok_reply(MOTHER_AUTHOR_REPLY, "stop"),
        ok_reply(&citing_reply(&["not-frozen"]), "stop"),
        ok_reply(REVIEW_FINDING, "stop"),
    ]);
    let rejected = run_video_optimize(
        optimize_case("generic", 1, "multi_image_to_video", &["img-a"], vec![image_row("img-a", 1)]),
        &mut unknown,
    )
    .expect("unknown id is a rejected draft, not a transport error");
    assert_eq!(rejected.rejected.unwrap().code, error_codes::VIDEO_PROMPT_MODEL_REFERENCE_INVALID);
    assert!(rejected.expression_review_raw.is_none());
    assert!(unknown.calls.iter().all(|call| call.step_label != STAGE_EXPRESSION_REVIEW));
    assert_eq!(unknown.calls.len(), 2);

    let mut missing = RecordingCompleter::new(vec![
        ok_reply(MOTHER_AUTHOR_REPLY, "stop"),
        ok_reply(&citing_reply(&["image1"]), "stop"),
        ok_reply(REVIEW_FINDING, "stop"),
    ]);
    let err = run_video_optimize(
        optimize_case("generic", 1, "multi_image_to_video", &["img-a"], Vec::new()),
        &mut missing,
    )
    .expect_err("missing inventory row");
    assert_eq!(err.code, error_codes::VIDEO_PROMPT_REFERENCE_ROW_MISSING);
    assert_eq!(missing.calls.len(), 1);
    assert!(missing.calls.iter().all(|call| call.step_label != STAGE_EXPRESSION && call.step_label != STAGE_EXPRESSION_REVIEW));

    let conflict = image_row("img-a", 1);
    let other = image_row("img-a", 2);
    let err = run_video_optimize(
        optimize_case("generic", 1, "multi_image_to_video", &["img-a"], vec![conflict, other]),
        &mut RecordingCompleter::new(vec![ok_reply(MOTHER_AUTHOR_REPLY, "stop")]),
    )
    .expect_err("conflicting rows");
    assert_eq!(err.code, error_codes::VIDEO_PROMPT_REFERENCE_ROW_CONFLICT);
}

#[test]
fn ordinary_speech_flows_inline_to_a_single_h3_document() {
    let source = "Worker: 把箱子放这里。 Her colleague moves the box. Worker: 谢谢。";
    let request = || {
        let mut request = optimize_case("generic", 1, "text_to_video", &[], vec![]);
        request.templates.retain(|stage, _| stage == STAGE_CONTENT || stage == STAGE_EXPRESSION);
        request.templates.insert(bgx_prompt_optimize::video_run::STAGE_SOURCE.into(), content_template());
        request.templates.insert(bgx_prompt_optimize::video_run::STAGE_LYRIC_SOURCE.into(), content_template());
        request.source_text = source.into();
        request.mother_author.as_mut().unwrap().task_context = serde_json::json!({"currentDraft": source});
        request
    };
    let mut mother: serde_json::Value = serde_json::from_str(MOTHER_AUTHOR_REPLY).unwrap();
    mother["shots"][0]["facts"][0]["text"] = serde_json::json!(source);
    let mut expression: serde_json::Value = serde_json::from_str(VALID_EXPRESSION_REPLY).unwrap();
    let final_body = "integrated_multimodal_description: The worker (S1) says <d>[Chinese] 把箱子放这里。</d> before the box moves. Then she says <d>[Chinese] 谢谢。</d>\noverall_soundscape: Unspecified.\nnon_diegetic_music: Unspecified.";
    expression["lines"][0]["text"] = serde_json::json!(final_body);
    let mut completer = RecordingCompleter::new(vec![
        ok_reply(&mother.to_string(), "stop"),
        ok_reply(&expression.to_string(), "stop"),
    ]);

    let result = run_video_optimize(request(), &mut completer).unwrap();
    let content_schema = completer.calls[0].schema.as_ref().unwrap();
    assert_eq!(content_schema["properties"]["shots"]["items"]["properties"]["quotes"]["maxItems"], 0);
    assert!(result.mother.unwrap().shots[0].quotes.is_empty());
    assert!(completer.calls[1].contents.join("\n").contains(source));
    assert_eq!(result.final_prompt.as_deref(), Some(final_body));
    assert_eq!(completer.calls.len(), 2);

    mother["shots"][0]["facts"][0]["quotes"] = serde_json::json!(["Q1"]);
    let mut completer = RecordingCompleter::new(vec![ok_reply(&mother.to_string(), "stop")]);
    assert!(run_video_optimize(request(), &mut completer).unwrap().pending.is_some());
    assert_eq!(completer.calls.len(), 1);
}

#[test]
fn native_video_projectors_preserve_inline_speech() {
    let speech = "请把箱子放这里。";
    for (profile, version, mode) in [
        ("h3", 1, "text_to_video"),
        ("wan", 1, "text_to_video"),
        ("seedance", 1, "text_to_video"),
        ("kling.classic", 1, "text_to_video"),
        ("kling.omni", 2, "text_to_video"),
    ] {
        let mut request = optimize_case(profile, version, mode, &[], vec![]);
        request.templates.retain(|stage, _| stage == STAGE_CONTENT || stage == STAGE_EXPRESSION);
        let source = format!("The worker looks up and says: {speech}");
        request.mother_author.as_mut().unwrap().task_context = serde_json::json!({"currentDraft": source});
        let mut mother: serde_json::Value = serde_json::from_str(MOTHER_AUTHOR_REPLY).unwrap();
        mother["shots"][0]["facts"][0]["text"] = serde_json::json!(source);
        let mut expression: serde_json::Value = serde_json::from_str(VALID_EXPRESSION_REPLY).unwrap();
        expression["lines"][0]["text"] = serde_json::json!(source);
        let mut completer = RecordingCompleter::new(vec![
            ok_reply(&mother.to_string(), "stop"),
            ok_reply(&expression.to_string(), "stop"),
        ]);
        let result = run_video_optimize(request, &mut completer)
            .unwrap_or_else(|error| panic!("{profile}: {error:?}"));
        let text = result.final_prompt.unwrap_or_else(|| panic!("{profile} did not compile"));
        assert_eq!(text.matches(speech).count(), 1, "{profile}: {text}");
        assert!(result.mother.unwrap().shots[0].quotes.is_empty(), "{profile}");
    }
}


#[test]
fn all_registered_profiles_use_the_same_reference_number_contract() {
    let registry = bgx_prompt_optimize::production_registry().unwrap();
    for profile in registry.identities() {
        let mode = profile.input_modes.iter()
            .find(|mode| mode.required_media == ["reference_image"])
            .or_else(|| profile.input_modes.iter().find(|mode| mode.required_media == ["first_frame"]))
            .unwrap_or_else(|| panic!("{} needs a reference fixture", profile.key()));
        let kind = mode.required_media[0];
        let number = if kind == "first_frame" { "firstFrame1" } else { "image1" };
        let id = "74b95d32-74e7-4959-94f2-34bf8aa3a831";
        let mut row = image_row(id, 1);
        row.label = "reference".into();
        row.binding_id = Some(id.into());
        row.media_kind = kind.into();
        let mut completer = RecordingCompleter::new(vec![
            ok_reply(MOTHER_AUTHOR_REPLY, "stop"),
            ok_reply(&citing_reply(&[number]), "stop"),
            ok_reply(REVIEW_FINDING, "stop"),
        ]);
        let result = run_video_optimize(
            optimize_case(profile.profile_id, profile.profile_version, mode.id, &[id], vec![row]),
            &mut completer,
        ).unwrap_or_else(|error| panic!("{}: {error:?}", profile.key()));
        assert!(result.final_prompt.is_some(), "{}: {:?}", profile.key(), result.rejected);
        assert_eq!(result.draft.unwrap().lines[0].reference_use_ids, [id]);
        assert_eq!(schema_reference_ids(completer.calls[1].schema.as_ref().unwrap()), [number]);
        for call in &completer.calls[1..] {
            assert!(call.contents.join("\n").contains(number), "{}", profile.key());
            assert!(!call.contents.join("\n").contains(id), "{}", profile.key());
        }
    }
}

#[test]
fn seven_picture_h3_replay_rejects_unknown_numbers_and_old_ids() {
    let ids = [
        "74b95d32-74e7-4959-94f2-34bf8aa3a831", "picture-use-2", "picture-use-3",
        "picture-use-4", "picture-use-5", "picture-use-6", "picture-use-7",
    ];
    let numbers = ["image1", "image2", "image3", "image4", "image5", "image6", "image7"];
    for language in ["zh", "en"] {
        for first in ["image1", "image8", "74b95d32-74f7-4959-94f2-34bf8aa3a831",
            "74b95d32-74b7-4959-94f2-34bf8aa3a831", ids[0]] {
            let mut reply_numbers = numbers;
            reply_numbers[0] = first;
            let expression_reply = citing_reply(&reply_numbers);
            let mut completer = RecordingCompleter::new(vec![
                ok_reply(MOTHER_AUTHOR_REPLY, "stop"),
                ok_reply(&expression_reply, "stop"),
                ok_reply(r#"{"findings":[{"referenceUseIds":["image8"]}]}"#, "stop"),
            ]);
            let rows = ids.iter().enumerate().map(|(i, id)| {
                let mut row = image_row(id, i as u32 + 1);
                row.label = "picture".into();
                row
            }).collect();
            let mut request = optimize_case("generic", 1, "multi_image_to_video", &ids, rows);
            request.format_method.stable_id = "h3-prompt-writing".into();
            request.target_snapshot["expressionLanguage"] = serde_json::json!(language);
            let result = run_video_optimize(request, &mut completer).unwrap();
            assert_eq!(schema_reference_ids(completer.calls[1].schema.as_ref().unwrap()), numbers);
            assert!(!completer.calls[1].contents.join("\n").contains(ids[0]));
            if first == "image1" {
                assert_eq!(result.draft.unwrap().lines[0].reference_use_ids, ids);
                assert!(result.final_prompt.is_some());
                let error = result.expression_review_resolved.unwrap().unwrap_err();
                assert!(error.contains("findings[0].referenceUseIds[0]"));
            } else {
                let rejected = result.rejected.unwrap();
                assert_eq!(rejected.code, error_codes::VIDEO_PROMPT_MODEL_REFERENCE_INVALID);
                assert_eq!(rejected.raw_reply.as_deref(), Some(expression_reply.as_str()));
                assert!(result.final_prompt.is_none());
                assert_eq!(completer.calls.len(), 2);
            }
        }
    }
}


#[test]
fn standalone_keeps_frozen_numbers_without_reference_analysis() {
    use bgx_prompt_optimize::video_run::STAGE_REFERENCE;

    let mut request = optimize_case(
        "generic", 1, "multi_image_to_video", &["img-a", "img-c"],
        vec![image_row("img-a", 1), image_row("img-c", 3)],
    );
    let source = "人物按图 1，赤脚走三步后回头。机位固定，环境按图 3。";
    request.source_text = source.into();
    request.mother_author.as_mut().unwrap().task_context["currentDraft"] = serde_json::json!(source);
    let template = serde_json::json!({
        "formatVersion": 2, "task": STAGE_REFERENCE, "kind": "request",
        "messages": [{
            "id": "user", "role": "user", "parts": [
                {"input": "taskContext", "encoding": "json"},
                {"input": "outputContract", "encoding": "json"}
            ]
        }]
    });
    request.templates.insert(
        STAGE_REFERENCE.into(),
        common::pack_task_template(STAGE_REFERENCE, &template.to_string()),
    );
    let mut completer = RecordingCompleter::new(vec![
        ok_reply(MOTHER_AUTHOR_REPLY, "stop"),
        ok_reply(&citing_reply(&["image1", "image3"]), "stop"),
        ok_reply(REVIEW_FINDING, "stop"),
    ]);
    let result = run_video_optimize(request, &mut completer).unwrap();
    assert!(completer.calls.iter().all(|call| call.step_label != STAGE_REFERENCE));
    assert!(completer.calls[0].contents.join("\n").contains(source));
    for call in &completer.calls {
        assert!(call.image_urls.is_empty());
        let sent = call.contents.join("\n");
        assert!(
            sent.contains("用户对目标画面的明确文字要求优先")
                || sent.contains("The user's explicit text takes precedence"),
            "{} must apply the common rule", call.step_label
        );
    }
    assert_eq!(schema_reference_ids(completer.calls[1].schema.as_ref().unwrap()), ["image1", "image3"]);
    assert_eq!(result.draft.unwrap().lines[0].reference_use_ids, ["img-a", "img-c"]);
}


#[test]
fn generic_keeps_voice_timbre_references_in_prose_with_no_structured_binding_offer() {
    let audio_refs = ["voice-one", "voice-two"].into_iter().enumerate().map(|(index, item_id)| {
        bgx_video_prompt_contract::inventory::ReferenceMediaInventoryItem {
            item_id: item_id.into(),
            media_kind: "reference_audio".into(),
            ordinal_within_kind: index as u32 + 1,
            audio_usage: Some("voice_timbre".into()),
            ..Default::default()
        }
    }).collect();
    let request = optimize_case(
        "generic", 1, "multi_image_to_video",
        &["voice-one", "voice-two"], audio_refs,
    );
    let draft = serde_json::json!({
        "lines": [{
            "shotId": "s1", "factIds": ["f1"], "quoteIds": [],
            "referenceUseIds": ["audio1", "audio2"],
            "text": "She walks. Use <Audio 1> for the first speaker and <Audio 2> for the second speaker."
        }],
        "conflicts": [], "speakerVoiceBindings": []
    });
    let mut completer = RecordingCompleter::new(vec![
        ok_reply(MOTHER_AUTHOR_REPLY, "stop"),
        ok_reply(&draft.to_string(), "stop"),
        ok_reply(REVIEW_FINDING, "stop"),
    ]);
    let result = run_video_optimize(request, &mut completer).unwrap();
    let expression = &completer.calls[1];
    let payload = expression.contents.iter().find_map(|text| {
        let value = serde_json::Deserializer::from_str(text).into_iter::<serde_json::Value>().next()?.ok()?;
        value.get("speakerVoiceBinding")?;
        Some(value)
    }).expect("expression payload");
    assert!(payload["speakerVoiceBinding"].is_null());
    let references = payload_references(expression);
    assert_eq!(reference_identity(&references), [("audio1".into(), 1), ("audio2".into(), 2)]);
    assert!(references.iter().all(|item| item["audioUsage"] == "voice_timbre"));
    assert_eq!(expression.schema.as_ref().unwrap()["properties"]["speakerVoiceBindings"]["maxItems"], 0);
    assert!(result.rejected.is_none());
    let prompt = result.final_prompt.expect("compiled prompt");
    assert!(prompt.contains("<Audio 1>") && prompt.contains("<Audio 2>"));
}
