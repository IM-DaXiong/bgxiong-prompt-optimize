//! 模块：Creator 工具与规则渐进发现；保持同一合同、执行内核与离线可验证边界。
//! C09/CT07: verification report over every input the client-pack generator consumes. The report
//! records repository-relative paths and SHA-256 of LF-normalized bytes (the same CRLF->LF rule
//! the pack applies), so the generator can refuse any input that changed after verification.
//! This is a seam proof for one repository build, not a signature.

use crate::catalog::CreatorCatalog;
use crate::diagnostics::ContractViolation;
use crate::turn::CreatorTurnContract;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub const REPORT_SCHEMA_VERSION: u32 = 1;

/// R02/R03: definitions the Host reads by name to build published tool input schemas
/// (`creator_tools::tool_schemas`). A missing one fails the report and the build here, before
/// any runtime construction could fail.
pub const HOST_CONSUMED_DEFINITIONS: &[(&str, &str)] = &[
    ("clip-workflow.schema.json", "ClipFrameArtifactReadArgs"),
    ("creator-asr-tools.schema.json", "asr.models.list"),
    ("creator-asr-tools.schema.json", "asr.preferences.get"),
    ("creator-asr-tools.schema.json", "asr.transcribe"),
    (
        "creator-generative-video-edit-tools.schema.json",
        "video.edit.capabilities.list",
    ),
    (
        "creator-generative-video-edit-tools.schema.json",
        "video.edit.submit",
    ),
    (
        "creator-generative-video-edit-tools.schema.json",
        "video.edit.analyze",
    ),
    (
        "creator-generative-video-edit-tools.schema.json",
        "video.edit.selection.freeze",
    ),
    (
        "creator-generative-video-edit-tools.schema.json",
        "video.edit.preview.get",
    ),
    (
        "creator-generative-video-edit-tools.schema.json",
        "video.edit.task.get",
    ),
    ("creator-local-source-v1.schema.json", "rootsInput"),
    ("creator-local-source-v1.schema.json", "listInput"),
    ("creator-local-source-v1.schema.json", "readInput"),
    ("creator-local-source-v1.schema.json", "sourceRef"),
    ("creator-workbench-patch-v1.schema.json", "readInput"),
    ("creator-workbench-patch-v1.schema.json", "patchInput"),
    ("creator-media-describe-v1.schema.json", "describeInput"),
    ("video-span-v1.schema.json", "readArgs"),
    ("video-span-v1.schema.json", "prepareArgs"),
    (
        "video-prompt-projection.schema.json",
        "pipelineBusinessRequest",
    ),
];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportInput {
    pub path: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportCounts {
    pub examples: usize,
    pub example_schemas: usize,
    pub schemas: usize,
    pub catalog_tools: usize,
    pub json_inputs_parsed: usize,
    /// R03: `.schema.json` inputs verified as schema documents (keywords, arguments, refs).
    pub schema_documents_verified: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationReport {
    pub schema_version: u32,
    pub status: String,
    pub checks: Vec<String>,
    pub counts: ReportCounts,
    pub inputs: Vec<ReportInput>,
}

fn lf_sha256(bytes: &[u8]) -> String {
    let mut normalized = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\r' && i + 1 < bytes.len() && bytes[i + 1] == b'\n' {
            i += 1;
            continue;
        }
        normalized.push(bytes[i]);
        i += 1;
    }
    format!("{:x}", Sha256::digest(&normalized))
}

fn collect_client_sources(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| format!("REPORT_IO: read_dir {}: {e}", dir.display()))?
        .map(|e| e.map(|e| e.path()).map_err(|e| format!("REPORT_IO: {e}")))
        .collect::<Result<_, _>>()?;
    entries.sort();
    for p in entries {
        if p.file_name().and_then(|name| name.to_str()) == Some("__pycache__") {
            continue;
        }
        if p.is_dir() {
            collect_client_sources(&p, out)?;
        } else if p.is_file()
            && p.file_name().and_then(|name| name.to_str()) != Some("manifest.template.json")
            && !matches!(p.extension().and_then(|ext| ext.to_str()), Some("pyc" | "pyo"))
        {
            out.push(p);
        }
    }
    Ok(())
}

