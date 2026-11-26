use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use bgx_contract_gen::schema_types;

const SCHEMA: &str = "video-span-v1.schema.json";
const DEFINITIONS: &[&str] = &["intent", "readiness", "submitAcknowledgement", "vm", "readArgs", "prepareArgs", "frameworkImportReceipt"];

fn main() -> Result<(), String> {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "--check".into());
    if mode != "--check" && mode != "--write" {
        return Err("usage: gen-video-span-contracts --check|--write".into());
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let schema_path = root.join("packages/app-contracts/v1").join(SCHEMA);
    let raw = std::fs::read_to_string(&schema_path).expect("read video span schema");
    let schema = serde_json::from_str(&raw).expect("parse video span schema");
    let contracts = root.join("packages/app-contracts/v1");
    let mut docs = BTreeMap::from([(SCHEMA.to_string(), schema)]);
    for name in ["video-prompt-projection.schema.json", "video-span-migration-resolution.schema.json", "agent-skills.json"] {
        let raw = std::fs::read_to_string(contracts.join(name)).expect("read referenced schema");
        docs.insert(name.to_string(), serde_json::from_str(&raw).expect("parse referenced schema"));
    }
    let generated = schema_types::generate(&docs, SCHEMA, DEFINITIONS, "VideoSpan")
        .expect("generate video span contract types");
    let target: PathBuf = root.join("ui/src/domain/video-span/contracts.generated.ts");
    match mode.as_str() {
        "--write" => {
            std::fs::create_dir_all(target.parent().expect("generated TS parent")).expect("create TS parent");
            std::fs::write(&target, generated.typescript).expect("write generated TS");
        }
        "--check" => {
            let actual = std::fs::read_to_string(&target).expect("read generated TS");
            if actual.replace("\r\n", "\n") != generated.typescript.replace("\r\n", "\n") {
                return Err("video span TS contract is stale; run scripts/generate-video-span-contracts.mjs --write".into());
            }
        }
        _ => unreachable!(),
    }
    Ok(())
}
