//! H3 reference-audio usage closed set (INV-H3-AUDIO-USE).
//! Generated from `packages/app-contracts/v1/h3-reference-audio-usage.json`.

include!(concat!(env!("OUT_DIR"), "/h3_reference_audio_usage.rs"));

pub fn parse_h3_reference_audio_usage(raw: Option<&str>) -> &'static str {
    match raw.map(str::trim).filter(|s| !s.is_empty()) {
        Some(H3_REFERENCE_AUDIO_USAGE_SOUNDTRACK_COPY) => H3_REFERENCE_AUDIO_USAGE_SOUNDTRACK_COPY,
        Some(H3_REFERENCE_AUDIO_USAGE_VOICE_TIMBRE) => H3_REFERENCE_AUDIO_USAGE_VOICE_TIMBRE,
        _ => H3_REFERENCE_AUDIO_USAGE_DEFAULT,
    }
}

pub fn is_soundtrack_copy_usage(raw: Option<&str>) -> bool {
    parse_h3_reference_audio_usage(raw) == H3_REFERENCE_AUDIO_USAGE_SOUNDTRACK_COPY
}

/// Empty stays the closed-set default. A non-empty value outside the two-value set is an error.
pub fn parse_explicit_h3_reference_audio_usage(raw: Option<&str>) -> Result<&'static str, ()> {
    match raw.map(str::trim).filter(|s| !s.is_empty()) {
        None => Ok(H3_REFERENCE_AUDIO_USAGE_DEFAULT),
        Some(H3_REFERENCE_AUDIO_USAGE_SOUNDTRACK_COPY) => {
            Ok(H3_REFERENCE_AUDIO_USAGE_SOUNDTRACK_COPY)
        }
        Some(H3_REFERENCE_AUDIO_USAGE_VOICE_TIMBRE) => Ok(H3_REFERENCE_AUDIO_USAGE_VOICE_TIMBRE),
        Some(_) => Err(()),
    }
}

/// Whether every explicitly declared reference audio uses soundtrack_copy.
pub fn inventory_all_audio_is_soundtrack_copy(inventory: &[bgx_video_prompt_contract::inventory::ReferenceMediaInventoryItem]) -> bool {
    let mut saw = false;
    for it in inventory
        .iter()
        .filter(|it| it.media_kind.trim() == "reference_audio")
    {
        saw = true;
        if !is_soundtrack_copy_usage(it.audio_usage.as_deref()) {
            return false;
        }
    }
    saw
}
