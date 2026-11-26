//! Unique vendor-error matcher (INV-AITASK-ERR-05~08).
//! Display copy lives in `Locale.providerErrors`; this module only emits `category` + raw `message`.

use serde_json::{json, Value};

use super::user_facing_api_error::{
    encode_user_facing_api_error, user_hint_from_provider_error_wire, UserFacingApiError,
    USER_FACING_API_ERROR_PREFIX,
};

pub const ERR_CONTRACT_INVALID: &str = "LOCAL_GPU_OCCUPANCY_CONTRACT_INVALID";


mod dashscope;
mod generic;
mod jimeng;
mod kling;
mod minimax;
mod policy;
mod seedance;
mod signal;
mod speech;
mod host_error_codes;

pub use signal::{
    extract_http_status_prefix, extract_numeric_code_from_text, extract_request_id_from_text,
};

/// wire.`message` 落盘上限（完整原文进 tracing）
pub const WIRE_MESSAGE_MAX_CHARS: usize = 4096;

#[derive(Debug, Clone)]
pub struct ProviderErrorMatch {
    pub category: String,
    pub retryable: bool,
    pub params: Option<Value>,
    /// Canonical vendor code established by a strict provider-owned parser.
    pub authoritative_vendor_code: Option<String>,
}

pub(crate) fn hit(category: &str, retryable: bool) -> ProviderErrorMatch {
    ProviderErrorMatch {
        category: category.to_string(),
        retryable,
        params: None,
        authoritative_vendor_code: None,
    }
}

pub(crate) fn hit_params(category: &str, retryable: bool, params: Value) -> ProviderErrorMatch {
    ProviderErrorMatch {
        category: category.to_string(),
        retryable,
        params: Some(params),
        authoritative_vendor_code: None,
    }
}

pub fn truncate_wire_message(raw: &str) -> String {
    let n = raw.chars().count();
    if n <= WIRE_MESSAGE_MAX_CHARS {
        return raw.to_string();
    }
    let head: String = raw.chars().take(WIRE_MESSAGE_MAX_CHARS).collect();
    format!("{head}…")
}

/// Unique public entry: always `__BGX_ERR_JSON__` (never raw vendor JSON).
pub fn to_user_facing_error(api_type: &str, provider_kind: &str, raw: String) -> String {
    if wire_starts_with_user_facing_prefix(&raw) {
        return raw;
    }
    if raw.contains(ERR_CONTRACT_INVALID) {
        return encode_user_facing_api_error(&UserFacingApiError {
            api_type: "infra".into(),
            provider: provider_kind.to_string(),
            category: "infra".into(),
            message: truncate_wire_message(&raw),
            code: Some(ERR_CONTRACT_INVALID.to_string()),
            request_id: None,
            retryable: false,
            user_hint: String::new(),
            params: None,
        });
    }
    let matched = match_provider_user_hint(api_type, provider_kind, &raw)
        .unwrap_or_else(|| hit("vendor_request_failed", false));
    encode_matched_provider_error(api_type, provider_kind, raw, matched)
}

pub fn match_provider_user_hint(
    api_type: &str,
    provider_kind: &str,
    raw: &str,
) -> Option<ProviderErrorMatch> {
    if let Some(m) = kling::match_kling(provider_kind, raw) {
        return Some(m);
    }
    if let Some(m) = seedance::match_output_video_copyright(api_type, raw) {
        return Some(m);
    }
    if let Some(m) = jimeng::match_jimeng(api_type, provider_kind, raw) {
        return Some(m);
    }
    if let Some(m) = minimax::match_minimax(provider_kind, raw) {
        return Some(m);
    }
    if let Some(m) = policy::match_policy(api_type, provider_kind, raw) {
        return Some(m);
    }
    if let Some(m) = seedance::match_seedance(api_type, provider_kind, raw) {
        return Some(m);
    }
    if let Some(m) = dashscope::match_dashscope(api_type, provider_kind, raw) {
        return Some(m);
    }
    if let Some(m) = speech::match_speech(api_type, provider_kind, raw) {
        return Some(m);
    }
    generic::match_generic(api_type, provider_kind, raw)
}

