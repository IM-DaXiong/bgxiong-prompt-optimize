//! Local job cancel flag. TextCall.check_cancel is a function pointer, so the
//! current job uses a process-wide atomic rather than a capturing closure.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use bgx_prompt_optimize::{error_codes, OptimizeError};
use uuid::Uuid;

use crate::error_ui;

static CANCELLED: AtomicBool = AtomicBool::new(false);
static CURRENT_JOB: OnceLock<Mutex<Option<String>>> = OnceLock::new();

fn current_job() -> &'static Mutex<Option<String>> {
    CURRENT_JOB.get_or_init(|| Mutex::new(None))
}

pub fn start_job() -> Result<String, OptimizeError> {
    let id = Uuid::new_v4().to_string();
    let mut guard = current_job()
        .lock()
        .map_err(|_| error_ui::err("OPTIMIZE_CANCEL_LOCK", "无法读取当前任务状态"))?;
    if guard.is_some() {
        return Err(error_ui::err("BUSY", "已有提示词正在优化。"));
    }
    CANCELLED.store(false, Ordering::SeqCst);
    *guard = Some(id.clone());
    Ok(id)
}

pub fn finish_job(job_id: &str) {
    if let Ok(mut guard) = current_job().lock() {
        if guard.as_deref() == Some(job_id) {
            *guard = None;
            CANCELLED.store(false, Ordering::SeqCst);
        }
    }
}

pub fn request_cancel(job_id: &str) -> Result<(), OptimizeError> {
    let guard = current_job()
        .lock()
        .map_err(|_| error_ui::err("OPTIMIZE_CANCEL_LOCK", "无法读取当前任务状态"))?;
    if guard.as_deref() != Some(job_id) {
        return Err(error_ui::err(
            "OPTIMIZE_JOB_NOT_FOUND",
            "没有找到这个正在进行的优化任务",
        ));
    }
    CANCELLED.store(true, Ordering::SeqCst);
    Ok(())
}

pub fn is_cancelled() -> bool {
    CANCELLED.load(Ordering::SeqCst)
}

pub fn check_cancel() -> Result<(), OptimizeError> {
    if is_cancelled() {
        Err(OptimizeError::new(
            error_codes::OPTIMIZE_CANCELLED,
            "已取消",
        ))
    } else {
        Ok(())
    }
}
