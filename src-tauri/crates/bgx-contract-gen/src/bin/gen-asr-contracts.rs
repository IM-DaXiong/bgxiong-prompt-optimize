use std::path::Path;
fn main() -> Result<(), String> {
    let mode = std::env::args().nth(1).unwrap_or("--check".into());
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let content = bgx_contract_gen::asr::typescript(&root)?;
    for (target, content) in [
        (root.join("ui/src/api/asr.generated.ts"), content),
        (
            root.join("runtimes/asr/_shared/asr-contract.json"),
            bgx_contract_gen::asr::exported_contract(&root)?,
        ),
        (
            root.join(bgx_contract_gen::local_gpu_occupancy::TS_TARGET),
            bgx_contract_gen::local_gpu_occupancy::typescript(&root)?,
        ),
    ] {
        match mode.as_str() {
            "--write" => std::fs::write(target, content).map_err(|e| e.to_string())?,
            "--check" => {
                if std::fs::read_to_string(&target)
                    .map_err(|e| e.to_string())?
                    .replace("\r\n", "\n")
                    != content
                {
                    return Err(format!(
                        "ASR generated artifact stale: {}",
                        target.display()
                    ));
                }
            }
            _ => return Err("usage: gen-asr-contracts --check|--write".into()),
        }
    }
    Ok(())
}