pub fn encode_matched_provider_error(
    api_type: &str,
    provider_kind: &str,
    raw: String,
    matched: ProviderErrorMatch,
) -> String {
    let message = truncate_wire_message(&raw);
    let code = matched.authoritative_vendor_code.clone()
        .or_else(|| extract_numeric_code_from_text(&raw).map(|c| c.to_string()));
    let request_id = extract_request_id_from_text(&raw);
    let http_status = extract_http_status_prefix(&raw);

    // Host logs this match through tracing; the crate emits the same wire without a tracing dep.
    encode_user_facing_api_error(&UserFacingApiError {
        api_type: api_type.to_string(),
        provider: provider_kind.to_string(),
        category: matched.category,
        message,
        code: code.or(http_status),
        request_id,
        retryable: matched.retryable,
        user_hint: String::new(),
        params: matched.params,
    })
}

/// 视频 poll：未命中厂商表时用 caller fallback category（Comfy 宕机等）。
pub fn ensure_video_provider_wire(
    provider_kind: &str,
    raw: String,
    fallback_category: &str,
    _fallback_hint: &str,
) -> String {
    if wire_starts_with_user_facing_prefix(&raw) {
        return raw;
    }
    if let Some(m) = match_provider_user_hint("video", provider_kind, &raw) {
        return encode_matched_provider_error("video", provider_kind, raw, m);
    }
    encode_matched_provider_error(
        "video",
        provider_kind,
        raw,
        generic::fallback_category(fallback_category),
    )
}

pub fn wire_starts_with_user_facing_prefix(s: &str) -> bool {
    s.trim_start().starts_with(USER_FACING_API_ERROR_PREFIX)
}

pub fn extract_provider_wire_from_err(s: &str) -> Option<&str> {
    let t = s.trim();
    if wire_starts_with_user_facing_prefix(t) {
        return Some(t);
    }
    const IMAGE_WRAP: &str = "生图失败: ";
    if let Some(rest) = t.strip_prefix(IMAGE_WRAP) {
        let r = rest.trim();
        if wire_starts_with_user_facing_prefix(r) {
            return Some(r);
        }
    }
    if let Some(idx) = t.find(USER_FACING_API_ERROR_PREFIX) {
        return Some(&t[idx..]);
    }
    None
}

/// Only the transport's structured delivery classification can mark a call as unresolved.
/// Local preparation failures and a received provider rejection are known failures.
pub fn is_outcome_unknown_error(err: &str) -> bool {
    extract_provider_wire_from_err(err)
        .and_then(|wire| wire.strip_prefix(USER_FACING_API_ERROR_PREFIX))
        .and_then(|body| serde_json::from_str::<UserFacingApiError>(body.trim()).ok())
        .is_some_and(|error| error.category == "network_ambiguous")
}

