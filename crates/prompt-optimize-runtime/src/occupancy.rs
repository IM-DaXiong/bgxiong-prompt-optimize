//! Process-local occupancy for the standalone shell.
//! The host passes `"main"`; this shell passes its window label `prompt-optimize`.

use std::sync::atomic::{AtomicBool, Ordering};

/// Window label from `tauri.conf.json`. This is the occupancy label for local GGUF.
pub const SHELL_OCCUPANCY_LABEL: &str = "prompt-optimize";

static INFER_BUSY: AtomicBool = AtomicBool::new(false);

/// Called by the frozen local exit before inference. Validates the window label
/// and rejects a second overlapping job. The exclusive flag is taken when the
/// shell runtime starts `infer`, not here, because this callback has no release.
pub fn require_occupancy(label: &str) -> Result<(), String> {
    if label != SHELL_OCCUPANCY_LABEL {
        return Err(format!(
            "本地模型占用标签必须是 {SHELL_OCCUPANCY_LABEL}，收到的是 {label}"
        ));
    }
    if INFER_BUSY.load(Ordering::SeqCst) {
        return Err(format!("本地模型正被占用（{SHELL_OCCUPANCY_LABEL}）"));
    }
    Ok(())
}

pub fn try_begin_infer() -> Result<(), String> {
    INFER_BUSY
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .map(|_| ())
        .map_err(|_| format!("本地模型正被占用（{SHELL_OCCUPANCY_LABEL}）"))
}

pub fn end_infer() {
    INFER_BUSY.store(false, Ordering::SeqCst);
}

pub struct InferGuard;

impl InferGuard {
    pub fn acquire() -> Result<Self, String> {
        try_begin_infer()?;
        Ok(Self)
    }
}

impl Drop for InferGuard {
    fn drop(&mut self) {
        end_infer();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foreign_label_is_rejected() {
        let err = require_occupancy("main").unwrap_err();
        assert!(err.contains(SHELL_OCCUPANCY_LABEL), "{err}");
        assert!(err.contains("main"), "{err}");
    }

    #[test]
    fn shell_label_is_accepted_when_idle() {
        // Do not hold InferGuard across tests; just the label check.
        if !INFER_BUSY.load(Ordering::SeqCst) {
            require_occupancy(SHELL_OCCUPANCY_LABEL).unwrap();
        }
    }
}
