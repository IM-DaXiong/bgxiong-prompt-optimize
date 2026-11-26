//! `gen-video-prompt-contracts`: offline writer / checker for the TypeScript twins of the
//! generated app contracts touched by the video prompt projection plan (VS07).
//!
//! `src-tauri/build.rs` generates the Rust side into `OUT_DIR` with the same
//! `schema_types::generate` calls and fails the build when a TypeScript twin differs, so this
//! binary only refreshes the twins before a build:
//!
//! ```text
//! cargo run --manifest-path src-tauri/crates/bgx-contract-gen/Cargo.toml \
//!     --bin gen-video-prompt-contracts -- --write
//! ```
//!
//! `--check` reports stale twins without writing (non-zero exit). Paths resolve from this
//! crate's manifest directory, so the working directory does not matter. It never builds the
//! application and never edits a schema.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use bgx_contract_gen::schema_types;

struct Failure(String);

impl fmt::Debug for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

enum Mode {
    Write,
    Check,
}

/// One generated TypeScript file: repository-relative path and expected bytes.
struct Twin {
    path: &'static str,
    contents: String,
}

const STAGE_DOCUMENTS: &[&str] = &[
    "creative-context-pack-v1.schema.json",
    "video-prompt-projection-draft.schema.json",
    "video-prompt-channels.schema.json",
    "video-prompt-stage-contract.json",
];
const STAGE_DEFINITIONS: &[&str] = &[
    "frozenSource",
    "motherReviewResult",
    "expressionTurn",
    "failurePayload",
    "methodStageAdvisory",
];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")
}

fn read_docs(v1: &Path, names: &[&str]) -> Result<BTreeMap<String, serde_json::Value>, Failure> {
    let mut docs = BTreeMap::new();
    for name in names {
        let path = v1.join(name);
        let text = std::fs::read_to_string(&path)
            .map_err(|e| Failure(format!("read {}: {e}", path.display())))?;
        let value = serde_json::from_str(&text)
            .map_err(|e| Failure(format!("parse {}: {e}", path.display())))?;
        docs.insert((*name).to_string(), value);
    }
    Ok(docs)
}

/// Same generator call as the Agent Skills library block in `src-tauri/build.rs`.
fn library_twin(v1: &Path) -> Result<Twin, Failure> {
    let docs = read_docs(
        v1,
        &["agent-skills.json", "agent-skill-library.schema.json"],
    )?;
    let types = schema_types::generate(
        &docs,
        "agent-skill-library.schema.json",
        &[
            "Index",
            "entryToken",
            "category",
            "storedDraft",
            "storedDraftPointer",
            "patchInput",
            "libraryListInput",
            "libraryCursor",
            "storedPreparedPackage",
            "projectionBindingState",
        ],
        "SkillLibrary",
    )
    .map_err(|e| Failure(format!("agent-skill-library.schema.json types: {e}")))?;
    Ok(Twin {
        path: "ui/src/domain/agent-skills/libraryContract.generated.ts",
        contents: types.typescript,
    })
}

/// Same generator calls as the video prompt projection block in `src-tauri/build.rs`: one call
/// per schema document, TypeScript outputs concatenated in this order.
fn projection_twin(v1: &Path) -> Result<Twin, Failure> {
    let docs = read_docs(
        v1,
        &[
            "agent-skills.json",
            "agent-skill-library.schema.json",
            "video-prompt-projection.schema.json",
            "video-prompt-projection-draft.schema.json",
            "video-prompt-channels.schema.json",
        ],
    )?;
    let projection = schema_types::generate(
        &docs,
        "video-prompt-projection.schema.json",
        &[
            "pipelineMode",
            "projectionMethodSelection",
            "canonicalDocumentRef",
            "spanSourceRef",
            "frozenMethod",
            "targetSnapshot",
            "projectionReview",
            "projectionReceipt",
            "projectionQueryInput",
            "projectionQueryResult",
            "negativeInput",
            "negativeOperation",
            "expansionMode",
            "projectionDraft",
            "projectionReviewResult",
        ],
        "VideoPrompt",
    )
    .map_err(|e| Failure(format!("video-prompt-projection.schema.json types: {e}")))?;
    Ok(Twin {
        path: "ui/src/domain/video-prompt-pipeline/projectionContract.generated.ts",
        contents: projection.typescript,
    })
}

