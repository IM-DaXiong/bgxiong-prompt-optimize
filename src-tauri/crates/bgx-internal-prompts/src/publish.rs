use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::PromptError;
use crate::package::{self, Declaration};
use crate::snapshot::freeze_set;
use crate::source::{self, read_text, require_directory};
use crate::types::{Language, TaskKind};

const ALLOWED_ROOT_FILES: &[&str] = &["catalog.json"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageReport {
    pub digest: String,
    pub generation: PathBuf,
}

#[derive(Deserialize)]
struct CatalogFile {
    skills: Vec<CatalogSkill>,
}

#[derive(Deserialize)]
struct CatalogSkill {
    id: String,
    usage: String,
}

pub fn validate_root(root: &Path) -> Result<(), PromptError> {
    require_directory(root)?;
    let catalog = read_catalog(root)?;
    let found = package_ids(root)?;
    if found != catalog.keys().cloned().collect::<BTreeSet<_>>() {
        return Err(PromptError::new(
            "catalog-mismatch",
            "catalog ids and package directories differ",
        ));
    }
    let mut closure = BTreeSet::new();
    closure.insert("catalog.json".to_owned());
    for (task, usage) in &catalog {
        let name = format!("{task}/request.json");
        let raw = read_text(root, &name)?;
        let declaration = package::parse_declaration(&raw)?;
        let expected = match usage.as_str() {
            "request" => TaskKind::Request,
            "material" => TaskKind::Material,
            other => return Err(PromptError::new("usage-rejected", other)),
        };
        if declaration.task != *task || declaration.kind != expected {
            return Err(PromptError::new("kind-mismatch", task.clone()));
        }
        closure.insert(name);
        for file in package::files_for(&declaration, None)? {
            closure.insert(file);
        }
    }
    let present = walk_files(root, root)?;
    let mut allowed = closure;
    allowed.extend(lexicon_files(root)?);
    if present != allowed {
        return Err(PromptError::new(
            "closure-mismatch",
            "the directory is not exactly the declared resource closure",
        ));
    }
    let ids: Vec<&str> = catalog.keys().map(String::as_str).collect();
    freeze_set(root, Language::Zh, &ids)?;
    freeze_set(root, Language::En, &ids)?;
    Ok(())
}

pub fn stage_root(source: &Path, destination: &Path) -> Result<StageReport, PromptError> {
    validate_root(source)?;
    require_directory(destination)?;
    let files = closure_bytes(source)?;
    let digest = closure_digest(&files)?;
    let hex = digest
        .strip_prefix("sha256:")
        .ok_or_else(|| PromptError::new("digest-rejected", "generation digest is not sha256"))?;
    let generations = destination.join("generations");
    if generations.exists() {
        let meta = fs::symlink_metadata(&generations)
            .map_err(|_| PromptError::new("destination-unavailable", "generations"))?;
        if meta.file_type().is_symlink() || !meta.is_dir() {
            return Err(PromptError::new(
                "destination-occupied",
                "generations is not a real directory",
            ));
        }
    } else {
        fs::create_dir(&generations)
            .map_err(|_| PromptError::new("cannot-write", "generations"))?;
    }
    let final_dir = generations.join(hex);
    if final_dir.exists() {
        return Err(PromptError::new(
            "destination-occupied",
            "that generation is already present",
        ));
    }
    let partial = generations.join(format!("{hex}.partial"));
    if partial.exists() {
        fs::remove_dir_all(&partial)
            .map_err(|_| PromptError::new("cannot-write", "partial generation"))?;
    }
    if let Err(error) = write_tree(&partial, &files).and_then(|_| validate_root(&partial)) {
        let _ = fs::remove_dir_all(&partial);
        return Err(error);
    }
    fs::rename(&partial, &final_dir).map_err(|_| {
        let _ = fs::remove_dir_all(&partial);
        PromptError::new("cannot-activate", "generation directory could not be renamed")
    })?;
    let manifest = generations.join(format!("{hex}.manifest.json"));
    let body = format!("{{\"digest\":\"{digest}\"}}\n");
    if fs::write(&manifest, body).is_err() {
        let _ = fs::remove_dir_all(&final_dir);
        let _ = fs::remove_file(&manifest);
        return Err(PromptError::new("cannot-write", "generation manifest"));
    }
    Ok(StageReport {
        digest,
        generation: final_dir,
    })
}

fn read_catalog(root: &Path) -> Result<BTreeMap<String, String>, PromptError> {
    let raw = read_text(root, "catalog.json")?;
    let catalog: CatalogFile = serde_json::from_str(&raw)
        .map_err(|_| PromptError::new("catalog-rejected", "catalog.json"))?;
    let mut skills = BTreeMap::new();
    for skill in catalog.skills {
        if skills.insert(skill.id.clone(), skill.usage).is_some() {
            return Err(PromptError::new("duplicate-task", skill.id));
        }
    }
    if skills.is_empty() {
        return Err(PromptError::new("empty-task-set", "catalog"));
    }
    Ok(skills)
}

fn package_ids(root: &Path) -> Result<BTreeSet<String>, PromptError> {
    let mut ids = BTreeSet::new();
    for entry in fs::read_dir(root).map_err(|_| PromptError::new("cannot-read", "root"))? {
        let entry = entry.map_err(|_| PromptError::new("cannot-read", "root"))?;
        let name = entry
            .file_name()
            .to_str()
            .ok_or_else(|| PromptError::new("path-rejected", "non-utf8 directory"))?
            .to_owned();
        let meta = fs::symlink_metadata(entry.path())
            .map_err(|_| PromptError::new("cannot-read", name.clone()))?;
        if meta.file_type().is_symlink() {
            return Err(PromptError::new("path-is-link", name));
        }
        if meta.is_file() {
            if !ALLOWED_ROOT_FILES.contains(&name.as_str()) {
                return Err(PromptError::new("unexpected-resource", name));
            }
            continue;
        }
        if !meta.is_dir() {
            return Err(PromptError::new("unexpected-resource", name));
        }
        if name == "shared" {
            continue;
        }
        let declaration = entry.path().join("request.json");
        let declaration_meta = fs::symlink_metadata(&declaration)
            .map_err(|_| PromptError::new("missing-resource", format!("{name}/request.json")))?;
        if declaration_meta.file_type().is_symlink() || !declaration_meta.is_file() {
            return Err(PromptError::new("not-a-file", format!("{name}/request.json")));
        }
        ids.insert(name);
    }
    Ok(ids)
}

fn walk_files(root: &Path, dir: &Path) -> Result<BTreeSet<String>, PromptError> {
    let mut files = BTreeSet::new();
    for entry in fs::read_dir(dir).map_err(|_| PromptError::new("cannot-read", "directory"))? {
        let entry = entry.map_err(|_| PromptError::new("cannot-read", "directory"))?;
        let path = entry.path();
        let meta = fs::symlink_metadata(&path)
            .map_err(|_| PromptError::new("cannot-read", "directory entry"))?;
        let relative = path
            .strip_prefix(root)
            .map_err(|_| PromptError::new("path-escape", "walk left the root"))?;
        let name = relative
            .components()
            .map(|component| component.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        if meta.file_type().is_symlink() {
            return Err(PromptError::new("path-is-link", name));
        }
        if meta.is_dir() {
            files.extend(walk_files(root, &path)?);
        } else if meta.is_file() {
            files.insert(name);
        } else {
            return Err(PromptError::new("unexpected-resource", name));
        }
    }
    Ok(files)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LexiconIndex {
    files: Vec<String>,
}

/// Style lexicons live beside the packages. They are selected by the host later
/// and are not copied into every frozen rule set.
fn lexicon_files(root: &Path) -> Result<BTreeSet<String>, PromptError> {
    let index_path = root.join("shared/data/index.json");
    if !index_path.exists() {
        return Ok(BTreeSet::new());
    }
    let raw = read_text(root, "shared/data/index.json")?;
    let index: LexiconIndex = serde_json::from_str(&raw)
        .map_err(|_| PromptError::new("lexicon-rejected", "shared/data/index.json"))?;
    let mut expected = BTreeSet::from(["shared/data/index.json".to_owned()]);
    for name in &index.files {
        if name.is_empty()
            || name.contains('/')
            || name.contains('\\')
            || name.contains(':')
            || name == "index.json"
        {
            return Err(PromptError::new("lexicon-rejected", name.clone()));
        }
        let relative = format!("shared/data/{name}");
        let text = read_text(root, &relative)?;
        serde_json::from_str::<serde_json::Value>(&text)
            .map_err(|_| PromptError::new("lexicon-rejected", relative.clone()))?;
        if !expected.insert(relative) {
            return Err(PromptError::new("duplicate-lexicon", name.clone()));
        }
    }
    let mut on_disk = BTreeSet::new();
    let dir = root.join("shared/data");
    for entry in fs::read_dir(&dir).map_err(|_| PromptError::new("cannot-read", "shared/data"))? {
        let entry = entry.map_err(|_| PromptError::new("cannot-read", "shared/data"))?;
        let meta = fs::symlink_metadata(entry.path())
            .map_err(|_| PromptError::new("cannot-read", "shared/data"))?;
        let name = entry
            .file_name()
            .to_str()
            .ok_or_else(|| PromptError::new("path-rejected", "shared/data"))?
            .to_owned();
        if meta.file_type().is_symlink() || !meta.is_file() {
            return Err(PromptError::new("path-is-link", format!("shared/data/{name}")));
        }
        on_disk.insert(format!("shared/data/{name}"));
    }
    if on_disk != expected {
        return Err(PromptError::new(
            "lexicon-mismatch",
            "shared/data does not match its index",
        ));
    }
    Ok(expected)
}

fn closure_bytes(root: &Path) -> Result<BTreeMap<String, String>, PromptError> {
    let catalog = read_catalog(root)?;
    let mut files = BTreeMap::new();
    files.insert("catalog.json".to_owned(), read_text(root, "catalog.json")?);
    for task in catalog.keys() {
        let name = format!("{task}/request.json");
        let raw = read_text(root, &name)?;
        let declaration: Declaration = package::parse_declaration(&raw)?;
        files.insert(name, raw);
        for file in package::files_for(&declaration, None)? {
            if !files.contains_key(&file) {
                files.insert(file.clone(), read_text(root, &file)?);
            }
        }
    }
    for relative in lexicon_files(root)? {
        files.insert(relative.clone(), read_text(root, &relative)?);
    }
    Ok(files)
}

fn closure_digest(files: &BTreeMap<String, String>) -> Result<String, PromptError> {
    let bytes = serde_json::to_vec(files)
        .map_err(|_| PromptError::new("digest-rejected", "generation digest"))?;
    Ok(format!("sha256:{}", hex::encode(Sha256::digest(bytes))))
}

fn write_tree(root: &Path, files: &BTreeMap<String, String>) -> Result<(), PromptError> {
    fs::create_dir(root).map_err(|_| PromptError::new("cannot-write", "generation"))?;
    for (relative, text) in files {
        let path = source::rooted(root, relative)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|_| PromptError::new("cannot-write", relative.clone()))?;
        }
        fs::write(&path, text).map_err(|_| PromptError::new("cannot-write", relative.clone()))?;
    }
    Ok(())
}
