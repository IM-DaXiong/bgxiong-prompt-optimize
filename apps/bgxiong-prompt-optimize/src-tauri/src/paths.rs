use crate::error_ui;
use bgx_prompt_optimize::OptimizeError;
pub use prompt_optimize_runtime::paths::*;
use std::{fs, path::PathBuf};
use tauri::{AppHandle, Manager};

pub fn shell_app_data_dir(app: &AppHandle) -> Result<PathBuf, OptimizeError> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|error| {
            error_ui::err("SHELL_APP_DATA", format!("无法定位本程序数据目录：{error}"))
        })?
        .join(APP_DATA_DIR_NAME);
    reject_host_pointers(&dir)?;
    fs::create_dir_all(&dir).map_err(|error| {
        error_ui::err("SHELL_APP_DATA", format!("无法创建本程序数据目录：{error}"))
    })?;
    Ok(dir)
}

pub fn config_path(app: &AppHandle) -> Result<PathBuf, OptimizeError> {
    Ok(shell_app_data_dir(app)?.join(CONFIG_FILE_NAME))
}

pub fn default_skill_library_dir(app: &AppHandle) -> Result<PathBuf, OptimizeError> {
    Ok(shell_app_data_dir(app)?.join(SKILL_LIBRARY_DIR_NAME))
}

pub fn resource_dir(app: &AppHandle) -> Option<PathBuf> {
    app.path().resource_dir().ok()
}

pub fn exe_dir(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .resource_dir()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .or_else(|| {
            std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        })
}
