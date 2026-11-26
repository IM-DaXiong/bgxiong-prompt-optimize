//! Request an explicit thinking disable only for text channels that declare it.

use std::path::Path;

use bgx_text_http::openai::shared::{detect_local_thinking_capability, detect_thinking_capability};
use bgx_text_http::ThinkingRequest;

use crate::config::{ShellConfig, LOCAL_PROVIDER_KIND};

pub fn thinking_for_call(config: &ShellConfig, structured_output: bool) -> Option<ThinkingRequest> {
    if !structured_output {
        return None;
    }
    let capability = if config.provider_kind == LOCAL_PROVIDER_KIND {
        let path = Path::new(config.local_model_path.as_deref().unwrap_or_default());
        detect_local_thinking_capability(
            config.model.as_deref().unwrap_or_default(),
            path.file_name().and_then(|name| name.to_str()),
            path.file_stem().and_then(|name| name.to_str()),
        )
    } else {
        detect_thinking_capability(
            &config.provider_kind,
            config.model.as_deref().unwrap_or_default(),
        )
    };
    capability
        .supported
        .then_some(ThinkingRequest { enabled: false })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structured_disables_thinking_when_supported() {
        let config = ShellConfig {
            provider_kind: "text_openai_compat".into(),
            model: Some("qwen3-32b".into()),
            ..Default::default()
        };
        assert!(
            !thinking_for_call(&config, true)
                .expect("explicit disable")
                .enabled
        );
        assert!(thinking_for_call(&config, false).is_none());
    }

    #[test]
    fn structured_output_does_not_require_provider_thinking_support() {
        let config = ShellConfig {
            provider_kind: "text_openai_compat".into(),
            model: Some("custom-text-model".into()),
            ..Default::default()
        };
        assert!(thinking_for_call(&config, true).is_none());
    }
}