pub fn finish_enrich_from_provider_wire(wire: &str) -> Value {
    let user_hint = user_hint_from_provider_error_wire(wire).unwrap_or_default();
    let mut obj = json!({
        "userHint": user_hint,
    });
    let Some(json) = wire
        .strip_prefix(USER_FACING_API_ERROR_PREFIX)
        .map(str::trim)
    else {
        return obj;
    };
    let Ok(v) = serde_json::from_str::<Value>(json) else {
        return obj;
    };
    if let Some(c) = v.get("category").and_then(|x| x.as_str()) {
        obj["category"] = Value::String(c.to_string());
        obj["failureKind"] = Value::String(failure_kind_for_category(c).to_string());
        obj["failurePhase"] = Value::String(failure_phase_for_category(c).to_string());
    } else {
        obj["failurePhase"] = Value::String("poll".to_string());
        obj["failureKind"] = Value::String("vendor_generation_failed".to_string());
    }
    if let Some(code) = v.get("code").and_then(|x| x.as_str()) {
        if code.contains("LOCAL_GPU_OCCUPANCY") {
            obj["failurePhase"] = Value::String("validate".into());
            obj["failureKind"] = Value::String("validation_rejected".into());
        } else if matches!(code,
            host_error_codes::workflow::RESOLUTION_CONTRACT_INVALID
            | host_error_codes::workflow::MAPPING_STALE
            | host_error_codes::workflow::MAPPING_NEEDS_CONFIRMATION
            | host_error_codes::workflow::PROMPT_ENHANCER_REQUIRES_CONFIGURATION
            | host_error_codes::comfyui_http::PARAM_CONTRACT_AMBIGUOUS) {
            obj["failurePhase"] = Value::String(FAILURE_PHASE_PREFLIGHT.into());
            obj["failureKind"] = Value::String("validation_rejected".into());
        } else if is_missing_node_preflight_code(code) {
            // Rejected before any prompt reached ComfyUI: not a poll failure.
            obj["failurePhase"] = Value::String(FAILURE_PHASE_PREFLIGHT.into());
            obj["failureKind"] = Value::String(FAILURE_KIND_COMFYUI_MISSING_NODE.into());
        }
    }
    if let Some(r) = v.get("retryable").and_then(|x| x.as_bool()) {
        obj["retryable"] = Value::Bool(r);
    }
    if let Some(rid) = v
        .get("requestId")
        .or_else(|| v.get("request_id"))
        .and_then(|x| x.as_str())
    {
        obj["requestId"] = Value::String(rid.to_string());
    }
    if let Some(p) = v.get("params") {
        obj["params"] = p.clone();
    }
    obj
}

/// Local ComfyUI preflight: the workflow needs node classes the connected instance lacks.
pub const FAILURE_PHASE_PREFLIGHT: &str = "preflight";
pub const FAILURE_KIND_COMFYUI_MISSING_NODE: &str = "comfyui_missing_node";

/// Error codes raised by the node-dependency preflight before submit (no prompt queued).
pub fn is_missing_node_preflight_code(code: &str) -> bool {
    use self::host_error_codes::{comfyui_http, generic};
    code == comfyui_http::VDN_NODE_MISSING
        || code == comfyui_http::MISSING_NODE_TYPE
        || code == generic::MISSING_NODES
}

pub fn failure_kind_for_category(category: &str) -> &'static str {
    match category {
        "poll_wall_timeout" => "poll_wall_timeout",
        "vendor_query_http" => "vendor_query_http",
        "vendor_query_transport" => "vendor_query_transport",
        "comfyui_process_down" => "comfyui_process_down",
        "comfyuiUnreachable" => "comfyuiUnreachable",
        "download_failed" => "download_failed",
        "validation_rejected" => "validation_rejected",
        _ => "vendor_generation_failed",
    }
}

pub fn failure_phase_for_category(category: &str) -> &'static str {
    match category {
        "poll_wall_timeout"
        | "vendor_query_http"
        | "vendor_query_transport"
        | "comfyui_process_down"
        | "comfyuiUnreachable" => "poll",
        "download_failed" => "download",
        "validation_rejected" => "validate",
        _ => "poll",
    }
}

