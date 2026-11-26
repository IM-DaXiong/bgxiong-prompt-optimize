//! Refresh or check the static-frame TS twin from the same generator build.rs uses.
//! `cargo run --manifest-path src-tauri/crates/bgx-contract-gen/Cargo.toml --bin gen-static-frame-contracts -- --check|--write`
use std::path::Path;

fn main() -> Result<(), String> {
    let mode = std::env::args().nth(1).unwrap_or_else(|| "--check".into());
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let generated = bgx_contract_gen::static_frame::generate(&root)?;
    let target = root.join(bgx_contract_gen::static_frame::TS_TARGET);
    let content = generated.static_frame.typescript;
    match mode.as_str() {
        "--write" => {
            std::fs::create_dir_all(target.parent().ok_or("TS target has no parent")?)
                .map_err(|e| e.to_string())?;
            std::fs::write(&target, content).map_err(|e| e.to_string())
        }
        "--check" => {
            let current = std::fs::read_to_string(&target)
                .map_err(|e| format!("read {}: {e}", target.display()))?
                .replace("\r\n", "\n");
            if current != content {
                return Err(format!(
                    "static-frame TS contract stale: {} (run gen-static-frame-contracts --write)",
                    target.display()
                ));
            }
            Ok(())
        }
        _ => Err("usage: gen-static-frame-contracts --check|--write".into()),
    }
}
