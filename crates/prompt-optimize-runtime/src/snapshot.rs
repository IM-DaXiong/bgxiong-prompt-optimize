//! Build VideoPromptTargetSnapshot from the form. The user never types internal JSON,
//! never submits an empty snapshot, and the shell never falls back to generic.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use bgx_prompt_optimize::OptimizeError;

use crate::error_ui;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VideoTargetForm {
    pub profile_id: String,
    pub profile_version: u64,
    pub input_mode: String,
    pub duration_seconds: Option<f64>,
    pub expression_language: String,
    pub reference_use_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BuiltTargetSnapshot {
    pub snapshot: Value,
    pub caps_digest: String,
}

pub fn build_target_snapshot(form: &VideoTargetForm) -> Result<BuiltTargetSnapshot, OptimizeError> {
    if form.profile_id.trim().is_empty() {
        return Err(error_ui::err(
            "VIDEO_TARGET_REQUIRED",
            "请选择一个视频档案。",
        ));
    }
    if form.profile_id.eq_ignore_ascii_case("happyhorse") {
        return Err(error_ui::err(
            "VIDEO_PROMPT_PROJECTOR_MISSING",
            "HappyHorse 没有投影器，不能使用。",
        ));
    }
    if form.profile_version < 1 {
        return Err(error_ui::err(
            "VIDEO_TARGET_REQUIRED",
            "档案版本无效。",
        ));
    }
    if form.input_mode.trim().is_empty() {
        return Err(error_ui::err(
            "VIDEO_TARGET_REQUIRED",
            "请选择该档案支持的输入模式。",
        ));
    }
    if let Some(seconds) = form.duration_seconds {
        if !seconds.is_finite() || seconds <= 0.0 {
            return Err(error_ui::err(
                "VIDEO_DURATION_INVALID",
                "时长必须是大于 0 的数字，空白表示不填，不会改成默认秒数。",
            ));
        }
    }
    let language = match form.expression_language.as_str() {
        "en" => "en",
        "zh" => "zh",
        other if other.trim().is_empty() => "zh",
        other => {
            return Err(error_ui::err(
                "VIDEO_LANGUAGE_INVALID",
                format!("界面语言只支持 zh 或 en，收到的是 {other}"),
            ));
        }
    };

    // Canonical JSON of the form fields (except capsDigest). sha256 hex, no sha256: prefix,
    // matching targetSnapshot.capsDigest = sha256Hex.
    let canonical = json!({
        "profileId": form.profile_id,
        "profileVersion": form.profile_version,
        "inputMode": form.input_mode,
        "durationSeconds": form.duration_seconds,
        "expressionLanguage": language,
        "referenceUseIds": form.reference_use_ids,
    });
    let canonical_text = serde_json::to_vec(&canonical)
        .map_err(|_| error_ui::err("VIDEO_TARGET_SNAPSHOT", "无法生成目标快照。"))?;
    let caps_digest = sha256_hex(&canonical_text);

    let snapshot = json!({
        "profileId": form.profile_id,
        "profileVersion": form.profile_version,
        "inputMode": form.input_mode,
        "durationSeconds": form.duration_seconds,
        "capsDigest": caps_digest,
        "expressionLanguage": language,
        "referenceUseIds": form.reference_use_ids,
    });
    Ok(BuiltTargetSnapshot {
        snapshot,
        caps_digest,
    })
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for b in digest {
        out.push(char::from_digit((b >> 4) as u32, 16).unwrap_or('0'));
        out.push(char::from_digit((b & 0x0f) as u32, 16).unwrap_or('0'));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form() -> VideoTargetForm {
        VideoTargetForm {
            profile_id: "h3".into(),
            profile_version: 1,
            input_mode: "text_to_video".into(),
            duration_seconds: None,
            expression_language: "zh".into(),
            reference_use_ids: vec![],
        }
    }

    #[test]
    fn fills_caps_digest_and_does_not_fallback_to_generic() {
        let built = build_target_snapshot(&form()).unwrap();
        assert_eq!(built.caps_digest.len(), 64);
        assert!(built.caps_digest.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(built.snapshot["profileId"], "h3");
        assert_ne!(built.snapshot["profileId"], "generic");
    }

    #[test]
    fn happyhorse_is_rejected() {
        let mut form = form();
        form.profile_id = "happyhorse".into();
        let err = build_target_snapshot(&form).unwrap_err();
        assert!(err.message.contains("HappyHorse"), "{}", err.message);
    }

    #[test]
    fn invalid_duration_is_rejected_not_rewritten() {
        let mut form = form();
        form.duration_seconds = Some(0.0);
        let err = build_target_snapshot(&form).unwrap_err();
        assert!(err.message.contains("时长"), "{}", err.message);
        form.duration_seconds = Some(-3.0);
        assert!(build_target_snapshot(&form).is_err());
    }

    #[test]
    fn empty_profile_is_rejected() {
        let mut form = form();
        form.profile_id = "  ".into();
        assert!(build_target_snapshot(&form).is_err());
    }
}
