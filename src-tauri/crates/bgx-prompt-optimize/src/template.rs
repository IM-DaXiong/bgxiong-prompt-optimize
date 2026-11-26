//! Internal prompt package render, ported from `bgx-internal-prompts` package/snapshot.
//! Input is already-read request.json and language-selected file text; no Path in the core.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::error::{error_codes, OptimizeError};
use crate::skill_material::FrozenSkillBytes;

pub const FORMAT_VERSION: u32 = 2;

const MAX_FILES: usize = 512;
const MAX_TOTAL_BYTES: u64 = 1_048_576;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Zh,
    En,
}

impl Language {
    pub fn key(self) -> &'static str {
        match self {
            Self::Zh => "zh",
            Self::En => "en",
        }
    }

    pub fn parse(value: &str) -> Result<Self, OptimizeError> {
        match value {
            "zh" => Ok(Self::Zh),
            "en" => Ok(Self::En),
            _ => Err(config_error("missing-language", value)),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskKind {
    Request,
    Material,
}

impl TaskKind {
    pub fn key(self) -> &'static str {
        match self {
            Self::Request => "request",
            Self::Material => "material",
        }
    }
}

/// Complete rule bytes for one language. Restore does not take a directory.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FrozenRuleSet {
    pub format_version: u32,
    pub language: Language,
    pub tasks: BTreeMap<String, TaskKind>,
    pub files: BTreeMap<String, String>,
    pub digest: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RenderedMessage {
    pub id: String,
    pub role: String,
    pub content: String,
}

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
pub enum Encoding {
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DigestBody<'a> {
    format_version: u32,
    language: &'a str,
    tasks: &'a BTreeMap<String, TaskKind>,
    files: &'a BTreeMap<String, String>,
}

pub fn config_error(reason: &str, detail: impl Into<String>) -> OptimizeError {
    let detail = detail.into();
    OptimizeError::new(
        error_codes::INTERNAL_PROMPT_CONFIG,
        format!("INTERNAL_PROMPT_CONFIG: {reason}: {detail}"),
    )
}

/// `raw` is relative to the task directory and uses `/` only.
pub fn normalize_resource(task: &str, raw: &str) -> Result<String, OptimizeError> {
    if raw.is_empty() || raw.contains('\\') || raw.contains(':') || raw.starts_with('/') {
        return Err(config_error("path-rejected", raw));
    }
    let mut parts = vec![task.to_owned()];
    for segment in raw.split('/') {
        match segment {
            "" => return Err(config_error("path-rejected", raw)),
            "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err(config_error("path-escape", raw));
                }
            }
            _ => parts.push(segment.to_owned()),
        }
    }
    if parts.is_empty() || parts.iter().any(|part| part.is_empty()) {
        return Err(config_error("path-escape", raw));
    }
    Ok(parts.join("/"))
}

