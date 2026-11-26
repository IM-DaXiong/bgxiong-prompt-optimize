//! Writes/checks only the generated TypeScript twin; never builds the application.
fn main() -> Result<(), String> {
    let mode = std::env::args().nth(1).ok_or("expected --write, --check, --stdout or --dump-definitions")?;
    if !matches!(mode.as_str(), "--write" | "--check" | "--stdout" | "--dump-definitions") {
        return Err("expected --write, --check, --stdout or --dump-definitions".into());
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    if mode == "--dump-definitions" {
        let out = std::env::args().nth(2).ok_or("missing output path")?;
        let json = bgx_contract_gen::story_creation::definitions_json(&root.join("packages/app-contracts/v1"))?;
        std::fs::write(&out, &json).map_err(|e| e.to_string())?;
        println!("{}", json.len());
        return Ok(());
    }
    let generated = bgx_contract_gen::story_creation::generate(&root.join("packages/app-contracts/v1"))?;
    let path = root.join(bgx_contract_gen::story_creation::TS_TARGET);
    let clip = bgx_contract_gen::story_creation::clip_typescript(&root.join("packages/app-contracts/v1"))?;
    let clip_path = root.join(bgx_contract_gen::story_creation::CLIP_TS_TARGET);
    if mode == "--write" {
        bgx_contract_gen::write_generated(&clip_path, &clip);
    } else if mode == "--check" && bgx_contract_gen::read_twin_text(&clip_path) != clip {
        return Err("clipWorkflow.ts drift: run gen-story-creation-contracts --write".into());
    }
    if mode == "--stdout" {
        use std::io::Write;
        std::io::stdout().write_all(generated.typescript.as_bytes()).map_err(|e| e.to_string())?;
    } else if mode == "--write" {
        std::fs::create_dir_all(path.parent().ok_or("missing parent")?).map_err(|e| e.to_string())?;
        bgx_contract_gen::write_generated(&path, &generated.typescript);
    } else if bgx_contract_gen::read_twin_text(&path) != generated.typescript {
        return Err("storyCreation.ts drift: run gen-story-creation-contracts --write".into());
    }
    Ok(())
}
