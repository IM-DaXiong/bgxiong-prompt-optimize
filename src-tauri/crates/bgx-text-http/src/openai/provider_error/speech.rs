//! Cloud TTS / Doubao seed-audio / DashScope speech.

use super::signal::{
    contains_any_case_insensitive, extract_http_status_prefix, extract_numeric_code_from_text,
};
use super::{hit, ProviderErrorMatch};

pub fn match_speech(api_type: &str, provider_kind: &str, raw: &str) -> Option<ProviderErrorMatch> {
    let speech = api_type == "speech" || api_type == "audio" || api_type == "tts";
    if !speech {
        return None;
    }
    let code = extract_numeric_code_from_text(raw);

    if provider_kind == "speech_volc_seed_audio" {
        if code == Some(45000010) || contains_any_case_insensitive(raw, &["invalid x-api-key"]) {
            return Some(hit("openspeech_invalid_api_key", false));
        }
        if extract_http_status_prefix(raw).as_deref() == Some("401")
            || contains_any_case_insensitive(raw, &["unauthorized", "invalid api", "x-api-key"])
        {
            return Some(hit("openspeech_invalid_api_key", false));
        }
        if code == Some(45000030)
            || contains_any_case_insensitive(
                raw,
                &["requested resource not granted", "volc.service_type.10074"],
            )
        {
            return Some(hit("resource_not_granted", false));
        }
        if contains_any_case_insensitive(raw, &["\"code\":", "code != 0", "合成失败"]) {
            return Some(hit("openspeech_tts_failed", false));
        }
    }
    if provider_kind == "speech_dashscope" {
        if extract_http_status_prefix(raw).as_deref() == Some("401")
            || contains_any_case_insensitive(raw, &["invalidapikey", "no api-key", "unauthorized"])
        {
            return Some(hit("auth_failed", false));
        }
        // CosyVoice voice-enrollment create_voice: HTTP 500 + InternalError / feat extract.
        if extract_http_status_prefix(raw).as_deref() == Some("500")
            || contains_any_case_insensitive(
                raw,
                &[
                    "\"code\":\"InternalError\"",
                    "feat extract error",
                    "internal server error",
                ],
            )
        {
            return Some(hit("vendor_internal_error", true));
        }
        if contains_any_case_insensitive(raw, &["缺少 voice", "voice_id", "声音设计"]) {
            return Some(hit("voice_not_configured", false));
        }
    }
    None
}
