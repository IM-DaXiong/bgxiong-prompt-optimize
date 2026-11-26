use serde::Deserialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

use crate::error::PromptError;
use crate::source::normalize_resource;
use crate::types::{Language, RenderedMessage, TaskKind};

#[derive(Debug)]
pub struct Declaration {
    pub task: String,
    pub kind: TaskKind,
    pub messages: Vec<MessageDecl>,
    pub materials: Vec<SlotDecl>,
}

#[derive(Debug)]
pub struct MessageDecl {
    pub id: String,
    pub role: String,
    pub parts: Vec<PartDecl>,
}

#[derive(Debug)]
pub struct SlotDecl {
    pub slot: String,
    pub parts: Vec<PartDecl>,
}

#[derive(Debug)]
pub enum PartDecl {
    File(BTreeMap<String, String>),
    Input { name: String, encoding: Encoding },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Encoding {
    Json,
    Text,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DeclarationFile {
    format_version: u32,
    task: String,
    kind: String,
    messages: Option<Vec<MessageFile>>,
    materials: Option<Vec<SlotFile>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MessageFile {
    id: String,
    role: String,
    parts: Vec<PartFile>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SlotFile {
    slot: String,
    parts: Vec<PartFile>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum PartFile {
    File(FilePart),
    Input(InputPart),
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FilePart {
    file_by_language: BTreeMap<String, String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InputPart {
    input: String,
    encoding: Encoding,
}

pub fn parse_declaration(raw: &str) -> Result<Declaration, PromptError> {
    let file: DeclarationFile = serde_json::from_str(raw)
        .map_err(|_| PromptError::new("declaration-rejected", "declaration JSON is not version 2"))?;
    if file.format_version != 2 {
        return Err(PromptError::new(
            "unsupported-version",
            "only formatVersion 2 is accepted",
        ));
    }
    ident("task", &file.task)?;
    let kind = match file.kind.as_str() {
        "request" => TaskKind::Request,
        "material" => TaskKind::Material,
        _ => return Err(PromptError::new("kind-rejected", file.kind)),
    };
    match kind {
        TaskKind::Request => {
            if file.materials.is_some() {
                return Err(PromptError::new(
                    "kind-mismatch",
                    "a request package cannot declare materials",
                ));
            }
            let messages = file.messages.ok_or_else(|| {
                PromptError::new("kind-mismatch", "a request package needs messages")
            })?;
            if messages.is_empty() {
                return Err(PromptError::new("empty-messages", file.task));
            }
            let mut seen = BTreeSet::new();
            let mut parsed = Vec::new();
            for message in messages {
                ident("message", &message.id)?;
                if !seen.insert(message.id.clone()) {
                    return Err(PromptError::new("duplicate-message", message.id));
                }
                if !matches!(message.role.as_str(), "system" | "user" | "assistant") {
                    return Err(PromptError::new("role-rejected", message.role));
                }
                parsed.push(MessageDecl {
                    id: message.id,
                    role: message.role,
                    parts: parse_parts(&file.task, message.parts)?,
                });
            }
            Ok(Declaration {
                task: file.task,
                kind,
                messages: parsed,
                materials: Vec::new(),
            })
        }
        TaskKind::Material => {
            if file.messages.is_some() {
                return Err(PromptError::new(
                    "kind-mismatch",
                    "a material package cannot declare messages",
                ));
            }
            let materials = file.materials.ok_or_else(|| {
                PromptError::new("kind-mismatch", "a material package needs materials")
            })?;
            if materials.is_empty() {
                return Err(PromptError::new("empty-materials", file.task));
            }
            let mut seen = BTreeSet::new();
            let mut parsed = Vec::new();
            for material in materials {
                slot_name(&material.slot)?;
                if !seen.insert(material.slot.clone()) {
                    return Err(PromptError::new("duplicate-slot", material.slot));
                }
                parsed.push(SlotDecl {
                    slot: material.slot,
                    parts: parse_parts(&file.task, material.parts)?,
                });
            }
            Ok(Declaration {
                task: file.task,
                kind,
                messages: Vec::new(),
                materials: parsed,
            })
        }
    }
}

fn parse_parts(task: &str, parts: Vec<PartFile>) -> Result<Vec<PartDecl>, PromptError> {
    if parts.is_empty() {
        return Err(PromptError::new("empty-parts", task));
    }
    parts
        .into_iter()
        .map(|part| match part {
            PartFile::File(file) => {
                if file.file_by_language.len() != 2
                    || !file.file_by_language.contains_key("zh")
                    || !file.file_by_language.contains_key("en")
                {
                    return Err(PromptError::new(
                        "missing-language",
                        "a file part must declare zh and en",
                    ));
                }
                for path in file.file_by_language.values() {
                    normalize_resource(task, path)?;
                }
                Ok(PartDecl::File(file.file_by_language))
            }
            PartFile::Input(input) => {
                if input.input.is_empty()
                    || !input
                        .input
                        .chars()
                        .next()
                        .is_some_and(|char| char.is_ascii_alphabetic())
                    || !input.input.chars().all(|char| char.is_ascii_alphanumeric())
                {
                    return Err(PromptError::new("input-rejected", input.input));
                }
                Ok(PartDecl::Input {
                    name: input.input,
                    encoding: input.encoding,
                })
            }
        })
        .collect()
}

pub fn files_for(declaration: &Declaration, language: Option<Language>) -> Result<Vec<String>, PromptError> {
    let mut names = Vec::new();
    let parts = declaration
        .messages
        .iter()
        .flat_map(|message| message.parts.iter())
        .chain(declaration.materials.iter().flat_map(|slot| slot.parts.iter()));
    for part in parts {
        if let PartDecl::File(files) = part {
            let selected: Vec<&str> = match language {
                Some(language) => vec![language.key()],
                None => vec!["zh", "en"],
            };
            for key in selected {
                let raw = files
                    .get(key)
                    .ok_or_else(|| PromptError::new("missing-language", key))?;
                names.push(normalize_resource(&declaration.task, raw)?);
            }
        }
    }
    Ok(names)
}

pub fn render_parts(
    task: &str,
    language: Language,
    parts: &[PartDecl],
    files: &BTreeMap<String, String>,
    inputs: &BTreeMap<String, Value>,
) -> Result<String, PromptError> {
    let mut content = String::new();
    for part in parts {
        match part {
            PartDecl::File(by_language) => {
                let raw = by_language
                    .get(language.key())
                    .ok_or_else(|| PromptError::new("missing-language", language.key()))?;
                let name = normalize_resource(task, raw)?;
                let text = files
                    .get(&name)
                    .ok_or_else(|| PromptError::new("missing-resource", name.clone()))?;
                content.push_str(&insert_file(&name, text)?);
            }
            PartDecl::Input { name, encoding } => {
                let value = inputs
                    .get(name)
                    .ok_or_else(|| PromptError::new("missing-input", name.clone()))?;
                match encoding {
                    Encoding::Json => content.push_str(&stable_json(value)?),
                    Encoding::Text => {
                        let text = value
                            .as_str()
                            .ok_or_else(|| PromptError::new("input-not-text", name.clone()))?;
                        content.push_str(text);
                    }
                }
            }
        }
    }
    Ok(content)
}

pub fn render_request(
    declaration: &Declaration,
    language: Language,
    files: &BTreeMap<String, String>,
    inputs: &BTreeMap<String, Value>,
) -> Result<Vec<RenderedMessage>, PromptError> {
    if declaration.kind != TaskKind::Request {
        return Err(PromptError::new(
            "kind-mismatch",
            format!("{} is not a request", declaration.task),
        ));
    }
    declaration
        .messages
        .iter()
        .map(|message| {
            Ok(RenderedMessage {
                id: message.id.clone(),
                role: message.role.clone(),
                content: render_parts(
                    &declaration.task,
                    language,
                    &message.parts,
                    files,
                    inputs,
                )?,
            })
        })
        .collect()
}

pub fn render_material(
    declaration: &Declaration,
    language: Language,
    slot: &str,
    files: &BTreeMap<String, String>,
    inputs: &BTreeMap<String, Value>,
) -> Result<String, PromptError> {
    if declaration.kind != TaskKind::Material {
        return Err(PromptError::new(
            "kind-mismatch",
            format!("{} is not a material", declaration.task),
        ));
    }
    let material = declaration
        .materials
        .iter()
        .find(|item| item.slot == slot)
        .ok_or_else(|| PromptError::new("missing-slot", slot))?;
    render_parts(&declaration.task, language, &material.parts, files, inputs)
}

fn insert_file(name: &str, raw: &str) -> Result<String, PromptError> {
    let file_name = name.rsplit('/').next().unwrap_or(name);
    if file_name != "SKILL.md" {
        return Ok(raw.to_owned());
    }
    let parsed = bgx_agent_skills_core::frontmatter::parse_skill_md(raw.as_bytes())
        .parsed
        .ok_or_else(|| PromptError::new("invalid-skill", name))?;
    Ok(parsed.body_raw.to_owned())
}

/// Method body historically read from the material `method` slot.
/// Request packages keep the same body as the system-message file that
/// selects `SKILL.md` (zh) or `prompts/method.<lang>.md`.
pub fn render_method_body(
    declaration: &Declaration,
    language: Language,
    files: &BTreeMap<String, String>,
) -> Result<String, PromptError> {
    match declaration.kind {
        TaskKind::Material => {
            render_material(declaration, language, "method", files, &BTreeMap::new())
        }
        TaskKind::Request => render_request_method(declaration, language, files),
    }
}

fn render_request_method(
    declaration: &Declaration,
    language: Language,
    files: &BTreeMap<String, String>,
) -> Result<String, PromptError> {
    let mut found = None;
    for message in &declaration.messages {
        if message.role != "system" {
            continue;
        }
        for part in &message.parts {
            let PartDecl::File(by_language) = part else {
                continue;
            };
            let raw = by_language
                .get(language.key())
                .ok_or_else(|| PromptError::new("missing-language", language.key()))?;
            let name = normalize_resource(&declaration.task, raw)?;
            let file_name = name.rsplit('/').next().unwrap_or(name.as_str());
            let is_method = file_name == "SKILL.md"
                || (file_name.starts_with("method.") && file_name.ends_with(".md"));
            if !is_method {
                continue;
            }
            if found.is_some() {
                return Err(PromptError::new(
                    "duplicate-slot",
                    format!("{} method", declaration.task),
                ));
            }
            let text = files
                .get(&name)
                .ok_or_else(|| PromptError::new("missing-resource", name.clone()))?;
            found = Some(insert_file(&name, text)?);
        }
    }
    found.ok_or_else(|| PromptError::new("missing-slot", "method"))
}

fn stable_json(value: &Value) -> Result<String, PromptError> {
    fn sort(value: &Value) -> Value {
        match value {
            Value::Object(map) => {
                let mut keys: Vec<_> = map.keys().cloned().collect();
                keys.sort();
                let mut ordered = serde_json::Map::new();
                for key in keys {
                    ordered.insert(key.clone(), sort(&map[&key]));
                }
                Value::Object(ordered)
            }
            Value::Array(items) => Value::Array(items.iter().map(sort).collect()),
            other => other.clone(),
        }
    }
    serde_json::to_string(&sort(value))
        .map_err(|_| PromptError::new("input-not-json", "input could not be serialized"))
}

fn ident(kind: &str, value: &str) -> Result<(), PromptError> {
    let bytes = value.as_bytes();
    if bytes.is_empty()
        || bytes.first() == Some(&b'-')
        || bytes.last() == Some(&b'-')
        || !bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
    {
        return Err(PromptError::new("id-rejected", format!("{kind} {value}")));
    }
    Ok(())
}

fn slot_name(value: &str) -> Result<(), PromptError> {
    let mut chars = value.chars();
    if !chars.next().is_some_and(|char| char.is_ascii_alphabetic())
        || !chars.all(|char| char.is_ascii_alphanumeric())
    {
        return Err(PromptError::new("slot-rejected", value));
    }
    Ok(())
}
