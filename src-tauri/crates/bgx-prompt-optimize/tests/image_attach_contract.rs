//! T30: attachment order, cap, and image TextCall contracts through public `run_image`.
//! Existing lib tests already cover 1-image attach, cancel, and 17-as-submitted reject;
//! this file adds the missing multi-image order and optional-channel keep-first-N path.

mod common;

use bgx_prompt_optimize::error::error_codes;
use bgx_prompt_optimize::image_job::output_schema;
use bgx_prompt_optimize::text_port::TextImageCountPolicy;
use bgx_prompt_optimize::vision::{encode_original_image_bytes, MAX_IPC_RELS};
use bgx_prompt_optimize::{run_image, ImageOptimizeRequest, TextVisionCaps, VisionChannel};
use common::{official_image_template, ok_reply, submitted, RecordingCompleter};

fn image_request() -> ImageOptimizeRequest {
    ImageOptimizeRequest {
        task_id: "job-attach".into(),
        user_text: "把人物换成红衣".into(),
        method: None,
        template: official_image_template(),
        language: "zh".into(),
        reference_image_count: 0,
        images: Vec::new(),
        text_vision: TextVisionCaps {
            supports_vision: true,
        },
        channel: VisionChannel::AsSubmitted,
    }
}

fn prompt_reply(
) -> Result<bgx_prompt_optimize::text_port::TextReply, bgx_prompt_optimize::error::OptimizeError> {
    ok_reply(r#"{"optimizedPrompt":"红衣人物站在原背景前"}"#, "stop")
}

#[test]
fn as_submitted_zero_images_is_text_only_and_sends_output_schema() {
    let request = image_request();
    let expected_schema = output_schema().expect("image output schema");
    let mut completer = RecordingCompleter::new(vec![prompt_reply()]);
    let result = run_image(request, &mut completer).expect("zero images is allowed");
    assert_eq!(completer.calls.len(), 1);
    let call = &completer.calls[0];
    assert!(call.image_urls.is_empty());
    assert_eq!(call.image_count_policy, TextImageCountPolicy::AsSubmitted);
    assert_eq!(call.schema.as_ref(), Some(&expected_schema));
    assert_eq!(result.input_mode, "text");
    assert_eq!(result.attached_image_count, 0);
    assert!(!result.vision_truncated);
    assert_eq!(result.optimized_prompt, "红衣人物站在原背景前");
    assert_eq!(
        result.raw_reply,
        r#"{"optimizedPrompt":"红衣人物站在原背景前"}"#
    );
    assert_eq!(result.finish_reason.as_deref(), Some("stop"));
}

#[test]
fn as_submitted_three_images_keep_submission_order_on_text_call() {
    let mut request = image_request();
    request.images = vec![
        submitted(1, 16, 16, [11, 22, 33]),
        submitted(2, 16, 18, [44, 55, 66]),
        submitted(3, 18, 16, [77, 88, 99]),
    ];
    request.reference_image_count = 3;
    let expected_urls: Vec<String> = request
        .images
        .iter()
        .map(|image| encode_original_image_bytes(&image.bytes).unwrap().data_url)
        .collect();
    assert_eq!(expected_urls.len(), 3);
    assert_ne!(expected_urls[0], expected_urls[1]);
    assert_ne!(expected_urls[1], expected_urls[2]);

    let mut completer = RecordingCompleter::new(vec![prompt_reply()]);
    let result = run_image(request, &mut completer).expect("three as-submitted images");
    assert_eq!(completer.calls[0].image_urls, expected_urls);
    assert_eq!(
        completer.calls[0].image_count_policy,
        TextImageCountPolicy::AsSubmitted
    );
    assert_eq!(result.attached_image_count, 3);
    assert_eq!(result.input_mode, "vision");
    assert!(!result.vision_truncated);
}

#[test]
fn as_submitted_seventeen_images_fail_before_complete() {
    let mut request = image_request();
    request.images = (1..=MAX_IPC_RELS + 1)
        .map(|i| submitted(i, 16, 16, [7, 8, 9]))
        .collect();
    request.reference_image_count = MAX_IPC_RELS + 1;
    let mut completer = RecordingCompleter::new(vec![prompt_reply()]);
    let err = run_image(request, &mut completer).expect_err("over cap");
    assert_eq!(err.code, error_codes::IMAGE_PROMPT_REFERENCE_LIMIT);
    assert!(err.message.contains("上限"));
    assert!(completer.calls.is_empty());
}

#[test]
fn optional_channel_keeps_first_max_ipc_rels_in_submission_order() {
    let mut request = image_request();
    request.channel = VisionChannel::Optional;
    request.images = (1..=MAX_IPC_RELS + 1)
        .map(|i| {
            let tint = (i % 200) as u8;
            submitted(i, 16, 16, [tint, 20, 30])
        })
        .collect();
    request.reference_image_count = MAX_IPC_RELS + 1;
    let expected_urls: Vec<String> = request
        .images
        .iter()
        .take(MAX_IPC_RELS as usize)
        .map(|image| encode_original_image_bytes(&image.bytes).unwrap().data_url)
        .collect();

    let mut completer = RecordingCompleter::new(vec![prompt_reply()]);
    let result = run_image(request, &mut completer).expect("optional cap is not a hard error");
    assert_eq!(completer.calls.len(), 1);
    assert_eq!(completer.calls[0].image_urls, expected_urls);
    assert_eq!(
        completer.calls[0].image_count_policy,
        TextImageCountPolicy::ModelCapability
    );
    assert_eq!(result.attached_image_count, MAX_IPC_RELS);
    assert!(result.vision_truncated);
    assert_eq!(result.input_mode, "vision");
}


#[test]
fn ordinary_optimization_keeps_four_reference_ordinals_and_never_encodes_images() {
    let mut request = image_request();
    request.channel = VisionChannel::default();
    request.user_text = "人物按图 1，赤脚。图 2 是环境，图 3 是道具，图 4 是光线。".into();
    request.reference_image_count = 4;
    // Invalid bytes must never reach an image decoder in the reference-only mode.
    request.images = vec![bgx_prompt_optimize::SubmittedImage {
        bytes: vec![0],
        declared_order: 1,
    }];
    let mut completer = RecordingCompleter::new(vec![ok_reply(
        r#"{"optimizedPrompt":"人物按图 1，赤脚。图 2 是环境，图 3 是道具，图 4 是光线。"}"#,
        "stop",
    )]);
    let result = run_image(request, &mut completer).unwrap();
    let call = &completer.calls[0];
    let sent = call.contents.join("\n");
    assert!(sent.contains("赤脚"));
    assert!(sent.contains("\"referenceImageOrder\":[1,2,3,4]"), "{sent}");
    assert!(call.image_urls.is_empty());
    assert_eq!(result.attached_image_count, 0);
    assert_eq!(result.vision_skip_reason, "");
}
