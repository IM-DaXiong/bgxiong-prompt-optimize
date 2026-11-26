//! T28e local GGUF text. No story.db. Occupancy label and inference state are explicit ports.
//! The host (or shell) constructs and owns llama-server / GPU state; this crate never
//! calls `global_local_ai()` or hard-codes the `"main"` window label.

use crate::openai::shared::{
    detect_local_thinking_capability, LlmContentPart, TextImageCountPolicy, TextTransportContent,
    TextTransportMessage, ThinkingParamFamily, ThinkingRequest,
};

#[derive(Debug, Clone)]
pub struct LocalModelView {
    pub id: String,
    pub name: String,
    pub model_path: std::path::PathBuf,
}

#[derive(Debug, Clone, Default)]
pub struct LocalTextGenerationRequest {
    pub model_id: String,
    pub system_prompt: Option<String>,
    pub user_prompt: String,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub stream: Option<bool>,
    pub response_format_json: bool,
    pub output_schema: Option<serde_json::Value>,
    pub enable_thinking: Option<bool>,
    pub reasoning_effort: Option<&'static str>,
    pub user_image_data_urls: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct LocalTextGenerationResponse {
    pub content: String,
    pub tokens_used: Option<u64>,
    pub elapsed_ms: u64,
    pub finish_reason: Option<String>,
}

/// Host-owned local runtime (config lookup + llama-server inference).
pub trait LocalTextRuntime {
    fn get_config(&self, model_id: &str) -> Option<LocalModelView>;
    fn infer(
        &self,
        config: &LocalModelView,
        request: LocalTextGenerationRequest,
    ) -> Result<LocalTextGenerationResponse, String>;
}

fn flatten_prompts(roles_and_text: &[(String, String)]) -> (String, String, String) {
    let final_prompt = roles_and_text
        .iter()
        .map(|(_, c)| c.as_str())
        .collect::<Vec<_>>()
        .join("\n\n---\n\n");
    let system_prompt = roles_and_text
        .iter()
        .filter(|(role, _)| role == "system")
        .map(|(_, c)| c.as_str())
        .collect::<Vec<_>>()
        .join("\n\n");
    let user_prompt = roles_and_text
        .iter()
        .filter(|(role, _)| role != "system")
        .map(|(_, c)| c.as_str())
        .collect::<Vec<_>>()
        .join("\n\n");
    (final_prompt, system_prompt, user_prompt)
}

fn submitted_text(content: &TextTransportContent) -> String {
    match content {
        TextTransportContent::Text(text) => text.clone(),
        TextTransportContent::Parts(parts) => parts
            .iter()
            .filter_map(|part| match part {
                LlmContentPart::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

pub fn extract_image_data_urls(messages: &[TextTransportMessage]) -> Vec<String> {
    let mut urls = Vec::new();
    for m in messages {
        match &m.content {
            TextTransportContent::Text(_) => {}
            TextTransportContent::Parts(parts) => {
                for p in parts {
                    if let LlmContentPart::ImageUrl { image_url } = p {
                        let u = image_url.url.trim();
                        if !u.is_empty() {
                            urls.push(u.to_string());
                        }
                    }
                }
            }
        }
    }
    urls
}

fn resolve_thinking(
    config: &LocalModelView,
    thinking: Option<ThinkingRequest>,
) -> Result<(Option<bool>, Option<&'static str>), String> {
    match thinking {
        Some(req) => {
            let file_name = config
                .model_path
                .file_name()
                .and_then(|s| s.to_str());
            let file_stem = config
                .model_path
                .file_stem()
                .and_then(|s| s.to_str());
            let cap = detect_local_thinking_capability(&config.name, file_name, file_stem);
            if !cap.supported {
                return Err(format!(
                    "thinking mode requested but local model is not in whitelist: id={} name={}",
                    config.id, config.name
                ));
            }
            match cap.param_family {
                Some(ThinkingParamFamily::QwenEnableThinking) => Ok((Some(req.enabled), None)),
                Some(ThinkingParamFamily::GptOssReasoningEffort) => {
                    Ok((None, Some(if req.enabled { "medium" } else { "low" })))
                }
                other => Err(format!(
                    "thinking mode param_family unsupported for local model: id={} family={other:?}",
                    config.id
                )),
            }
        }
        None => Ok((None, None)),
    }
}

fn missing_model_error() -> String {
    "未指定本地模型：请在设置中选择一个本地文本模型。".to_string()
}

/// Complete local text from transport messages.
///
/// `occupancy_label` is supplied by the caller: the host passes `"main"`, the shell
/// passes its own window label. This function never looks up a global local-AI state.
pub fn complete_from_transport_messages(
    runtime: &dyn LocalTextRuntime,
    model_id: Option<&str>,
    occupancy_label: &str,
    require_occupancy: &dyn Fn(&str) -> Result<(), String>,
    messages: &[TextTransportMessage],
    temperature: Option<f32>,
    response_format_json: bool,
    output_schema: Option<&serde_json::Value>,
    thinking: Option<ThinkingRequest>,
    image_count_policy: TextImageCountPolicy,
) -> Result<(String, Option<String>), String> {
    let model_id = model_id.unwrap_or("").trim();
    if model_id.is_empty() {
        return Err(missing_model_error());
    }
    require_occupancy(occupancy_label)?;
    let config = runtime
        .get_config(model_id)
        .ok_or_else(|| format!("本地模型配置不存在: {model_id}"))?;
    let image_urls = extract_image_data_urls(messages);
    let roles_and_text: Vec<(String, String)> = messages
        .iter()
        .map(|m| {
            let text = if image_count_policy == TextImageCountPolicy::AsSubmitted {
                submitted_text(&m.content)
            } else {
                m.content.text_preview()
            };
            (m.role.clone(), text)
        })
        .collect();
    let (_final_prompt, system_prompt, user_prompt) = flatten_prompts(&roles_and_text);
    let (enable_thinking, reasoning_effort) = resolve_thinking(&config, thinking)?;
    let request = LocalTextGenerationRequest {
        model_id: model_id.to_string(),
        system_prompt: if system_prompt.is_empty() {
            None
        } else {
            Some(system_prompt)
        },
        user_prompt,
        temperature,
        max_tokens: None,
        stream: Some(false),
        response_format_json,
        output_schema: output_schema.cloned(),
        enable_thinking,
        reasoning_effort,
        user_image_data_urls: image_urls,
    };
    let resp = runtime.infer(&config, request)?;
    Ok((resp.content, resp.finish_reason))
}

pub fn complete_from_chat_messages(
    runtime: &dyn LocalTextRuntime,
    model_id: Option<&str>,
    occupancy_label: &str,
    require_occupancy: &dyn Fn(&str) -> Result<(), String>,
    messages: &[crate::openai::ChatMessage],
    temperature: Option<f32>,
    response_format_json: bool,
    thinking: Option<ThinkingRequest>,
) -> Result<(String, Option<String>), String> {
    let transport: Vec<TextTransportMessage> = messages
        .iter()
        .map(|m| TextTransportMessage {
            role: m.role.clone(),
            content: TextTransportContent::Text(m.content.clone()),
            reasoning_content: None,
        })
        .collect();
    complete_from_transport_messages(
        runtime,
        model_id,
        occupancy_label,
        require_occupancy,
        &transport,
        temperature,
        response_format_json,
        None,
        thinking,
        TextImageCountPolicy::ModelCapability,
    )
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RecommendedModel {
    pub name: String,
    pub description: String,
    pub size_gb: f64,
    pub url: String,
    pub suggested_filename: String,
}

pub fn supported_model_formats() -> Vec<&'static str> {
    vec!["gguf", "ggml"]
}

pub fn recommended_models() -> Vec<RecommendedModel> {
    vec![
        RecommendedModel {
            name: "Qwen3-4B-Instruct-2507 (Q4_K_M)".to_string(),
            description: "阿里 Qwen3 4B 指令版，轻量省显存，中文优秀".to_string(),
            size_gb: 2.5,
            url: "https://huggingface.co/bartowski/Qwen_Qwen3-4B-Instruct-2507-GGUF".to_string(),
            suggested_filename: "Qwen_Qwen3-4B-Instruct-2507-Q4_K_M.gguf".to_string(),
        },
        RecommendedModel {
            name: "Qwen3-8B (Q4_K_M)".to_string(),
            description: "阿里 Qwen3 8B，故事/剧本主力，支持思考模式".to_string(),
            size_gb: 4.9,
            url: "https://huggingface.co/bartowski/Qwen_Qwen3-8B-GGUF".to_string(),
            suggested_filename: "Qwen_Qwen3-8B-Q4_K_M.gguf".to_string(),
        },
        RecommendedModel {
            name: "Llama-3.3-8B-Instruct (Q4_K_M)".to_string(),
            description: "Meta Llama 3.3 8B，英文/多语通用".to_string(),
            size_gb: 4.9,
            url: "https://huggingface.co/bartowski/allura-forge_Llama-3.3-8B-Instruct-GGUF"
                .to_string(),
            suggested_filename: "allura-forge_Llama-3.3-8B-Instruct-Q4_K_M.gguf".to_string(),
        },
        RecommendedModel {
            name: "DeepSeek-R1-Distill-Llama-8B (Q4_K_M)".to_string(),
            description: "DeepSeek R1 蒸馏版，复杂推理与结构化输出".to_string(),
            size_gb: 4.9,
            url: "https://huggingface.co/bartowski/DeepSeek-R1-Distill-Llama-8B-GGUF".to_string(),
            suggested_filename: "DeepSeek-R1-Distill-Llama-8B-Q4_K_M.gguf".to_string(),
        },
        RecommendedModel {
            name: "Gemma-3-4B-it (Q4_K_M)".to_string(),
            description: "Google Gemma 3 4B 指令版，轻量多语".to_string(),
            size_gb: 2.5,
            url: "https://huggingface.co/bartowski/google_gemma-3-4b-it-GGUF".to_string(),
            suggested_filename: "google_gemma-3-4b-it-Q4_K_M.gguf".to_string(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openai::shared::LlmImageUrl;
    use std::sync::{Arc, Mutex};

    #[test]
    fn recommended_models_has_five_gguf_entries() {
        let models = recommended_models();
        assert_eq!(models.len(), 5);
        for m in &models {
            assert!(
                m.suggested_filename.ends_with(".gguf"),
                "expected .gguf: {}",
                m.suggested_filename
            );
            assert!(!m.name.is_empty());
            assert!(!m.url.is_empty());
            assert!(m.size_gb > 0.0);
        }
        assert_eq!(
            models[0].suggested_filename,
            "Qwen_Qwen3-4B-Instruct-2507-Q4_K_M.gguf"
        );
        assert_eq!(models[1].suggested_filename, "Qwen_Qwen3-8B-Q4_K_M.gguf");
    }

    #[test]
    fn missing_model_id_does_not_start_cloud_or_runtime() {
        let runtime = RecordingRuntime::default();
        let err = complete_from_transport_messages(
            &runtime,
            None,
            "shell-window",
            &|_| panic!("occupancy must not run when model is missing"),
            &[TextTransportMessage {
                role: "user".into(),
                content: TextTransportContent::Text("hi".into()),
                reasoning_content: None,
            }],
            None,
            false,
            None,
            None,
            TextImageCountPolicy::ModelCapability,
        )
        .unwrap_err();
        assert_eq!(
            err,
            "未指定本地模型：请在设置中选择一个本地文本模型。"
        );
        assert!(runtime.inferred.lock().expect("log").is_empty());
    }

    #[test]
    fn occupancy_uses_caller_label_not_hardcoded_main() {
        let runtime = RecordingRuntime::default();
        runtime.configs.lock().expect("cfg").insert(
            "local-1".into(),
            LocalModelView {
                id: "local-1".into(),
                name: "Qwen3-8B".into(),
                model_path: "Qwen_Qwen3-8B-Q4_K_M.gguf".into(),
            },
        );
        let labels = Arc::new(Mutex::new(Vec::new()));
        let labels2 = labels.clone();
        let (content, finish) = complete_from_transport_messages(
            &runtime,
            Some("local-1"),
            "prompt-optimize-shell",
            &move |label| {
                labels2.lock().expect("labels").push(label.to_string());
                Ok(())
            },
            &[TextTransportMessage {
                role: "user".into(),
                content: TextTransportContent::Text("hello".into()),
                reasoning_content: None,
            }],
            Some(0.2),
            false,
            None,
            None,
            TextImageCountPolicy::ModelCapability,
        )
        .expect("local complete");
        assert_eq!(content, "local-ok");
        assert_eq!(finish.as_deref(), Some("stop"));
        assert_eq!(
            *labels.lock().expect("labels"),
            vec!["prompt-optimize-shell".to_string()]
        );
        let infer = runtime.inferred.lock().expect("log").clone();
        assert_eq!(infer.len(), 1);
        assert_eq!(infer[0].user_prompt, "hello");
        assert_eq!(infer[0].model_id, "local-1");
    }

    #[test]
    fn media_does_not_add_preview_markers_to_submitted_text() {
        let text = "  原文 [image]\n保留  ";
        let content = TextTransportContent::Parts(vec![
            LlmContentPart::Text { text: text.into() },
            LlmContentPart::ImageUrl {
                image_url: LlmImageUrl {
                    url: "data:image/png;base64,abc".into(),
                    detail: None,
                },
            },
        ]);
        assert_eq!(submitted_text(&content), text);
        assert_eq!(
            extract_image_data_urls(&[TextTransportMessage {
                role: "user".into(),
                content,
                reasoning_content: None
            }])
            .len(),
            1
        );
    }

    #[derive(Default)]
    struct RecordingRuntime {
        configs: Mutex<std::collections::HashMap<String, LocalModelView>>,
        inferred: Mutex<Vec<LocalTextGenerationRequest>>,
    }

    impl LocalTextRuntime for RecordingRuntime {
        fn get_config(&self, model_id: &str) -> Option<LocalModelView> {
            self.configs.lock().expect("cfg").get(model_id).cloned()
        }
        fn infer(
            &self,
            _config: &LocalModelView,
            request: LocalTextGenerationRequest,
        ) -> Result<LocalTextGenerationResponse, String> {
            self.inferred.lock().expect("log").push(request);
            Ok(LocalTextGenerationResponse {
                content: "local-ok".into(),
                tokens_used: None,
                elapsed_ms: 1,
                finish_reason: Some("stop".into()),
            })
        }
    }
}
