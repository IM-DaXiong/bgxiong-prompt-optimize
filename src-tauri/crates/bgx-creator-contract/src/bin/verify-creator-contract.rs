//! CT07: `verify-creator-contract --root <repo> [--report <file>]`. Exit 0 = every contract and
//! generator input verified; non-zero = typed violation printed as JSON on stderr. File I/O lives
//! only in this CLI and the report module; no machine path is baked in.

use std::path::PathBuf;
use std::process::ExitCode;

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let Some(root) = arg(&args, "--root") else {
        eprintln!("usage: verify-creator-contract --root <repo-root> [--report <file>]");
        return ExitCode::from(2);
    };
    let root = PathBuf::from(root);
    match bgx_creator_contract::verify_root(&root) {
        Ok(report) => {
            let json = match serde_json::to_string_pretty(&report) {
                Ok(j) => j,
                Err(e) => {
                    eprintln!("REPORT_SERIALIZE: {e}");
                    return ExitCode::from(1);
                }
            };
            if let Some(out) = arg(&args, "--report") {
                if let Err(e) = std::fs::write(&out, format!("{json}\n")) {
                    eprintln!("REPORT_IO: {e}");
                    return ExitCode::from(1);
                }
            }
            println!(
                "creator-contract PASS examples={} exampleSchemas={} schemas={} catalogTools={} inputs={}",
                report.counts.examples,
                report.counts.example_schemas,
                report.counts.schemas,
                report.counts.catalog_tools,
                report.inputs.len()
            );
            ExitCode::SUCCESS
        }
        Err(v) => {
            eprintln!("{}", serde_json::to_string(&v).unwrap_or_else(|_| v.to_string()));
            eprintln!("creator-contract FAIL {v}");
            ExitCode::from(1)
        }
    }
}