use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Literary / storyboard request. Success field remains `prompt`, not `optimizedPrompt`.
/// The payload follows `LiteraryPromptOpen` in content-authoring-stages-v1; host fills taskContext.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LiteraryOptimizeRequest {
    #[serde(default)]
    pub task_id: String,
    pub open: Value,
    pub template: crate::skill_material::FrozenSkillBytes,
    #[serde(default)]
    pub creative_method: Option<crate::skill_material::FrozenSkillBytes>,
    #[serde(default)]
    pub language: String,
    /// Official template input `outputContract`. Host-filled; omitted from frozen JSON.
    #[serde(default, skip)]
    pub output_contract: Option<Value>,
    /// Optional multimodal attachments already encoded by the host (data URLs).
    #[serde(default, skip)]
    pub image_data_urls: Vec<String>,
}
/// Literary / storyboard success. The field is `prompt`, never `optimizedPrompt`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LiteraryOptimizeResult {
    pub prompt: String,
    pub raw_reply: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
    #[serde(default)]
    pub output_incomplete: bool,
}

const LITERARY_TASK: &str = "text-prompt-optimize";

/// Synchronous literary / storyboard optimize. Host fills `open` / taskContext.
pub fn run_literary<C: crate::text_port::TextCompleter>(
    request: LiteraryOptimizeRequest,
    completer: &mut C,
) -> Result<LiteraryOptimizeResult, crate::error::OptimizeError> {
    use std::collections::BTreeMap;

    use serde_json::Value;

    use crate::error::{error_codes, OptimizeError};
    use crate::run::is_provider_output_truncated;
    use crate::template::{render_request, unpack_rules};
    use crate::text_port::{no_cancel, TextCall, TextMessage};

    let snapshot = unpack_rules(&request.template)?;
    if !snapshot.tasks.contains_key(LITERARY_TASK) {
        return Err(OptimizeError::new(
            error_codes::INTERNAL_PROMPT_CONFIG,
            "literary template task must be text-prompt-optimize",
        ));
    }
    let mut inputs: BTreeMap<String, Value> = BTreeMap::new();
    inputs.insert("taskContext".into(), request.open.clone());
    if let Some(method) = &request.creative_method {
        inputs.insert("creativeMethod".into(), Value::String(method.body.clone()));
    }
    if !request.language.is_empty() {
        inputs.insert("language".into(), Value::String(request.language.clone()));
    }
    if let Some(contract) = request.output_contract {
        inputs.insert("outputContract".into(), contract);
    }
    let rendered = render_request(&snapshot, LITERARY_TASK, &inputs)?;
    let mut messages: Vec<TextMessage> = rendered
        .into_iter()
        .map(|message| TextMessage {
            role: message.role,
            content: Value::String(message.content),
        })
        .collect();
    if !request.image_data_urls.is_empty() {
        messages = crate::run::splice_last_user_image_parts(messages, &request.image_data_urls)?;
    }
    completer.check_cancel()?;
    let reply = completer.complete(TextCall {
        messages,
        json_mode: true,
        schema: None,
        step_label: if request.task_id.is_empty() {
            LITERARY_TASK.to_owned()
        } else {
            request.task_id.clone()
        },
        check_cancel: no_cancel,
        image_count_policy: crate::text_port::TextImageCountPolicy::ModelCapability,
    })?;
    completer.check_cancel()?;
    let output_incomplete = is_provider_output_truncated(reply.finish_reason.as_deref());
    let value: Value = serde_json::from_str(reply.content.trim()).map_err(|error| {
        OptimizeError::new("LITERARY_JSON_INVALID", format!("LITERARY_JSON_INVALID: {error}"))
            .with_raw_reply(reply.content.clone())
    })?;
    let prompt = value
        .get("prompt")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            OptimizeError::new(
                "LITERARY_JSON_INVALID",
                "literary reply must contain a string field prompt, not optimizedPrompt",
            )
            .with_raw_reply(reply.content.clone())
        })?;
    Ok(LiteraryOptimizeResult {
        prompt: prompt.to_string(),
        raw_reply: reply.content,
        finish_reason: reply.finish_reason,
        output_incomplete,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::OptimizeError;
    use crate::template::{freeze_from_files, pack_rules, Language};
    use crate::text_port::{TextCall, TextCompleter, TextReply};
    use serde_json::json;
    use std::collections::BTreeMap;

    struct OnceCompleter(TextReply);

    impl TextCompleter for OnceCompleter {
        fn complete(&mut self, _call: TextCall) -> Result<TextReply, OptimizeError> {
            Ok(self.0.clone())
        }
    }

    #[test]
    fn literary_success_field_is_prompt_not_optimized_prompt() {
        let request_json = r#"{"formatVersion":2,"task":"text-prompt-optimize","kind":"request","messages":[{"id":"user","role":"user","parts":[{"input":"taskContext","encoding":"json"}]}]}"#;
        let mut files = BTreeMap::new();
        files.insert("text-prompt-optimize/request.json".into(), request_json.into());
        let snapshot = freeze_from_files(Language::Zh, &["text-prompt-optimize"], &files).unwrap();
        let template = pack_rules(&snapshot).unwrap();
        let request = LiteraryOptimizeRequest {
            task_id: "lit-1".into(),
            open: json!({"shot":"s1"}),
            template,
            creative_method: None,
            language: "zh".into(),
            output_contract: None,
            image_data_urls: Vec::new(),
        };
        let mut completer = OnceCompleter(TextReply {
            content: r#"{"prompt":"成稿一句","optimizedPrompt":"must-not-win"}"#.into(),
            finish_reason: Some("stop".into()),
            raw_json: None,
        });
        let result = run_literary(request, &mut completer).unwrap();
        assert_eq!(result.prompt, "成稿一句");
        assert!(!result.output_incomplete);
    }
}