use std::path::Path;
fn main() -> Result<(), String> {
    let mode = std::env::args().nth(1).unwrap_or("--check".into());
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let generated = bgx_contract_gen::comfy_prompt_origin::generate(&root)?;
    let path = root.join("ui/src/api/comfyPromptOrigin.generated.ts");
    match mode.as_str() {
        "--write" => std::fs::write(path, generated.typescript).map_err(|e| e.to_string())?,
        "--check" => if std::fs::read_to_string(&path).map_err(|e| e.to_string())?.replace("\r\n", "\n") != generated.typescript {
            return Err(format!("generated prompt origin contract stale: {}", path.display()));
        },
        _ => return Err("usage: gen-comfy-prompt-origin-contract --write|--check".into()),
    }
    Ok(())
}
