//! Independent shell paths. Never read the host `app_data_root.txt` or `story.db`.
//!
//! T29 resource table (relative to this crate's `tauri.conf.json`, i.e.
//! `apps/bgxiong-prompt-optimize/src-tauri/`):
//!   ../../../src-tauri/crates/bgx-internal-prompts/resources/internal-prompt-skills/ -> internal-prompt-skills/
//!   ../../../src-tauri/resources/builtin-agent-skills/ -> builtin-agent-skills/
//! Destinations contain no development-machine drive letters. Skills are not
//! copied into `apps/**` git.

use std::fs;
use std::path::{Path, PathBuf};

use bgx_prompt_optimize::{error_codes, OptimizeError};

use crate::error_ui;

pub const APP_DATA_DIR_NAME: &str = "bgxiong-prompt-optimize";
pub const CONFIG_FILE_NAME: &str = "config.json";
pub const SKILL_LIBRARY_DIR_NAME: &str = "agent_skills";
const HOST_APP_DATA_ROOT_FILE: &str = "app_data_root.txt";
const HOST_STORY_DB: &str = "story.db";
const INTERNAL_PROMPT_ENV: &str = "BGXIONG_INTERNAL_PROMPT_ROOT";
const INTERNAL_DIR_NAME: &str = "internal-prompt-skills";
const BUILTIN_DIR_NAME: &str = "builtin-agent-skills";

/// Refuse to treat the host's data-root pointer or story database as implicit inputs.
pub fn reject_host_pointers(dir: &Path) -> Result<(), OptimizeError> {
    let root_file = dir.join(HOST_APP_DATA_ROOT_FILE);
    if root_file.is_file() {
        return Err(error_ui::err(
            "SHELL_HOST_DATA_REFUSED",
            "独立壳不读取主程序 app_data_root.txt。如需使用已有技能库，请在设置里填写绝对路径。",
        ));
    }
    let db = dir.join(HOST_STORY_DB);
    if db.is_file() {
        return Err(error_ui::err(
            "SHELL_STORY_DB_REFUSED",
            "独立壳不打开 story.db。",
        ));
    }
    Ok(())
}

pub fn require_absolute(path: &Path, what: &str) -> Result<PathBuf, OptimizeError> {
    if !path.is_absolute() {
        return Err(error_ui::err(
            error_codes::INTERNAL_PROMPT_CONFIG,
            format!("{what} 必须是绝对路径"),
        ));
    }
    Ok(path.to_path_buf())
}

/// Template root: env (absolute only) → user config → exe/resource `internal-prompt-skills`.
pub fn resolve_internal_prompt_root(
    configured: Option<&Path>,
    resource_dir: Option<&Path>,
    exe_dir: Option<&Path>,
) -> Result<PathBuf, OptimizeError> {
    if let Some(value) = std::env::var_os(INTERNAL_PROMPT_ENV) {
        let root = PathBuf::from(value);
        if !root.is_absolute() {
            return Err(error_ui::err(
                error_codes::INTERNAL_PROMPT_CONFIG,
                "INTERNAL_PROMPT_CONFIG: BGXIONG_INTERNAL_PROMPT_ROOT must be absolute",
            ));
        }
        return require_real_dir(&root);
    }
    if let Some(configured) = configured {
        let root = require_absolute(configured, "内部模板根")?;
        return require_real_dir(&root);
    }
    if let Some(resource_dir) = resource_dir {
        let candidate = resource_dir.join(INTERNAL_DIR_NAME);
        if candidate.is_dir() {
            return require_real_dir(&candidate);
        }
    }
    if let Some(exe_dir) = exe_dir {
        let candidate = exe_dir.join(INTERNAL_DIR_NAME);
        if candidate.is_dir() {
            return require_real_dir(&candidate);
        }
    }
    Err(error_ui::err(
        error_codes::INTERNAL_PROMPT_CONFIG,
        "INTERNAL_PROMPT_CONFIG: BGXIONG_INTERNAL_PROMPT_ROOT is unset and current.json is missing",
    ))
}

