use bgx_prompt_optimize::sdk::protocol::{OptimizeArgs, TextMode};
use serde_json::json;

#[test]
fn optimize_request_guards_target_prompt_digest_and_api_keys() {
    let args: OptimizeArgs = serde_json::from_value(json!({
        "targetId":"image/qwen-image-2.1","sourcePrompt":" 雨夜公交车 "
    }))
    .unwrap();
    args.validate().unwrap();
    assert_eq!(args.source_prompt, " 雨夜公交车 ");
    assert_eq!(args.text_mode, TextMode::Configured);
    assert!(serde_json::from_value::<OptimizeArgs>(json!({
        "targetId":"image/qwen-image-2.1","sourcePrompt":"雨夜公交车","apiKey":"invalid"
    }))
    .is_err());
    let invalid: OptimizeArgs = serde_json::from_value(json!({
        "targetId":"image/qwen-image-2.1","sourcePrompt":"雨夜公交车","expectedMethodDigest":"old"
    }))
    .unwrap();
    assert_eq!(invalid.validate().unwrap_err().code, "INVALID_FRAME");
}
