//! T30: closed-set schema must be present on the expression TextCall, not only
//! checked after the model reply. Output-field and rejection contracts go through
//! the public `run_video_expression` entry. This is not a second T34 golden set.

mod common;

use bgx_prompt_optimize::error::error_codes;
use bgx_prompt_optimize::projectors::compiler_registry;
use bgx_prompt_optimize::projectors::port::{ProjectionCompileInput, ReviewedMother};
use bgx_prompt_optimize::{draft_schema_for, run_video_expression};
use common::{
    expected_closed_set, expected_expression_schema, expression_request, ok_reply,
    RecordingCompleter, OUTSIDE_CLOSED_SET_REPLY, VALID_EXPRESSION_REPLY,
};

#[test]
fn run_video_expression_sends_closed_set_schema_on_text_call() {
    let request = expression_request("generic", 1, "text_to_video");
    let expected_schema = expected_expression_schema(&request);
    let mut completer = RecordingCompleter::new(vec![ok_reply(VALID_EXPRESSION_REPLY, "stop")]);
    let result = run_video_expression(request, &mut completer).expect("valid closed-set draft");

    assert_eq!(completer.calls.len(), 1);
    let call = &completer.calls[0];
    assert_eq!(call.step_label, "video-expression");
    assert!(call.json_mode);
    assert_eq!(
        call.schema.as_ref(),
        Some(&expected_schema),
        "TextCall.schema must be draft_schema_for(closed) built before complete"
    );
    assert!(result.rejected.is_none());
    assert!(result.draft.is_some());
    assert!(result.final_prompt.is_some());
    // Structural: the bound enum is the closed fact/shot set, not an open string.
    let lines = expected_schema["properties"]["lines"]["items"]["properties"].clone();
    assert_eq!(lines["shotId"]["enum"], serde_json::json!(["s1"]));
    assert_eq!(lines["factIds"]["items"]["enum"], serde_json::json!(["f1"]));
    let fact_enum = lines["factIds"]["items"]["enum"].as_array().unwrap();
    assert!(!fact_enum.iter().any(|value| value == "not-a-fact"));
}

#[test]
fn expression_writes_from_source_timing_even_when_render_duration_differs() {
    let mut request = expression_request("generic", 1, "text_to_video");
    request.duration_seconds = Some(26.0);
    request.capability_snapshot["durationSeconds"] = serde_json::json!(26.0);
    let action = "12–15秒，女子（五官参考图1）转身，停留0.55秒。";
    request.mother.shots[0].facts[0].text = action.into();
    let target = bgx_prompt_optimize::video_job::target_from_request(&request).unwrap();
    let payload = bgx_prompt_optimize::video_job::expression_payload(&request, &target, "fingerprint").unwrap();
    assert_eq!(target.duration_seconds, Some(26.0));
    assert!(payload.get("durationSeconds").is_none());
    assert!(payload["capabilitySnapshot"].get("durationSeconds").is_none());
    assert_eq!(payload["shots"][0]["facts"][0]["text"], action);
}

#[test]
fn run_video_expression_success_final_prompt_matches_compiler() {
    let request = expression_request("generic", 1, "text_to_video");
    let mut completer = RecordingCompleter::new(vec![ok_reply(VALID_EXPRESSION_REPLY, "stop")]);
    let result = run_video_expression(request, &mut completer).expect("valid closed-set draft");
    let draft = result
        .draft
        .expect("success must retain the expression draft");
    let prompt = result
        .final_prompt
        .as_deref()
        .expect("success must return the compiled prompt");
    assert!(result.rejected.is_none());

    let request = expression_request("generic", 1, "text_to_video");
    let target = bgx_prompt_optimize::video_job::target_from_request(&request).unwrap();
    let fingerprint = "unused-for-generic-text";
    let compiled = compiler_registry()
        .unwrap()
        .projector("generic", 1)
        .unwrap()
        .compile(&ProjectionCompileInput {
            mother: ReviewedMother {
                document: &request.mother,
                canonical_fingerprint: fingerprint,
            },
            references: &[],
            identity_locks: &[],
            target: &target,
            method: None,
            expression: Some(&draft),
        })
        .unwrap();
    assert_eq!(prompt, compiled.text);
}

#[test]
fn run_video_expression_rejects_fact_outside_closed_set_without_final_prompt() {
    let request = expression_request("generic", 1, "text_to_video");
    let expected_schema = expected_expression_schema(&request);
    let mut completer = RecordingCompleter::new(vec![ok_reply(OUTSIDE_CLOSED_SET_REPLY, "stop")]);
    let result = run_video_expression(request, &mut completer)
        .expect("rejection is a result, not a runner error");

    assert_eq!(completer.calls.len(), 1);
    assert_eq!(completer.calls[0].schema.as_ref(), Some(&expected_schema));
    let rejected = result
        .rejected
        .expect("closed-set miss must populate rejected");
    assert!(result.draft.is_none());
    assert!(result.final_prompt.is_none());
    assert_eq!(
        rejected.raw_reply.as_deref(),
        Some(OUTSIDE_CLOSED_SET_REPLY)
    );
    assert!(
        rejected.detail.contains("not-a-fact")
            || rejected.code.contains("CLOSED")
            || rejected.code.contains("ID"),
        "rejected must name the illegal id or a closed-set/id code, got {} / {}",
        rejected.code,
        rejected.detail
    );
    // Schema still forbids the illegal id even though the scripted reply tried it.
    let closed = expected_closed_set(&expression_request("generic", 1, "text_to_video"));
    let schema = draft_schema_for(&closed);
    let fact_enum = schema["properties"]["lines"]["items"]["properties"]["factIds"]["items"]
        ["enum"]
        .as_array()
        .unwrap();
    assert!(!fact_enum.iter().any(|value| value == "not-a-fact"));
}

#[test]
fn run_video_expression_cancel_before_complete_does_not_read_reply() {
    let request = expression_request("generic", 1, "text_to_video");
    let mut completer = RecordingCompleter::cancelling();
    let err = run_video_expression(request, &mut completer).expect_err("cancelled");
    assert_eq!(err.code, error_codes::OPTIMIZE_CANCELLED);
    assert!(completer.calls.is_empty());
}

#[test]
fn unregistered_projector_is_refused_without_generic_fallback() {
    let request = expression_request("HappyHorse", 1, "text_to_video");
    let mut completer = RecordingCompleter::new(vec![ok_reply(VALID_EXPRESSION_REPLY, "stop")]);
    let err = run_video_expression(request, &mut completer).expect_err("unregistered");
    assert_eq!(err.code, error_codes::VIDEO_PROMPT_PROJECTOR_MISSING);
    assert!(
        err.message.contains("不会改用通用档案"),
        "missing projector must not fall back to generic: {}",
        err.message
    );
    assert!(completer.calls.is_empty());
}

#[test]
fn unsupported_input_mode_is_refused_before_complete() {
    let request = expression_request("generic", 1, "not-a-closed-mode");
    let mut completer = RecordingCompleter::new(vec![ok_reply(VALID_EXPRESSION_REPLY, "stop")]);
    let err = run_video_expression(request, &mut completer).expect_err("unsupported mode");
    assert_eq!(err.code, error_codes::VIDEO_PROMPT_INPUT_MODE_UNSUPPORTED);
    assert!(completer.calls.is_empty());
}
