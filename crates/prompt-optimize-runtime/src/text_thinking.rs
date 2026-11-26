//! Thinking flag for shell TextCompleter exits.
//!
//! Structured json / schema always sends ThinkingRequest { enabled: false }.
//! Unstructured calls must use the existing provider/model whitelist in
//! `src-tauri/src/open_platform/thinking_detection.rs` (no contains("think")
//! heuristics). That module lives in the main program crate and depends on
//! adapter host / local model config; the shell cannot link it. D should
//! extract the no-DB whitelist to a shared crate. Until then, unstructured
//! requests pass None rather than inventing enabled:true or copying the list.

use bgx_text_http::ThinkingRequest;

pub fn thinking_for_call(structured_output: bool) -> Option<ThinkingRequest> {
    if structured_output {
        Some(ThinkingRequest { enabled: false })
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structured_disables_thinking() {
        let thinking = thinking_for_call(true).expect("explicit disable");
        assert!(!thinking.enabled);
    }

    #[test]
    fn unstructured_does_not_invent_enabled() {
        assert!(thinking_for_call(false).is_none());
    }
}
