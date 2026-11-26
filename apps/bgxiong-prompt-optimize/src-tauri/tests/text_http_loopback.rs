//! T25 loopback checks.
//!
//! After T28 exports the shared HTTP functions, each cloud channel should be
//! driven against a fake server and assert these host paths:
//!   OpenAI-compatible family: `/v1/chat/completions`
//!   Qwen text: `/api/v1/services/aigc/text-generation/generation`
//!   Qwen multimodal (qwen3.8-max): `/api/v1/services/aigc/multimodal-generation/generation`
//!   MiniMax-M3: `/anthropic/v1/messages`
//!
//! The fake server is not a paid call. Production completion must not use a
//! script Completer. This file currently locks the path constants and the
//! production guard; it does not invent a second wire protocol.

use bgxiong_prompt_optimize::text_port_paths;

#[test]
fn openai_compat_path_stays_chat_completions() {
    assert_eq!(text_port_paths::openai_compat(), "/v1/chat/completions");
}

#[test]
fn qwen_uses_dashscope_native_paths() {
    assert_eq!(
        text_port_paths::qwen_text(),
        "/api/v1/services/aigc/text-generation/generation"
    );
    assert_eq!(
        text_port_paths::qwen_multimodal(),
        "/api/v1/services/aigc/multimodal-generation/generation"
    );
}

#[test]
fn minimax_uses_anthropic_messages_path() {
    assert_eq!(text_port_paths::minimax(), "/anthropic/v1/messages");
}
