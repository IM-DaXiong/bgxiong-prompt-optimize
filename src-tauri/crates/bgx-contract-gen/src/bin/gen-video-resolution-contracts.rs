//! 从分辨率机读合同生成UI类型；支持生成和只读漂移检查。
use std::path::Path;
fn main() -> Result<(), String> {
    let mode = std::env::args().nth(1).unwrap_or("--check".into());
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let neutral = bgx_contract_gen::video_resolution::generate(&root)?;
    let platform = bgx_contract_gen::video_resolution::generate_proof(&root)?;
    for (name, content) in [("videoResolution.generated.ts", neutral.typescript), ("comfyResolution.generated.ts", platform.typescript)] {
        let path = root.join("ui/src/api").join(name);
        match mode.as_str() {
            "--write" => std::fs::write(path, content).map_err(|e| e.to_string())?,
            "--check" => if std::fs::read_to_string(&path).map_err(|e| e.to_string())?.replace("\r\n", "\n") != content {
                return Err(format!("generated resolution contract stale: {}", path.display()));
            },
            _ => return Err("usage: gen-video-resolution-contracts --write|--check".into()),
        }
    }
    Ok(())
}
