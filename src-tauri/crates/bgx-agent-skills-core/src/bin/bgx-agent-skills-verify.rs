//! `bgx-agent-skills-verify`: read a cases JSON document on stdin, verify every case with the
//! core verifier, print path/field diagnostics and a summary line.
//! Exit code 0 = at least one case and all pass; 1 = failing case, zero cases or bad input.
//! Used by `scripts/check-agent-skills-format.ps1` (P06). No file system or process access
//! (pure-core crate role), so the failure exit code comes from `main` returning `Err`.

use std::fmt;
use std::io::Read;

struct Failure(String);

impl fmt::Debug for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

fn main() -> Result<(), Failure> {
    let mut input = String::new();
    std::io::stdin()
        .read_to_string(&mut input)
        .map_err(|e| Failure(format!("stdin is not UTF-8 text: {e}")))?;
    let summary = bgx_agent_skills_core::verify::run_cases_json(&input);
    print!("{}", summary.render());
    if summary.ok() {
        Ok(())
    } else {
        Err(Failure(summary.summary_line()))
    }
}
