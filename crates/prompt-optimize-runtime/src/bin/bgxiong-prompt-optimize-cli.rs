use std::process::ExitCode;

fn main() -> ExitCode {
    if std::env::args().skip(1).collect::<Vec<_>>() != ["--stdio"] {
        eprintln!("Usage: bgxiong-prompt-optimize-cli --stdio");
        return ExitCode::from(2);
    }
    match prompt_optimize_runtime::stdio::run_stdio() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Prompt optimize stdio: {error}");
            ExitCode::FAILURE
        }
    }
}
