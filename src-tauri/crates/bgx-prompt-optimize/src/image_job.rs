use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::error::{error_codes, OptimizeError};
use crate::skill_material::{validate_resource_paths, FrozenSkillBytes};
use crate::template::{self, FrozenRuleSet, Language};
use crate::text_port::TextMessage;

const CONTRACT: &str =
    include_str!("../../../../packages/app-contracts/v1/image-prompt-optimize.json");

pub const IMAGE_PROMPT_TASK: &str = "image-prompt-optimize";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub enum VisionChannel {
    #[default]
    ReferenceOnly,
    #[serde(rename = "asSubmitted")]
    AsSubmitted,
    Optional,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TextVisionCaps {
    pub supports_vision: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubmittedImage {
    pub bytes: Vec<u8>,
    pub declared_order: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImageOptimizeRequest {
    #[serde(default)]
    pub task_id: String,
    pub user_text: String,
    #[serde(default)]
    pub method: Option<FrozenSkillBytes>,
    pub template: FrozenSkillBytes,
    pub language: String,
    pub reference_image_count: u32,
    #[serde(default)]
    pub images: Vec<SubmittedImage>,
    pub text_vision: TextVisionCaps,
    #[serde(default)]
    pub channel: VisionChannel,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImageOptimizeResult {
    pub optimized_prompt: String,
    pub raw_reply: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
    #[serde(default)]
    pub output_incomplete: bool,
    pub attached_image_count: u32,
    #[serde(default)]
    pub vision_truncated: bool,
    #[serde(default)]
    pub vision_skip_reason: String,
    pub input_mode: String,
}

pub fn output_schema() -> Result<Value, OptimizeError> {
    let contract: Value = serde_json::from_str(CONTRACT).map_err(|err| {
        OptimizeError::new(
            error_codes::INTERNAL_PROMPT_CONFIG,
            format!("IMAGE_PROMPT_CONTRACT_INVALID: {err}"),
        )
    })?;
    Ok(contract["outputSchema"].clone())
}

pub fn method_resource_max_chars() -> Result<u64, OptimizeError> {
    let contract: Value = serde_json::from_str(CONTRACT).map_err(|err| {
        OptimizeError::new(
            error_codes::INTERNAL_PROMPT_CONFIG,
            format!("IMAGE_PROMPT_CONTRACT_INVALID: {err}"),
        )
    })?;
    contract["methodResourceMaxChars"].as_u64().ok_or_else(|| {
        OptimizeError::new(
            error_codes::INTERNAL_PROMPT_CONFIG,
            "IMAGE_PROMPT_CONTRACT_INVALID",
        )
    })
}

pub fn parse_optimized_prompt(raw: &str) -> Result<String, OptimizeError> {
    let value: Value = serde_json::from_str(raw).map_err(|err| {
        OptimizeError::new(
            error_codes::IMAGE_PROMPT_OUTPUT_INVALID,
            format!("IMAGE_PROMPT_OUTPUT_INVALID: {err}"),
        )
        .with_raw_reply(raw.to_owned())
    })?;
    let schema = output_schema().map_err(|err| {
        OptimizeError::new(error_codes::IMAGE_PROMPT_OUTPUT_INVALID, err.message)
            .with_raw_reply(raw.to_owned())
    })?;
    validate_output_schema(&schema, &value).map_err(|err| {
        OptimizeError::new(
            error_codes::IMAGE_PROMPT_OUTPUT_INVALID,
            format!("IMAGE_PROMPT_OUTPUT_INVALID: {err}"),
        )
        .with_raw_reply(raw.to_owned())
    })?;
    value["optimizedPrompt"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| {
            OptimizeError::new(
                error_codes::IMAGE_PROMPT_OUTPUT_INVALID,
                "IMAGE_PROMPT_OUTPUT_INVALID: optimizedPrompt must be a string",
            )
            .with_raw_reply(raw.to_owned())
        })
}

fn validate_output_schema(schema: &Value, value: &Value) -> Result<(), String> {
    match schema["type"].as_str() {
        Some("object") => {
            let obj = value
                .as_object()
                .ok_or_else(|| "expected a JSON object".to_string())?;
            if schema["additionalProperties"] == false {
                let allowed: Vec<&str> = schema["properties"]
                    .as_object()
                    .map(|props| props.keys().map(String::as_str).collect())
                    .unwrap_or_default();
                for key in obj.keys() {
                    if !allowed.contains(&key.as_str()) {
                        return Err(format!("additional property {key} is not allowed"));
                    }
                }
            }
            if let Some(required) = schema["required"].as_array() {
                for key in required {
                    let name = key.as_str().ok_or_else(|| "required key must be a string".to_string())?;
                    if !obj.contains_key(name) {
                        return Err(format!("missing required property {name}"));
                    }
                }
            }
            if let Some(props) = schema["properties"].as_object() {
                for (name, prop_schema) in props {
                    if let Some(item) = obj.get(name) {
                        validate_output_schema(prop_schema, item)?;
                    }
                }
            }
            Ok(())
        }
        Some("string") => {
            let text = value
                .as_str()
                .ok_or_else(|| "expected a string".to_string())?;
            if let Some(min) = schema["minLength"].as_u64() {
                if (text.chars().count() as u64) < min {
                    return Err(format!("string shorter than minLength {min}"));
                }
            }
            Ok(())
        }
        Some(other) => Err(format!("unsupported schema type {other}")),
        None => Err("schema type is required".into()),
    }
}

/// Compose the selectedMethod string from a frozen skill. Resource text is appended, never truncated.
///
/// Digest verification uses the full `SKILL.md` in `body`. The model-facing method text strips
/// YAML frontmatter when the body parses as a skill package; plain bodies stay unchanged.
pub fn compose_selected_method(method: &FrozenSkillBytes) -> Result<String, OptimizeError> {
    validate_resource_paths(method)?;
    crate::skill_material::verify_method_digest(method)?;
    let budget = method_resource_max_chars()?;
    let mut used = 0u64;
    let mut composed = match bgx_agent_skills_core::frontmatter::parse_skill_md(method.body.as_bytes())
        .parsed
    {
        Some(parsed) => parsed.body_for_model().to_string(),
        None => method.body.clone(),
    };
    for (path, bytes) in &method.resources {
        if path == "SKILL.md" {
            continue;
        }
        let text = String::from_utf8(bytes.clone()).map_err(|_| {
            OptimizeError::new(
                error_codes::INTERNAL_PROMPT_CONFIG,
                format!("INTERNAL_PROMPT_CONFIG: not-utf8: {path}"),
            )
        })?;
        let needed = text.chars().count() as u64;
        let remaining = budget.saturating_sub(used);
        if needed > remaining {
            return Err(OptimizeError::new(
                "PROJECTION_RESOURCE_BUDGET_EXCEEDED",
                format!(
                    "PROJECTION_RESOURCE_BUDGET_EXCEEDED: {path} needs {needed} characters but only {remaining} remain in this run's budget"
                ),
            ));
        }
        used += needed;
        composed.push_str(&format!("\n\n[Skill resource: {path}]\n{text}"));
    }
    Ok(composed)
}

pub struct AssembleVisionFacts {
    pub attached_image_count: u32,
    pub attached_image_order: Vec<Value>,
    pub images_read: bool,
}

pub fn load_template(request: &ImageOptimizeRequest) -> Result<FrozenRuleSet, OptimizeError> {
    let language = Language::parse(&request.language)?;
    let snapshot = template::unpack_rules(&request.template)?;
    if snapshot.language != language {
        return Err(template::config_error(
            "missing-language",
            format!(
                "template language {} does not match request {}",
                snapshot.language.key(),
                language.key()
            ),
        ));
    }
    if snapshot.sole_task().ok() != Some(IMAGE_PROMPT_TASK) {
        return Err(OptimizeError::new(
            error_codes::INTERNAL_PROMPT_CONFIG,
            "INTERNAL_PROMPT_CONFIG: frozen task does not match image prompt optimize",
        ));
    }
    Ok(snapshot)
}

pub fn assemble_messages(
    request: &ImageOptimizeRequest,
    facts: &AssembleVisionFacts,
) -> Result<Vec<TextMessage>, OptimizeError> {
    let snapshot = load_template(request)?;
    let user = request.user_text.trim();
    if user.is_empty() {
        return Err(OptimizeError::new(
            error_codes::IMAGE_PROMPT_USER_INPUT_REQUIRED,
            "IMAGE_PROMPT_USER_INPUT_REQUIRED",
        ));
    }
    let schema = output_schema()?;
    let selected = match &request.method {
        Some(method) => Value::String(compose_selected_method(method)?),
        None => Value::Null,
    };
    let input_mode = if facts.attached_image_count > 0 {
        "vision"
    } else {
        "text"
    };
    let context = json!({
        "userText": user,
        "selectedMethod": selected,
        "referenceImageCount": request.reference_image_count,
        "referenceImageOrder": (1..=request.reference_image_count).collect::<Vec<_>>(),
        "attachedImageCount": facts.attached_image_count,
        "inputMode": input_mode,
        "imagesRead": facts.images_read,
        "attachedImageOrder": facts.attached_image_order,
    });
    let rendered = template::render_request(
        &snapshot,
        IMAGE_PROMPT_TASK,
        &BTreeMap::from([
            ("taskContext".into(), context),
            ("outputContract".into(), schema),
        ]),
    )?;
    let mut messages = rendered
        .into_iter()
        .map(|message| TextMessage {
            role: message.role,
            content: Value::String(message.content),
        })
        .collect();
    crate::reference_guidance::apply(&mut messages, snapshot.language);
    Ok(messages)
}

impl FrozenRuleSet {
    fn sole_task(&self) -> Result<&str, OptimizeError> {
        if self.tasks.len() != 1 {
            return Err(template::config_error(
                "kind-mismatch",
                "frozen task does not match image prompt optimize",
            ));
        }
        Ok(self.tasks.keys().next().expect("len == 1").as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::template::{freeze_from_files, pack_rules, Language};

    const REQUEST_JSON: &str = include_str!(
        "../../bgx-internal-prompts/resources/internal-prompt-skills/image-prompt-optimize/request.json"
    );
    const SKILL_ZH: &str = include_str!(
        "../../bgx-internal-prompts/resources/internal-prompt-skills/image-prompt-optimize/SKILL.md"
    );
    const METHOD_EN: &str = include_str!(
        "../../bgx-internal-prompts/resources/internal-prompt-skills/image-prompt-optimize/prompts/method.en.md"
    );
    const GUIDANCE_ZH: &str = include_str!(
        "../../bgx-internal-prompts/resources/internal-prompt-skills/shared/request-guidance.zh.md"
    );
    const GUIDANCE_EN: &str = include_str!(
        "../../bgx-internal-prompts/resources/internal-prompt-skills/shared/request-guidance.en.md"
    );
    const CONTEXT_ZH: &str = include_str!(
        "../../bgx-internal-prompts/resources/internal-prompt-skills/shared/context-heading.zh.md"
    );
    const CONTEXT_EN: &str = include_str!(
        "../../bgx-internal-prompts/resources/internal-prompt-skills/shared/context-heading.en.md"
    );
    const OUTPUT_ZH: &str = include_str!(
        "../../bgx-internal-prompts/resources/internal-prompt-skills/shared/output-heading.zh.md"
    );
    const OUTPUT_EN: &str = include_str!(
        "../../bgx-internal-prompts/resources/internal-prompt-skills/shared/output-heading.en.md"
    );

    fn authored_files() -> BTreeMap<String, String> {
        BTreeMap::from([
            (
                "image-prompt-optimize/request.json".into(),
                REQUEST_JSON.to_owned(),
            ),
            ("image-prompt-optimize/SKILL.md".into(), SKILL_ZH.to_owned()),
            (
                "image-prompt-optimize/prompts/method.en.md".into(),
                METHOD_EN.to_owned(),
            ),
            ("shared/request-guidance.zh.md".into(), GUIDANCE_ZH.to_owned()),
            ("shared/request-guidance.en.md".into(), GUIDANCE_EN.to_owned()),
            ("shared/context-heading.zh.md".into(), CONTEXT_ZH.to_owned()),
            ("shared/context-heading.en.md".into(), CONTEXT_EN.to_owned()),
            ("shared/output-heading.zh.md".into(), OUTPUT_ZH.to_owned()),
            ("shared/output-heading.en.md".into(), OUTPUT_EN.to_owned()),
        ])
    }

    fn request(language: Language, user: &str, method: Option<&str>) -> ImageOptimizeRequest {
        let snapshot = freeze_from_files(language, &[IMAGE_PROMPT_TASK], &authored_files()).unwrap();
        ImageOptimizeRequest {
            task_id: String::new(),
            user_text: user.into(),
            method: method.map(|body| FrozenSkillBytes {
                stable_id: "user-method".into(),
                package_digest: bgx_agent_skills_core::digest::package_digest([("SKILL.md", body.as_bytes())]),
                body: body.into(),
                resources: BTreeMap::new(),
            }),
            template: pack_rules(&snapshot).unwrap(),
            language: language.key().into(),
            reference_image_count: 0,
            images: Vec::new(),
            text_vision: TextVisionCaps {
                supports_vision: true,
            },
            channel: VisionChannel::AsSubmitted,
        }
    }

    fn facts(attached: u32, order: Vec<Value>, images_read: bool) -> AssembleVisionFacts {
        AssembleVisionFacts {
            attached_image_count: attached,
            attached_image_order: order,
            images_read,
        }
    }

    #[test]
    fn single_object_contract_is_shared_by_prompt_and_acceptance() {
        let valid = r#"{"optimizedPrompt":"A red umbrella beside a quiet bus stop."}"#;
        assert_eq!(
            parse_optimized_prompt(valid).unwrap(),
            "A red umbrella beside a quiet bus stop."
        );
        for invalid in [
            r#"{}"#,
            r#"{"optimizedPrompt":""}"#,
            r#"{"optimizedPrompt":12}"#,
            r#"{"optimizedPrompt":"x","aspect_ratio":"1:1"}"#,
            r#"["x"]"#,
        ] {
            let err = parse_optimized_prompt(invalid).unwrap_err();
            assert_eq!(err.code, error_codes::IMAGE_PROMPT_OUTPUT_INVALID);
            assert_eq!(err.raw_reply.as_deref(), Some(invalid));
        }
        let req = request(Language::En, "A red umbrella", Some("Write a complete image prompt."));
        let messages = assemble_messages(&req, &facts(0, vec![], false)).unwrap();
        assert_eq!(messages.len(), 2);
        let schema = output_schema().unwrap();
        let user = messages[1].content.as_str().unwrap();
        assert!(user.contains(&crate::template::stable_json(&schema).unwrap()));
        assert!(messages[0]
            .content
            .as_str()
            .unwrap()
            .contains("Edit the current image prompt"));
        assert!(user.contains("Write a complete image prompt."));
    }

    #[test]
    fn text_mode_states_unread_references_and_keeps_the_selected_method() {
        let source = "图1 的人物换成红衣，背景保留图2\n她说：\"不要替换\"【原句】";
        let mut req = request(Language::Zh, source, Some("用户选定的摄影方法。"));
        req.reference_image_count = 2;
        let text = assemble_messages(&req, &facts(0, vec![], false)).unwrap();
        let user = text[1].content.as_str().unwrap();
        assert!(user.contains("用户选定的摄影方法。"));
        assert!(user.contains("\"referenceImageCount\":2"));
        assert!(user.contains("\"attachedImageCount\":0"));
        assert!(user.contains("\"inputMode\":\"text\""));
        assert!(user.contains("\"imagesRead\":false"));
        assert!(user.contains("图1 的人物换成红衣"));
        assert!(user.contains("\\\"不要替换\\\""));
        assert!(text[0]
            .content
            .as_str()
            .unwrap()
            .contains("以当前图片提示词为基准"));
        assert!(!text[0].content.as_str().unwrap().contains("SELECTED_SKILL"));
        let vision = assemble_messages(&req, &facts(2, vec![json!(1), json!(2)], true)).unwrap();
        let vision_user = vision[1].content.as_str().unwrap();
        assert!(vision_user.contains("\"inputMode\":\"vision\""));
        assert!(vision_user.contains("\"attachedImageCount\":2"));
        assert!(vision_user.contains("\"imagesRead\":true"));
        let mut none = request(Language::Zh, source, None);
        none.reference_image_count = 0;
        let none_msgs = assemble_messages(&none, &facts(0, vec![], false)).unwrap();
        assert!(none_msgs[1]
            .content
            .as_str()
            .unwrap()
            .contains("\"selectedMethod\":null"));
        assert!(none_msgs[1]
            .content
            .as_str()
            .unwrap()
            .contains("\"referenceImageCount\":0"));
    }

    #[test]
    fn empty_user_text_is_required() {
        let req = request(Language::Zh, "   ", None);
        let err = assemble_messages(&req, &facts(0, vec![], false)).unwrap_err();
        assert_eq!(err.code, error_codes::IMAGE_PROMPT_USER_INPUT_REQUIRED);
    }

    #[test]
    fn method_resources_are_refused_without_truncation_when_over_budget() {
        let over = "汉".repeat((method_resource_max_chars().unwrap() as usize) + 8);
        let skill_md: &[u8] = b"body";
        let notes: &[u8] = over.as_bytes();
        // package_digest takes IntoIterator<Item = (&str, &[u8])>.
        let files: [(&str, &[u8]); 2] = [("SKILL.md", skill_md), ("notes.md", notes)];
        let method = FrozenSkillBytes {
            stable_id: "user-method".into(),
            package_digest: bgx_agent_skills_core::digest::package_digest(files),
            body: "body".into(),
            resources: BTreeMap::from([
                ("SKILL.md".into(), skill_md.to_vec()),
                ("notes.md".into(), notes.to_vec()),
            ]),
        };
        let err = compose_selected_method(&method).unwrap_err();
        assert_eq!(err.code, "PROJECTION_RESOURCE_BUDGET_EXCEEDED");
        assert!(err.message.contains("notes.md"));
        assert!(!err.message.contains(&over));
    }

    #[test]
    fn compose_selected_method_strips_frontmatter_but_keeps_plain_bodies() {
        let skill_md = "---\nname: photography\ndescription: Camera method.\n---\nWrite a complete image prompt.\n";
        let notes = b"# Extra notes\n";
        let method = FrozenSkillBytes {
            stable_id: "user-method".into(),
            package_digest: bgx_agent_skills_core::digest::package_digest([
                ("SKILL.md", skill_md.as_bytes()),
                ("notes.md", notes.as_slice()),
            ]),
            body: skill_md.into(),
            resources: BTreeMap::from([("notes.md".into(), notes.to_vec())]),
        };
        let composed = compose_selected_method(&method).unwrap();
        assert!(composed.starts_with("Write a complete image prompt."));
        assert!(!composed.contains("---"));
        assert!(!composed.contains("name: photography"));
        assert!(composed.contains("[Skill resource: notes.md]"));
        assert!(composed.contains("# Extra notes"));

        let plain = FrozenSkillBytes {
            stable_id: "plain".into(),
            package_digest: bgx_agent_skills_core::digest::package_digest([(
                "SKILL.md",
                b"Write a complete image prompt.".as_slice(),
            )]),
            body: "Write a complete image prompt.".into(),
            resources: BTreeMap::new(),
        };
        assert_eq!(
            compose_selected_method(&plain).unwrap(),
            "Write a complete image prompt."
        );
    }
}