/// If the installed tree uses `current.json` + `generations/<hex>`, follow it.
/// A direct package tree (dev resource copy) is used as-is.
pub fn effective_internal_generation(root: &Path) -> Result<PathBuf, OptimizeError> {
    let pointer = root.join("current.json");
    if !pointer.is_file() {
        return Ok(root.to_path_buf());
    }
    let text = fs::read_to_string(&pointer).map_err(|_| {
        error_ui::err(
            error_codes::INTERNAL_PROMPT_CONFIG,
            "INTERNAL_PROMPT_CONFIG: current.json is not the installed pointer",
        )
    })?;
    let value: serde_json::Value = serde_json::from_str(&text).map_err(|_| {
        error_ui::err(
            error_codes::INTERNAL_PROMPT_CONFIG,
            "INTERNAL_PROMPT_CONFIG: current.json is not the installed pointer",
        )
    })?;
    let digest = value
        .get("digest")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            error_ui::err(
                error_codes::INTERNAL_PROMPT_CONFIG,
                "INTERNAL_PROMPT_CONFIG: current.json has no digest",
            )
        })?;
    let hex = digest.strip_prefix("sha256:").ok_or_else(|| {
        error_ui::err(
            error_codes::INTERNAL_PROMPT_CONFIG,
            "INTERNAL_PROMPT_CONFIG: current.json digest is not sha256",
        )
    })?;
    if hex.len() != 64 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(error_ui::err(
            error_codes::INTERNAL_PROMPT_CONFIG,
            "INTERNAL_PROMPT_CONFIG: current.json digest is not sha256",
        ));
    }
    require_real_dir(&root.join("generations").join(hex))
}

pub fn resolve_builtin_skills_root(
    resource_dir: Option<&Path>,
    exe_dir: Option<&Path>,
) -> Result<PathBuf, OptimizeError> {
    if let Some(resource_dir) = resource_dir {
        let candidate = resource_dir.join(BUILTIN_DIR_NAME);
        if candidate.is_dir() {
            return require_real_dir(&candidate);
        }
    }
    if let Some(exe_dir) = exe_dir {
        let candidate = exe_dir.join(BUILTIN_DIR_NAME);
        if candidate.is_dir() {
            return require_real_dir(&candidate);
        }
    }
    Err(error_ui::err(
        "BUILTIN_SKILLS_MISSING",
        "找不到内置技能包。请确认安装资源已包含 builtin-agent-skills。",
    ))
}

fn require_real_dir(root: &Path) -> Result<PathBuf, OptimizeError> {
    let meta = fs::symlink_metadata(root).map_err(|_| {
        error_ui::err(
            error_codes::INTERNAL_PROMPT_CONFIG,
            "INTERNAL_PROMPT_CONFIG: internal prompt root cannot be inspected",
        )
    })?;
    if meta.file_type().is_symlink() {
        return Err(error_ui::err(
            error_codes::INTERNAL_PROMPT_CONFIG,
            "INTERNAL_PROMPT_CONFIG: internal prompt root must not be a link",
        ));
    }
    if !meta.is_dir() {
        return Err(error_ui::err(
            error_codes::INTERNAL_PROMPT_CONFIG,
            "INTERNAL_PROMPT_CONFIG: internal prompt root is not a directory",
        ));
    }
    Ok(root.to_path_buf())
}

/// T29: destination keys in the shell resource table must not contain a drive letter.
pub fn bundled_resource_destinations() -> &'static [(&'static str, &'static str)] {
    &[
        (
            "../../../src-tauri/crates/bgx-internal-prompts/resources/internal-prompt-skills/",
            "internal-prompt-skills/",
        ),
        (
            "../../../src-tauri/resources/builtin-agent-skills/",
            "builtin-agent-skills/",
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn env_relative_internal_root_is_rejected() {
        let previous = std::env::var_os(INTERNAL_PROMPT_ENV);
        std::env::set_var(INTERNAL_PROMPT_ENV, "relative/skills");
        let result = resolve_internal_prompt_root(None, None, None);
        match previous {
            Some(v) => std::env::set_var(INTERNAL_PROMPT_ENV, v),
            None => std::env::remove_var(INTERNAL_PROMPT_ENV),
        }
        let err = result.expect_err("relative env must fail");
        assert_eq!(err.code, error_codes::INTERNAL_PROMPT_CONFIG);
        assert!(err.message.contains("must be absolute"), "{}", err.message);
    }

    #[test]
    fn bundled_destinations_have_no_drive_letter() {
        for (src, dest) in bundled_resource_destinations() {
            assert!(!has_drive_letter(src), "source should be repo-relative: {src}");
            assert!(!has_drive_letter(dest), "destination must not contain a drive letter: {dest}");
            assert!(!dest.contains(':'), "{dest}");
        }
    }

    fn has_drive_letter(s: &str) -> bool {
        let b = s.as_bytes();
        b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
    }

    #[test]
    fn current_json_generation_is_followed() {
        let dir = tempfile_dir();
        let hex = "a".repeat(64);
        let gen = dir.join("generations").join(&hex);
        fs::create_dir_all(&gen).unwrap();
        fs::write(
            dir.join("current.json"),
            format!(r#"{{"digest":"sha256:{hex}"}}"#),
        )
        .unwrap();
        let resolved = effective_internal_generation(&dir).unwrap();
        assert_eq!(resolved, gen);
        let _ = fs::remove_dir_all(&dir);
    }

    fn tempfile_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "bgx-prompt-opt-paths-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}
