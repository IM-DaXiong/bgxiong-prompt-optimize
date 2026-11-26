use std::env;
use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("validate") => {
            let root = flag(&mut args, "--root")?;
            require_empty(&mut args)?;
            bgx_internal_prompts::validate_root(Path::new(&root)).map_err(|error| error.to_string())
        }
        Some("stage") => {
            let source = flag(&mut args, "--source")?;
            let destination = flag(&mut args, "--destination")?;
            require_empty(&mut args)?;
            let report = bgx_internal_prompts::stage_root(Path::new(&source), Path::new(&destination))
                .map_err(|error| error.to_string())?;
            println!("{}", report.digest);
            Ok(())
        }
        _ => Err(
            "usage: internal-prompts validate --root <absolute> | stage --source <absolute> --destination <absolute>"
                .into(),
        ),
    }
}

fn flag(args: &mut impl Iterator<Item = String>, name: &str) -> Result<String, String> {
    match args.next().as_deref() {
        Some(value) if value == name => args
            .next()
            .ok_or_else(|| format!("missing value for {name}")),
        Some(other) => Err(format!("expected {name}, got {other}")),
        None => Err(format!("missing {name}")),
    }
}

fn require_empty(args: &mut impl Iterator<Item = String>) -> Result<(), String> {
    if args.next().is_some() {
        return Err("unexpected extra argument".into());
    }
    Ok(())
}