/// Same generator call as the video prompt stage-contract block in `src-tauri/build.rs`.
fn stage_twin(v1: &Path) -> Result<Twin, Failure> {
    let docs = read_docs(v1, STAGE_DOCUMENTS)?;
    let stage = generate_stage_types(&docs)?;
    Ok(Twin {
        path: "ui/src/domain/video-prompt-pipeline/stageContract.generated.ts",
        contents: stage.typescript,
    })
}

/// The stage contract is closed: the shared generator accepts open records
/// (`additionalProperties: true` -> flattened extension map), this document must not use them.
fn refuse_open_records(path: &str, value: &serde_json::Value) -> Result<(), Failure> {
    match value {
        serde_json::Value::Object(map) => {
            if map.get("additionalProperties") == Some(&serde_json::Value::Bool(true)) {
                return Err(Failure(format!(
                    "video-prompt-stage-contract.json: {path} is an open record; stage contracts are closed"
                )));
            }
            for (key, child) in map {
                refuse_open_records(&format!("{path}/{key}"), child)?;
            }
            Ok(())
        }
        serde_json::Value::Array(items) => {
            for (i, child) in items.iter().enumerate() {
                refuse_open_records(&format!("{path}/{i}"), child)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn generate_stage_types(
    docs: &BTreeMap<String, serde_json::Value>,
) -> Result<schema_types::GeneratedTypes, Failure> {
    if let Some(doc) = docs.get("video-prompt-stage-contract.json") {
        refuse_open_records("#", doc)?;
    }
    let stage = schema_types::generate(
        docs,
        "video-prompt-stage-contract.json",
        STAGE_DEFINITIONS,
        "VideoPromptStage",
    )
    .map_err(|e| Failure(format!("video-prompt-stage-contract.json types: {e}")))?;
    Ok(stage)
}

fn main() -> Result<(), Failure> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = match args.as_slice() {
        [flag] if flag == "--write" => Mode::Write,
        [flag] if flag == "--check" => Mode::Check,
        _ => {
            return Err(Failure(
                "usage: gen-video-prompt-contracts (--write | --check)".to_string(),
            ))
        }
    };
    let root = repo_root();
    let v1 = root.join("packages/app-contracts/v1");
    let twins = [library_twin(&v1)?, projection_twin(&v1)?, stage_twin(&v1)?];
    let mut stale = Vec::new();
    for twin in &twins {
        let path = root.join(twin.path);
        let current = std::fs::read_to_string(&path)
            .unwrap_or_default()
            .replace("\r\n", "\n");
        if current == twin.contents.replace("\r\n", "\n") {
            println!("up to date: {}", twin.path);
            continue;
        }
        match mode {
            Mode::Write => {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| Failure(format!("create {}: {e}", parent.display())))?;
                }
                std::fs::write(&path, &twin.contents)
                    .map_err(|e| Failure(format!("write {}: {e}", path.display())))?;
                println!("wrote: {}", twin.path);
            }
            Mode::Check => {
                println!("stale: {}", twin.path);
                stale.push(twin.path);
            }
        }
    }
    if stale.is_empty() {
        Ok(())
    } else {
        Err(Failure(format!(
            "stale TypeScript twins: {}",
            stale.join(", ")
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_generation_rejects_an_open_record() {
        let v1 = repo_root().join("packages/app-contracts/v1");
        let mut docs = read_docs(&v1, STAGE_DOCUMENTS).unwrap();
        docs.get_mut("video-prompt-stage-contract.json").unwrap()["definitions"]
            ["failurePayload"]["additionalProperties"] = serde_json::json!(true);
        assert!(generate_stage_types(&docs).is_err());
    }
}
