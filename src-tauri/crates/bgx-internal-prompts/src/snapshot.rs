use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::error::PromptError;
use crate::limits::{MAX_FILES, MAX_TOTAL_BYTES};
use crate::package::{self, Declaration};
use crate::source::{read_text, reject_if_changed, require_directory};
use crate::types::{FrozenRuleSet, Language, RenderedMessage, TaskKind, FORMAT_VERSION};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DigestBody<'a> {
    format_version: u32,
    language: &'a str,
    tasks: &'a BTreeMap<String, TaskKind>,
    files: &'a BTreeMap<String, String>,
}

pub fn freeze_set(
    root: &Path,
    language: Language,
    task_ids: &[&str],
) -> Result<FrozenRuleSet, PromptError> {
    require_directory(root)?;
    if task_ids.is_empty() {
        return Err(PromptError::new("empty-task-set", "freeze_set needs at least one task"));
    }
    let mut ordered = BTreeSet::new();
    for task in task_ids {
        if !ordered.insert((*task).to_owned()) {
            return Err(PromptError::new("duplicate-task", *task));
        }
    }
    let mut tasks = BTreeMap::new();
    let mut files = BTreeMap::new();
    for task in &ordered {
        let declaration_name = format!("{task}/request.json");
        let raw = read_text(root, &declaration_name)?;
        let declaration = package::parse_declaration(&raw)?;
        if declaration.task != *task {
            return Err(PromptError::new("task-mismatch", declaration_name));
        }
        tasks.insert(task.clone(), declaration.kind);
        files.insert(declaration_name, raw);
        for name in package::files_for(&declaration, Some(language))? {
            if !files.contains_key(&name) {
                files.insert(name.clone(), read_text(root, &name)?);
            }
        }
    }
    enforce_budget(&files)?;
    reject_if_changed(root, &files)?;
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

pub fn restore(snapshot: FrozenRuleSet) -> Result<FrozenRuleSet, PromptError> {
    verify_set(&snapshot)?;
    Ok(snapshot)
}

pub fn restore_value(value: &serde_json::Value) -> Result<FrozenRuleSet, PromptError> {
    let snapshot: FrozenRuleSet = serde_json::from_value(value.clone())
        .map_err(|_| PromptError::new("snapshot-rejected", "frozen rules are not version 2"))?;
    restore(snapshot)
}

pub fn render_request(
    snapshot: &FrozenRuleSet,
    task: &str,
    inputs: &BTreeMap<String, serde_json::Value>,
) -> Result<Vec<RenderedMessage>, PromptError> {
    verify_set(snapshot)?;
    let declaration = declaration_of(snapshot, task)?;
    package::render_request(&declaration, snapshot.language, &snapshot.files, inputs)
}

pub fn render_material(
    snapshot: &FrozenRuleSet,
    task: &str,
    slot: &str,
    inputs: &BTreeMap<String, serde_json::Value>,
) -> Result<String, PromptError> {
    verify_set(snapshot)?;
    let declaration = declaration_of(snapshot, task)?;
    package::render_material(
        &declaration,
        snapshot.language,
        slot,
        &snapshot.files,
        inputs,
    )
}

pub fn render_method_body(
    snapshot: &FrozenRuleSet,
    task: &str,
) -> Result<String, PromptError> {
    verify_set(snapshot)?;
    let declaration = declaration_of(snapshot, task)?;
    package::render_method_body(&declaration, snapshot.language, &snapshot.files)
}

fn declaration_of(snapshot: &FrozenRuleSet, task: &str) -> Result<Declaration, PromptError> {
    if !snapshot.tasks.contains_key(task) {
        return Err(PromptError::new("unknown-task", task));
    }
    let name = format!("{task}/request.json");
    let raw = snapshot
        .files
        .get(&name)
        .ok_or_else(|| PromptError::new("missing-resource", name.clone()))?;
    let declaration = package::parse_declaration(raw)?;
    if declaration.task != task || snapshot.tasks.get(task) != Some(&declaration.kind) {
        return Err(PromptError::new("task-mismatch", name));
    }
    Ok(declaration)
}

pub fn verify_set(snapshot: &FrozenRuleSet) -> Result<(), PromptError> {
    if snapshot.format_version != FORMAT_VERSION {
        return Err(PromptError::new(
            "unsupported-version",
            "only formatVersion 2 is accepted",
        ));
    }
    if snapshot.tasks.is_empty() {
        return Err(PromptError::new("empty-task-set", "frozen rules have no tasks"));
    }
    if content_digest(snapshot)? != snapshot.digest {
        return Err(PromptError::new(
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
            .ok_or_else(|| PromptError::new("missing-resource", name.clone()))?;
        let declaration = package::parse_declaration(raw)?;
        if declaration.task != *task || declaration.kind != *kind {
            return Err(PromptError::new("task-mismatch", name));
        }
        expected.insert(name);
        for file in package::files_for(&declaration, Some(snapshot.language))? {
            if !snapshot.files.contains_key(&file) {
                return Err(PromptError::new("missing-resource", file));
            }
            expected.insert(file);
        }
    }
    for name in snapshot.files.keys() {
        if !expected.contains(name) {
            return Err(PromptError::new("unexpected-resource", name.clone()));
        }
    }
    Ok(())
}

fn content_digest(snapshot: &FrozenRuleSet) -> Result<String, PromptError> {
    let body = DigestBody {
        format_version: snapshot.format_version,
        language: snapshot.language.key(),
        tasks: &snapshot.tasks,
        files: &snapshot.files,
    };
    let bytes = serde_json::to_vec(&body)
        .map_err(|_| PromptError::new("digest-rejected", "frozen rules could not be hashed"))?;
    Ok(format!("sha256:{}", hex::encode(Sha256::digest(bytes))))
}

fn enforce_budget(files: &BTreeMap<String, String>) -> Result<(), PromptError> {
    if files.len() > MAX_FILES {
        return Err(PromptError::new("too-many-files", files.len().to_string()));
    }
    let total: u64 = files.values().map(|text| text.len() as u64).sum();
    if total > MAX_TOTAL_BYTES {
        return Err(PromptError::new("closure-too-large", total.to_string()));
    }
    Ok(())
}