pub fn parse_declaration(raw: &str) -> Result<Declaration, OptimizeError> {
    let file: DeclarationFile = serde_json::from_str(raw)
        .map_err(|_| config_error("declaration-rejected", "declaration JSON is not version 2"))?;
    if file.format_version != 2 {
        return Err(config_error(
            "unsupported-version",
            "only formatVersion 2 is accepted",
        ));
    }
    ident("task", &file.task)?;
    let kind = match file.kind.as_str() {
        "request" => TaskKind::Request,
        "material" => TaskKind::Material,
        _ => return Err(config_error("kind-rejected", file.kind)),
    };
    match kind {
        TaskKind::Request => {
            if file.materials.is_some() {
                return Err(config_error(
                    "kind-mismatch",
                    "a request package cannot declare materials",
                ));
            }
            let messages = file.messages.ok_or_else(|| {
                config_error("kind-mismatch", "a request package needs messages")
            })?;
            if messages.is_empty() {
                return Err(config_error("empty-messages", file.task));
            }
            let mut seen = BTreeSet::new();
            let mut parsed = Vec::new();
            for message in messages {
                ident("message", &message.id)?;
                if !seen.insert(message.id.clone()) {
                    return Err(config_error("duplicate-message", message.id));
                }
                if !matches!(message.role.as_str(), "system" | "user" | "assistant") {
                    return Err(config_error("role-rejected", message.role));
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
                return Err(config_error(
                    "kind-mismatch",
                    "a material package cannot declare messages",
                ));
            }
            let materials = file.materials.ok_or_else(|| {
                config_error("kind-mismatch", "a material package needs materials")
            })?;
            if materials.is_empty() {
                return Err(config_error("empty-materials", file.task));
            }
            let mut seen = BTreeSet::new();
            let mut parsed = Vec::new();
            for material in materials {
                slot_name(&material.slot)?;
                if !seen.insert(material.slot.clone()) {
                    return Err(config_error("duplicate-slot", material.slot));
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

fn parse_parts(task: &str, parts: Vec<PartFile>) -> Result<Vec<PartDecl>, OptimizeError> {
    if parts.is_empty() {
        return Err(config_error("empty-parts", task));
    }
    parts
        .into_iter()
        .map(|part| match part {
            PartFile::File(file) => {
                if file.file_by_language.len() != 2
                    || !file.file_by_language.contains_key("zh")
                    || !file.file_by_language.contains_key("en")
                {
                    return Err(config_error(
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
                        .is_some_and(|ch| ch.is_ascii_alphabetic())
                    || !input.input.chars().all(|ch| ch.is_ascii_alphanumeric())
                {
                    return Err(config_error("input-rejected", input.input));
                }
                Ok(PartDecl::Input {
                    name: input.input,
                    encoding: input.encoding,
                })
            }
        })
        .collect()
}

pub fn files_for(
    declaration: &Declaration,
    language: Option<Language>,
) -> Result<Vec<String>, OptimizeError> {
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
                    .ok_or_else(|| config_error("missing-language", key))?;
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
) -> Result<String, OptimizeError> {
    let mut content = String::new();
    for part in parts {
        match part {
            PartDecl::File(by_language) => {
                let raw = by_language
                    .get(language.key())
                    .ok_or_else(|| config_error("missing-language", language.key()))?;
                let name = normalize_resource(task, raw)?;
                let text = files
                    .get(&name)
                    .ok_or_else(|| config_error("missing-resource", name.clone()))?;
                content.push_str(&insert_file(&name, text)?);
            }
            PartDecl::Input { name, encoding } => {
                let value = inputs
                    .get(name)
                    .ok_or_else(|| config_error("missing-input", name.clone()))?;
                match encoding {
                    Encoding::Json => content.push_str(&stable_json(value)?),
                    Encoding::Text => {
                        let text = value
                            .as_str()
                            .ok_or_else(|| config_error("input-not-text", name.clone()))?;
                        content.push_str(text);
                    }
                }
            }
        }
    }
    Ok(content)
}

pub fn render_declaration_request(
    declaration: &Declaration,
    language: Language,
    files: &BTreeMap<String, String>,
    inputs: &BTreeMap<String, Value>,
) -> Result<Vec<RenderedMessage>, OptimizeError> {
    if declaration.kind != TaskKind::Request {
        return Err(config_error(
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

pub fn render_declaration_material(
    declaration: &Declaration,
    language: Language,
    slot: &str,
    files: &BTreeMap<String, String>,
    inputs: &BTreeMap<String, Value>,
) -> Result<String, OptimizeError> {
    if declaration.kind != TaskKind::Material {
        return Err(config_error(
            "kind-mismatch",
            format!("{} is not a material", declaration.task),
        ));
    }
    let material = declaration
        .materials
        .iter()
        .find(|item| item.slot == slot)
        .ok_or_else(|| config_error("missing-slot", slot))?;
    render_parts(&declaration.task, language, &material.parts, files, inputs)
}

fn insert_file(name: &str, raw: &str) -> Result<String, OptimizeError> {
    let file_name = name.rsplit('/').next().unwrap_or(name);
    if file_name != "SKILL.md" {
        return Ok(raw.to_owned());
    }
    let parsed = bgx_agent_skills_core::frontmatter::parse_skill_md(raw.as_bytes())
        .parsed
        .ok_or_else(|| config_error("invalid-skill", name))?;
    Ok(parsed.body_raw.to_owned())
}

pub(crate) fn stable_json(value: &Value) -> Result<String, OptimizeError> {
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
        .map_err(|_| config_error("input-not-json", "input could not be serialized"))
}

fn ident(kind: &str, value: &str) -> Result<(), OptimizeError> {
    let bytes = value.as_bytes();
    if bytes.is_empty()
        || bytes.first() == Some(&b'-')
        || bytes.last() == Some(&b'-')
        || !bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
    {
        return Err(config_error("id-rejected", format!("{kind} {value}")));
    }
    Ok(())
}

fn slot_name(value: &str) -> Result<(), OptimizeError> {
    let mut chars = value.chars();
    if !chars.next().is_some_and(|ch| ch.is_ascii_alphabetic())
        || !chars.all(|ch| ch.is_ascii_alphanumeric())
    {
        return Err(config_error("slot-rejected", value));
    }
    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for b in digest {
        out.push(char::from_digit((b >> 4) as u32, 16).unwrap_or('0'));
        out.push(char::from_digit((b & 0x0f) as u32, 16).unwrap_or('0'));
    }
    out
}

fn content_digest(snapshot: &FrozenRuleSet) -> Result<String, OptimizeError> {
    let body = DigestBody {
        format_version: snapshot.format_version,
        language: snapshot.language.key(),
        tasks: &snapshot.tasks,
        files: &snapshot.files,
    };
    let bytes = serde_json::to_vec(&body)
        .map_err(|_| config_error("digest-rejected", "frozen rules could not be hashed"))?;
    Ok(format!("sha256:{}", sha256_hex(&bytes)))
}

fn enforce_budget(files: &BTreeMap<String, String>) -> Result<(), OptimizeError> {
    if files.len() > MAX_FILES {
        return Err(config_error("too-many-files", files.len().to_string()));
    }
    let total: u64 = files.values().map(|text| text.len() as u64).sum();
    if total > MAX_TOTAL_BYTES {
        return Err(config_error("closure-too-large", total.to_string()));
    }
    Ok(())
}

fn declaration_of(snapshot: &FrozenRuleSet, task: &str) -> Result<Declaration, OptimizeError> {
    if !snapshot.tasks.contains_key(task) {
        return Err(config_error("unknown-task", task));
    }
    let name = format!("{task}/request.json");
    let raw = snapshot
        .files
        .get(&name)
        .ok_or_else(|| config_error("missing-resource", name.clone()))?;
    let declaration = parse_declaration(raw)?;
    if declaration.task != task || snapshot.tasks.get(task) != Some(&declaration.kind) {
        return Err(config_error("task-mismatch", name));
    }
    Ok(declaration)
}

pub fn verify_set(snapshot: &FrozenRuleSet) -> Result<(), OptimizeError> {
    if snapshot.format_version != FORMAT_VERSION {
        return Err(config_error(
            "unsupported-version",
            "only formatVersion 2 is accepted",
        ));
    }
    if snapshot.tasks.is_empty() {
        return Err(config_error("empty-task-set", "frozen rules have no tasks"));
    }
    if content_digest(snapshot)? != snapshot.digest {
        return Err(config_error(
            "digest-mismatch",
            "frozen rule identity does not match its bytes",
        ));
    }
    enforce_budget(&snapshot.files)?;
    let mut expected = BTreeSet::new();
    for (task, kind) in &snapshot.tasks {
        let name = format!("{task}/request.json");
        let raw = snapshot
            .files
            .get(&name)
            .ok_or_else(|| config_error("missing-resource", name.clone()))?;
        let declaration = parse_declaration(raw)?;
        if declaration.task != *task || declaration.kind != *kind {
            return Err(config_error("task-mismatch", name));
        }
        expected.insert(name);
        for file in files_for(&declaration, Some(snapshot.language))? {
            if !snapshot.files.contains_key(&file) {
                return Err(config_error("missing-resource", file));
            }
            expected.insert(file);
        }
    }
    for name in snapshot.files.keys() {
        if !expected.contains(name) {
            return Err(config_error("unexpected-resource", name.clone()));
        }
    }
    Ok(())
}

pub fn restore(snapshot: FrozenRuleSet) -> Result<FrozenRuleSet, OptimizeError> {
    verify_set(&snapshot)?;
    Ok(snapshot)
}

pub fn restore_value(value: &Value) -> Result<FrozenRuleSet, OptimizeError> {
    let snapshot: FrozenRuleSet = serde_json::from_value(value.clone())
        .map_err(|_| config_error("snapshot-rejected", "frozen rules are not version 2"))?;
    restore(snapshot)
}

/// Freeze already-read files. Keys are package-relative paths such as `episode/request.json`.
pub fn freeze_from_files(
    language: Language,
    task_ids: &[&str],
    available: &BTreeMap<String, String>,
) -> Result<FrozenRuleSet, OptimizeError> {
    if task_ids.is_empty() {
        return Err(config_error("empty-task-set", "freeze_set needs at least one task"));
    }
    let mut ordered = BTreeSet::new();
    for task in task_ids {
        if !ordered.insert((*task).to_owned()) {
            return Err(config_error("duplicate-task", *task));
        }
    }
    let mut tasks = BTreeMap::new();
    let mut files = BTreeMap::new();
    for task in &ordered {
        let declaration_name = format!("{task}/request.json");
        let raw = available
            .get(&declaration_name)
            .ok_or_else(|| config_error("missing-resource", declaration_name.clone()))?;
        let declaration = parse_declaration(raw)?;
        if declaration.task != *task {
            return Err(config_error("task-mismatch", declaration_name));
        }
        tasks.insert(task.clone(), declaration.kind);
        files.insert(declaration_name, raw.clone());
        for name in files_for(&declaration, Some(language))? {
            if !files.contains_key(&name) {
                let text = available
                    .get(&name)
                    .ok_or_else(|| config_error("missing-resource", name.clone()))?;
                files.insert(name, text.clone());
            }
        }
    }
    enforce_budget(&files)?;
    let mut snapshot = FrozenRuleSet {
        format_version: FORMAT_VERSION,
        language,
        tasks,
        files,
        digest: String::new(),
    };
    snapshot.digest = content_digest(&snapshot)?;
    verify_set(&snapshot)?;
    Ok(snapshot)
}

/// Frozen public render entry. `files` is the already language-selected package map.
/// This wraps the existing T03 engine and does not replace it.
pub fn render(
    request_json: &str,
    files: &BTreeMap<String, String>,
    task: &str,
    inputs: &BTreeMap<String, serde_json::Value>,
) -> Result<Vec<RenderedMessage>, OptimizeError> {
    let mut available = files.clone();
    available.insert(format!("{task}/request.json"), request_json.to_string());
    let mut last_err = None;
    for language in [Language::Zh, Language::En] {
        match freeze_from_files(language, &[task], &available) {
            Ok(snapshot) => return render_request(&snapshot, task, inputs),
            Err(err) => last_err = Some(err),
        }
    }
    Err(last_err.unwrap_or_else(|| config_error("missing-resource", task)))
}

pub fn render_request(
    snapshot: &FrozenRuleSet,
    task: &str,
    inputs: &BTreeMap<String, Value>,
) -> Result<Vec<RenderedMessage>, OptimizeError> {
    verify_set(snapshot)?;
    let declaration = declaration_of(snapshot, task)?;
    render_declaration_request(&declaration, snapshot.language, &snapshot.files, inputs)
}

pub fn render_material(
    snapshot: &FrozenRuleSet,
    task: &str,
    slot: &str,
    inputs: &BTreeMap<String, Value>,
) -> Result<String, OptimizeError> {
    verify_set(snapshot)?;
    let declaration = declaration_of(snapshot, task)?;
    render_declaration_material(
        &declaration,
        snapshot.language,
        slot,
        &snapshot.files,
        inputs,
    )
}

/// Pack a verified rule set into the frozen skill envelope used by core requests.
/// `body` is the FrozenRuleSet JSON; `package_digest` is the rule digest.
pub fn pack_rules(snapshot: &FrozenRuleSet) -> Result<FrozenSkillBytes, OptimizeError> {
    verify_set(snapshot)?;
    let body = serde_json::to_string(snapshot)
        .map_err(|_| config_error("snapshot-rejected", "frozen rules could not be serialized"))?;
    let stable_id = snapshot.tasks.keys().cloned().collect::<Vec<_>>().join(",");
    Ok(FrozenSkillBytes {
        stable_id,
        package_digest: snapshot.digest.clone(),
        body,
        resources: BTreeMap::new(),
    })
}

/// Restore a FrozenRuleSet from `FrozenSkillBytes.body`.
pub fn unpack_rules(material: &FrozenSkillBytes) -> Result<FrozenRuleSet, OptimizeError> {
    let snapshot = restore_value(
        &serde_json::from_str(&material.body)
            .map_err(|_| config_error("snapshot-rejected", "frozen rules are not version 2"))?,
    )?;
    if !material.package_digest.is_empty() && material.package_digest != snapshot.digest {
        return Err(config_error(
            "digest-mismatch",
            "frozen rule identity does not match its bytes",
        ));
    }
    Ok(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn skill(body: &str) -> String {
        format!("---\nname: sample\ndescription: internal\n---\n{body}")
    }

    fn sample_files() -> BTreeMap<String, String> {
        let mut files = BTreeMap::new();
        files.insert("shared/guidance.zh.md".into(), "共同约定\n".into());
        files.insert("shared/guidance.en.md".into(), "Shared guidance\n".into());
        files.insert("episode/SKILL.md".into(), skill("\n保留原文。\n"));
        files.insert(
            "episode/prompts/method.en.md".into(),
            "Preserve the source.\n".into(),
        );
        files.insert(
            "episode/request.json".into(),
            json!({
                "formatVersion": 2,
                "task": "episode",
                "kind": "request",
                "messages": [
                    {"id": "system", "role": "system", "parts": [
                        {"fileByLanguage": {"zh": "../shared/guidance.zh.md", "en": "../shared/guidance.en.md"}},
                        {"fileByLanguage": {"zh": "SKILL.md", "en": "prompts/method.en.md"}}
                    ]},
                    {"id": "user", "role": "user", "parts": [
                        {"input": "taskContext", "encoding": "json"}
                    ]}
                ]
            })
            .to_string(),
        );
        files.insert("layout/SKILL.md".into(), skill("\n布局词句。\n"));
        files.insert("layout/prompts/method.en.md".into(), "Layout phrase.\n".into());
        files.insert(
            "layout/request.json".into(),
            json!({
                "formatVersion": 2,
                "task": "layout",
                "kind": "material",
                "materials": [
                    {"slot": "guidance", "parts": [
                        {"fileByLanguage": {"zh": "../shared/guidance.zh.md", "en": "../shared/guidance.en.md"}}
                    ]},
                    {"slot": "method", "parts": [
                        {"fileByLanguage": {"zh": "SKILL.md", "en": "prompts/method.en.md"}}
                    ]}
                ]
            })
            .to_string(),
        );
        files
    }

    fn inputs() -> BTreeMap<String, Value> {
        BTreeMap::from([("taskContext".into(), json!({"script": "她说：\"{{不要替换}}\""}))])
    }

    #[test]
    fn request_order_inputs_and_material_slots_stay_separate() {
        let files = sample_files();
        let frozen = freeze_from_files(Language::Zh, &["episode", "layout"], &files).unwrap();
        let messages = render_request(&frozen, "episode", &inputs()).unwrap();
        assert_eq!(messages[0].id, "system");
        assert_eq!(messages[0].content, "共同约定\n\n保留原文。\n");
        assert_eq!(
            serde_json::from_str::<Value>(&messages[1].content).unwrap(),
            json!({"script": "她说：\"{{不要替换}}\""})
        );
        assert!(render_request(&frozen, "layout", &inputs()).is_err());
        let method = render_material(&frozen, "layout", "method", &BTreeMap::new()).unwrap();
        assert_eq!(method, "\n布局词句。\n");
        assert!(!method.contains("共同约定"));
        let guidance = render_material(&frozen, "layout", "guidance", &BTreeMap::new()).unwrap();
        assert_eq!(guidance, "共同约定\n");
        assert_eq!(
            frozen
                .files
                .keys()
                .filter(|name| name.ends_with("guidance.zh.md"))
                .count(),
            1
        );
    }

    #[test]
    fn an_edit_changes_only_the_next_freeze_and_restore_needs_no_directory() {
        let mut files = sample_files();
        let first = freeze_from_files(Language::En, &["episode"], &files).unwrap();
        let saved = serde_json::to_value(&first).unwrap();
        files.insert(
            "episode/prompts/method.en.md".into(),
            "New author instructions.\n".into(),
        );
        let second = freeze_from_files(Language::En, &["episode"], &files).unwrap();
        assert_ne!(first.digest, second.digest);
        let restored = restore_value(&saved).unwrap();
        let messages = render_request(&restored, "episode", &inputs()).unwrap();
        assert_eq!(messages[0].content, "Shared guidance\nPreserve the source.\n");
        assert_eq!(
            render_request(&second, "episode", &inputs()).unwrap()[0].content,
            "Shared guidance\nNew author instructions.\n"
        );
    }

    #[test]
    fn old_version_unknown_fields_missing_input_and_escapes_fail() {
        let mut files = sample_files();
        let mut declaration: Value =
            serde_json::from_str(&files["episode/request.json"]).unwrap();
        declaration["formatVersion"] = json!(1);
        files.insert("episode/request.json".into(), declaration.to_string());
        let err = freeze_from_files(Language::Zh, &["episode"], &files).unwrap_err();
        assert_eq!(err.code, error_codes::INTERNAL_PROMPT_CONFIG);

        let mut unknown = declaration;
        unknown["formatVersion"] = json!(2);
        unknown["extra"] = json!(true);
        files.insert("episode/request.json".into(), unknown.to_string());
        let err = freeze_from_files(Language::Zh, &["episode"], &files).unwrap_err();
        assert_eq!(err.code, error_codes::INTERNAL_PROMPT_CONFIG);

        let clean = sample_files();
        let frozen = freeze_from_files(Language::Zh, &["episode"], &clean).unwrap();
        assert!(render_request(&frozen, "episode", &BTreeMap::new()).is_err());

        let mut escaped = sample_files();
        escaped.insert(
            "episode/request.json".into(),
            serde_json::to_string(&json!({
                "formatVersion": 2,
                "task": "episode",
                "kind": "request",
                "messages": [{"id": "system", "role": "system", "parts": [
                    {"fileByLanguage": {"zh": "../../outside.md", "en": "SKILL.md"}}
                ]}]
            }))
            .unwrap(),
        );
        let err = freeze_from_files(Language::Zh, &["episode"], &escaped).unwrap_err();
        assert_eq!(err.code, error_codes::INTERNAL_PROMPT_CONFIG);

        escaped.insert(
            "episode/request.json".into(),
            serde_json::to_string(&json!({
                "formatVersion": 2,
                "task": "episode",
                "kind": "request",
                "messages": [{"id": "system", "role": "system", "parts": [
                    {"fileByLanguage": {"zh": "C:/outside.md", "en": "SKILL.md"}}
                ]}]
            }))
            .unwrap(),
        );
        let err = freeze_from_files(Language::Zh, &["episode"], &escaped).unwrap_err();
        assert_eq!(err.code, error_codes::INTERNAL_PROMPT_CONFIG);
    }

    #[test]
    fn task_name_must_be_lowercase_digits_or_hyphen() {
        let err = ident("task", "Image_Prompt").unwrap_err();
        assert_eq!(err.code, error_codes::INTERNAL_PROMPT_CONFIG);
        assert!(ident("task", "image-prompt-optimize").is_ok());
    }

    #[test]
    fn packed_rules_round_trip() {
        let frozen = freeze_from_files(Language::Zh, &["episode"], &sample_files()).unwrap();
        let packed = pack_rules(&frozen).unwrap();
        let restored = unpack_rules(&packed).unwrap();
        assert_eq!(restored.digest, frozen.digest);
        assert_eq!(
            render_request(&restored, "episode", &inputs()).unwrap()[0].content,
            render_request(&frozen, "episode", &inputs()).unwrap()[0].content
        );
        let mut bad = packed.clone();
        bad.package_digest = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into();
        assert_eq!(
            unpack_rules(&bad).unwrap_err().code,
            error_codes::INTERNAL_PROMPT_CONFIG
        );
    }
}