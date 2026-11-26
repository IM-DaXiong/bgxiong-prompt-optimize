//! 视频分辨率值与平台尺寸证明：JSON Schema单一源生成Rust/TS，禁止客户端手写镜像。
use std::{collections::BTreeMap, path::Path};
use crate::schema_types::{self, GeneratedTypes};

fn schema(root: &Path, name: &str, definitions: &[&str], prefix: &str) -> Result<GeneratedTypes, String> {
    let raw = crate::read_contract_json(&root.join("packages/app-contracts/v1").join(name));
    crate::require_schema_v1(&raw, name);
    schema_types::generate(&BTreeMap::from([(name.to_owned(), raw)]), name, definitions, prefix)
}

/// 模型无关数值/能力及真实请求选项；供adapter核心与GUI共用。
pub fn generate(root: &Path) -> Result<GeneratedTypes, String> {
    schema(root, "video-resolution-selection.schema.json", &["value", "valueCaps", "selection"], "VideoResolution")
}

/// ComfyUI私有图证明只进入平台核心，不污染公共SDK。
pub fn generate_proof(root: &Path) -> Result<GeneratedTypes, String> {
    schema(root, "comfy-resolution-proof.schema.json", &["selectorProof"], "ComfyResolution")
}