/// Exactly the authored client sources and contract inputs consumed by
/// `scripts/generate_creator_client_pack.py`. Local Python caches are not pack inputs.
pub fn generator_inputs(root: &Path) -> Result<Vec<PathBuf>, String> {
    let source = root.join("packages").join("creator-client").join("v1");
    let contracts = root.join("packages").join("app-contracts").join("v1");
    let mut files = Vec::new();
    collect_client_sources(&source, &mut files)?;
    let mut creator: Vec<PathBuf> = std::fs::read_dir(&contracts)
        .map_err(|e| format!("REPORT_IO: read_dir {}: {e}", contracts.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.is_file()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("creator-") && n.ends_with(".json"))
        })
        .collect();
    creator.sort();
    files.extend(creator);
    for name in [
        "video-span-v1.schema.json",
        "video-prompt-projection.schema.json",
        "video-resolution-selection.schema.json",
        "asr.schema.json",
        "clip-workflow.schema.json",
        "story-creation.schema.json",
        "story-creation-model.schema.json",
        "story-creation-content.schema.json",
    ] {
        let p = contracts.join(name);
        if p.is_file() {
            files.push(p);
        }
    }
    Ok(files)
}

fn rel(root: &Path, p: &Path) -> String {
    p.strip_prefix(root)
        .unwrap_or(p)
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// Verify the contracts under `root` (repository root) and build the report. Any failure is a
/// typed violation; nothing is skipped to make the report pass.
pub fn verify_root(root: &Path) -> Result<VerificationReport, ContractViolation> {
    let io = |m: String| ContractViolation::new("", "", m);
    let contracts = root.join("packages").join("app-contracts").join("v1");
    let read = |name: &str| {
        std::fs::read_to_string(contracts.join(name))
            .map_err(|e| io(format!("REPORT_IO: {name}: {e}")))
    };
    let turn = CreatorTurnContract::parse_and_verify(&read(crate::TURN_CONTRACT_FILE)?)?;
    let catalog = CreatorCatalog::parse_and_verify(&read(crate::CATALOG_FILE)?)?;
    let discovery = crate::discovery::DiscoveryConfig::parse(&read("creator-tool-discovery.json")?)
        .map_err(|e| ContractViolation::new("creator-tool-discovery.json", "", e))?;
    let rules = serde_json::from_str(&read("creator-tool-rules.json")?)
        .map_err(|e| ContractViolation::new("creator-tool-rules.json", "", format!("{e}")))?;
    let errors = serde_json::from_str(&read("creator-tool-errors.json")?)
        .map_err(|e| ContractViolation::new("creator-tool-errors.json", "", format!("{e}")))?;
    discovery
        .verify_schema(
            &read("creator-tool-discovery.json")?,
            &read("creator-tool-discovery.schema.json")?,
        )
        .map_err(|e| ContractViolation::new("creator-tool-discovery.schema.json", "", e))?;
    discovery
        .verify(&catalog, &rules, &errors)
        .map_err(|e| ContractViolation::new("creator-tool-discovery.json", "", e))?;
    let parse = |name: &str| -> Result<serde_json::Value, ContractViolation> {
        serde_json::from_str(&read(name)?)
            .map_err(|e| ContractViolation::new(name, "", e.to_string()))
    };
    crate::text_fields::verify(
        &parse("creator-resource-text-fields.json")?,
        &parse("creator-resource-text-fields.schema.json")?,
        &parse(crate::CATALOG_FILE)?,
    )?;
    crate::effect_policy::verify(&parse("creator-effect-policy.json")?)
        .map_err(|e| ContractViolation::new("creator-effect-policy.json", "", e))?;
    let files = generator_inputs(root).map_err(io)?;
    let mut inputs = Vec::new();
    let mut json_inputs_parsed = 0;
    // file name -> parsed document, so cross-file `$ref`s resolve against what is consumed.
    let mut parsed: std::collections::BTreeMap<String, (String, serde_json::Value)> =
        Default::default();
    for p in &files {
        let bytes =
            std::fs::read(p).map_err(|e| io(format!("REPORT_IO: {}: {e}", rel(root, p))))?;
        let r = rel(root, p);
        if r.ends_with(".json") {
            let v = serde_json::from_slice::<serde_json::Value>(&bytes)
                .map_err(|e| ContractViolation::new(&r, "", format!("JSON_PARSE: {e}")))?;
            json_inputs_parsed += 1;
            let name = p
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_string();
            parsed.insert(name, (r.clone(), v));
        }
        inputs.push(ReportInput {
            path: r,
            sha256: lf_sha256(&bytes),
        });
    }
    // R03: JSON that parses is not yet a valid schema. Every consumed `.schema.json` input is
    // verified as a schema document before the report can pass.
    let schema_documents_verified = verify_schema_documents(&contracts, &parsed)?;
    for (file, name) in HOST_CONSUMED_DEFINITIONS {
        let (r, doc) = parsed.get(*file).ok_or_else(|| {
            io(format!(
                "HOST_CONTRACT_MISSING: {file} is not a generator input"
            ))
        })?;
        if doc.get("definitions").and_then(|d| d.get(*name)).is_none() {
            return Err(ContractViolation::new(
                r,
                "",
                format!("SCHEMA_DEFINITION_MISSING: definitions.{name} (read by the Host)"),
            ));
        }
    }
    let (examples, example_schemas, schemas) = turn.counts();
    Ok(VerificationReport {
        schema_version: REPORT_SCHEMA_VERSION,
        status: "passed".into(),
        checks: vec![
            "turnContract.parseResolveBudgetsReviewStageExamples".into(),
            "turnContract.schemaKeywords".into(),
            "catalog.fieldsUniqueWireInjective".into(),
            "generatorInputs.jsonParse".into(),
            "generatorInputs.schemaDocuments".into(),
            "hostConsumedDefinitions.present".into(),
            "textFields.uniqueOwnerAdapterHistory".into(),
            "effectPolicy.crossFieldSemantics".into(),
        ],
        counts: ReportCounts {
            examples,
            example_schemas,
            schemas,
            catalog_tools: catalog.tools().len(),
            json_inputs_parsed,
            schema_documents_verified,
        },
        inputs,
    })
}

/// R03: verify every `.schema.json` among the parsed inputs as a schema document. Cross-file
/// refs resolve against the other parsed inputs first, then against sibling contract files under
/// `packages/app-contracts/v1` (loaded on demand); a ref to a file that exists nowhere fails.
fn verify_schema_documents(
    contracts: &Path,
    parsed: &std::collections::BTreeMap<String, (String, serde_json::Value)>,
) -> Result<usize, ContractViolation> {
    let mut siblings: std::collections::BTreeMap<String, serde_json::Value> = Default::default();
    // Preload every sibling a document references but the input set does not contain.
    for (_, doc) in parsed.values() {
        let mut refs = Vec::new();
        collect_ref_files(doc, &mut refs);
        for f in refs {
            if parsed.contains_key(&f) || siblings.contains_key(&f) {
                continue;
            }
            let p = contracts.join(&f);
            if let Ok(text) = std::fs::read_to_string(&p) {
                let v = serde_json::from_str(&text)
                    .map_err(|e| ContractViolation::new(&f, "", format!("JSON_PARSE: {e}")))?;
                siblings.insert(f, v);
            }
        }
    }
    let lookup = |f: &str| parsed.get(f).map(|(_, v)| v).or_else(|| siblings.get(f));
    let mut n = 0;
    for (name, (r, doc)) in parsed {
        if !name.ends_with(".schema.json") {
            continue;
        }
        crate::schema_value::verify_schema_document(&crate::schema_value::SchemaDocument {
            file: r,
            root: doc,
            external: &lookup,
        })
        .map_err(|e| ContractViolation::new(r, "", e))?;
        n += 1;
    }
    Ok(n)
}

fn collect_ref_files(v: &serde_json::Value, out: &mut Vec<String>) {
    match v {
        serde_json::Value::Object(o) => {
            if let Some(r) = o.get("$ref").and_then(|r| r.as_str()) {
                let f = r.split_once('#').map_or(r, |(f, _)| f);
                if !f.is_empty() && !out.iter().any(|x| x == f) {
                    out.push(f.to_string());
                }
            }
            o.values().for_each(|x| collect_ref_files(x, out));
        }
        serde_json::Value::Array(a) => a.iter().for_each(|x| collect_ref_files(x, out)),
        _ => {}
    }
}