pub fn parse_wire_category(wire: &str) -> Option<String> {
    let json = wire.strip_prefix(USER_FACING_API_ERROR_PREFIX)?.trim();
    let v: Value = serde_json::from_str(json).ok()?;
    v.get("category")
        .and_then(|x| x.as_str())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;


    // Host minimax_token_plan_original_2056 test is ported in `crate::minimax`
    // together with `decorate_minimax_anthropic_error` (needs prepared-schema SHA).

    fn category_of(wire: &str) -> String {
        parse_wire_category(wire).unwrap_or_default()
    }

    #[test]
    fn maps_output_video_copyright_seedance() {
        let raw = "The request failed because the output video may be related to copyright restrictions. Request id: abc123";
        let m = match_provider_user_hint("video", "video_seedance_2_0", raw).unwrap();
        assert_eq!(m.category, "output_video_copyright_rejected");
    }

    #[test]
    fn maps_reference_image_copyright_50518() {
        let raw = r#"{"code":50518,"message":"Pre Img Risk Not Pass: Copyright"}"#;
        let m = match_provider_user_hint("video", "video_seedance_2_0", raw).unwrap();
        assert_eq!(m.category, "reference_image_copyright_rejected");
    }

    #[test]
    fn to_user_facing_never_returns_raw() {
        let out = to_user_facing_error(
            "video",
            "video_openapi_compat",
            "vendor unknown reason".into(),
        );
        assert!(out.starts_with(USER_FACING_API_ERROR_PREFIX));
        assert_eq!(category_of(&out), "vendor_request_failed");
    }

    #[test]
    fn finish_enrich_from_copyright_wire_has_failure_kind() {
        let raw =
            "The request failed because the output video may be related to copyright restrictions.";
        let wire = encode_matched_provider_error(
            "video",
            "video_seedance_2_0",
            raw.to_string(),
            match_provider_user_hint("video", "video_seedance_2_0", raw).unwrap(),
        );
        let enrich = finish_enrich_from_provider_wire(&wire);
        assert_eq!(
            enrich.get("failureKind").and_then(|v| v.as_str()),
            Some("vendor_generation_failed")
        );
        assert_eq!(
            enrich.get("category").and_then(|v| v.as_str()),
            Some("output_video_copyright_rejected")
        );
    }

    #[test]
    fn extract_provider_wire_from_image_wrap() {
        let wire = format!("{USER_FACING_API_ERROR_PREFIX}{{\"userHint\":\"x\"}}");
        let wrapped = format!("生图失败: {wire}");
        assert_eq!(
            extract_provider_wire_from_err(&wrapped),
            Some(wire.as_str())
        );
    }

    #[test]
    fn maps_minimax_h3_insufficient_balance_context_ir_402() {
        let raw = r#"Context-IR HTTP 402 Payment Required: {"type":"error","error":{"type":"insufficient_balance_error","message":"insufficient balance (1008)","http_code":"402"},"request_id":"06d22e01848c1ff8d2a920474e1ffd75"}"#;
        let m = match_provider_user_hint("video", "video_minimax_h3", raw).unwrap();
        assert_eq!(m.category, "minimax_h3_insufficient_balance");
        assert!(!m.retryable);
        let wire = to_user_facing_error("video", "video_minimax_h3", raw.to_string());
        assert!(wire.starts_with(USER_FACING_API_ERROR_PREFIX));
        assert_eq!(category_of(&wire), "minimax_h3_insufficient_balance");
        let parsed: UserFacingApiError =
            serde_json::from_str(wire.strip_prefix(USER_FACING_API_ERROR_PREFIX).unwrap()).unwrap();
        assert!(parsed.user_hint.is_empty());
        assert!(parsed.message.contains("insufficient_balance_error"));
        assert!(parsed.message.contains("http_code"));
    }

    #[test]
    fn maps_minimax_image_status_code_1008() {
        let raw = "MiniMax 生图失败 status_code=1008: insufficient balance";
        let m = match_provider_user_hint("image", "minimax_image_t2i", raw).unwrap();
        assert_eq!(m.category, "vendor_insufficient_balance");
        assert!(!m.retryable);
    }

    #[test]
    fn maps_minimax_text_media_size_limit_apart_from_generic_invalid_request() {
        let raw = r#"HTTP 400: {"type":"error","error":{"type":"invalid_request_error","message":"media exceeds size limit: max 10485760 bytes (2013)"},"request_id":"074a7cde9d"}"#;
        let m = match_provider_user_hint("text", "text_minimax_m3", raw).unwrap();
        assert_eq!(m.category, "media_too_large");
        assert!(!m.retryable);
    }

    #[test]
    fn maps_minimax_h3_token_plan_or_credit_unsupported() {
        let raw = r#"Context-IR HTTP 400 Bad Request: {"type":"error","error":{"type":"bad_request_error","message":"invalid params, TokenPlan 或 Credit 暂不支持 MiniMax-H3 系列模型 (2013)","http_code":"400"},"request_id":"06cf6f1d4298ae138217f3613be682a7"}"#;
        let m = match_provider_user_hint("video", "video_minimax_h3", raw).unwrap();
        assert_eq!(m.category, "minimax_h3_token_plan_or_credit_unsupported");
        assert!(!m.retryable);
        let wire = to_user_facing_error("video", "video_minimax_h3", raw.to_string());
        assert_eq!(
            category_of(&wire),
            "minimax_h3_token_plan_or_credit_unsupported"
        );
        assert!(!category_of(&wire).contains("400"));
    }

    #[test]
    fn maps_volc_seed_audio_resource_not_granted_45000030() {
        let raw = r#"豆包 seed-audio 合成失败 (HTTP 403): {"code":45000030,"message":"[resource_id=volc.service_type.10074] requested resource not granted"}"#;
        let m = match_provider_user_hint("speech", "speech_volc_seed_audio", raw).unwrap();
        assert_eq!(m.category, "resource_not_granted");
    }

    #[test]
    fn maps_volc_seed_audio_invalid_api_key_45000010() {
        let raw = r#"豆包 seed-audio 合成失败 (HTTP 401): {"code":45000010,"message":"Invalid X-Api-Key"}"#;
        let m = match_provider_user_hint("audio", "speech_volc_seed_audio", raw).unwrap();
        assert_eq!(m.category, "openspeech_invalid_api_key");
    }

    #[test]
    fn dashscope_pascal_case_data_inspection_failed_matches_content_policy() {
        let raw =
            "DataInspectionFailed: Output data may contain inappropriate content. Request id: x";
        let m = match_provider_user_hint("video", "video_dashscope_wan", raw).unwrap();
        assert_eq!(m.category, "provider_content_policy_rejected_video");
        assert!(!m.retryable);
    }

    #[test]
    fn maps_seg_video_reference_capacity_exceeded() {
        let raw =
            "SEG_VIDEO_REFERENCE_CAPACITY_EXCEEDED: role=reference_image declaredCount=4 maxAllowed=1";
        let m = match_provider_user_hint("video", "video_dashscope_wan", raw).unwrap();
        assert_eq!(m.category, "seg_video_reference_capacity_exceeded");
        let p = m.params.expect("params");
        assert_eq!(p.get("declaredCount").and_then(|v| v.as_u64()), Some(4));
        assert_eq!(p.get("maxAllowed").and_then(|v| v.as_u64()), Some(1));
        assert!(!m.retryable);
    }

    #[test]
    fn ideogram_local_safety_hint_must_not_say_cloud_vendor() {
        let raw = "Ideogram 4 安全过滤：输出为灰底占位图（Image blocked by safety filter）。请改写提示词。";
        let m = match_provider_user_hint("image", "comfyui", raw).unwrap();
        assert_eq!(m.category, "local_ideogram_safety_blocked");
    }

    #[test]
    fn kling_access_key_1002() {
        let raw = r#"HTTP 401 Unauthorized: {"code":1002,"message":"access key not found","request_id":"abc"}"#;
        let m = match_provider_user_hint("video", "video_kling", raw).unwrap();
        assert_eq!(m.category, "kling_access_key_invalid");
    }

    #[test]
    fn maps_dashscope_voice_design_http_500_internal_error_feat_extract() {
        let raw = r#"HTTP 500: {"request_id":"6df2c2e3-5a58-9a3c-8785-b9c907c6cc63","code":"InternalError","message":"feat extract error"}"#;
        let m = match_provider_user_hint("audio", "speech_dashscope", raw).unwrap();
        assert_eq!(m.category, "vendor_internal_error");
        assert!(m.retryable);
        let wire = to_user_facing_error("audio", "speech_dashscope", raw.to_string());
        assert!(wire.starts_with(USER_FACING_API_ERROR_PREFIX));
        assert_eq!(category_of(&wire), "vendor_internal_error");
        let parsed: UserFacingApiError =
            serde_json::from_str(wire.strip_prefix(USER_FACING_API_ERROR_PREFIX).unwrap()).unwrap();
        assert!(parsed.user_hint.is_empty());
        assert!(parsed.message.contains("feat extract error"));
        assert_eq!(
            parsed.request_id.as_deref(),
            Some("6df2c2e3-5a58-9a3c-8785-b9c907c6cc63")
        );
    }
}
