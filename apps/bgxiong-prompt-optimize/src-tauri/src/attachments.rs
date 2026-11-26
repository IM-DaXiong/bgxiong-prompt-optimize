//! Only paths returned by the system file dialog may be read. Paths that appear
//! only in a model reply are never opened.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use bgx_prompt_optimize::OptimizeError;

use crate::error_ui;

#[derive(Debug, Default)]
pub struct PickedPathSet {
    inner: Mutex<HashSet<PathBuf>>,
}

impl PickedPathSet {
    pub fn remember(&self, paths: &[PathBuf]) {
        if let Ok(mut guard) = self.inner.lock() {
            for path in paths {
                guard.insert(normalize(path));
            }
        }
    }

    pub fn contains(&self, path: &Path) -> bool {
        self.inner
            .lock()
            .map(|guard| guard.contains(&normalize(path)))
            .unwrap_or(false)
    }
}

fn normalize(path: &Path) -> PathBuf {
    path.components().collect()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReferencePick {
    pub path: String,
    pub media_kind: String,
    pub usage: String,
}

pub fn require_picked_path(allow: &PickedPathSet, path: &Path) -> Result<(), OptimizeError> {
    if !path.is_absolute() {
        return Err(error_ui::err(
            "ATTACHMENT_PATH_DENIED",
            "只读取系统文件框返回的绝对路径。",
        ));
    }
    if !allow.contains(path) {
        return Err(error_ui::err(
            "ATTACHMENT_PATH_DENIED",
            "只读取系统文件框返回的路径，模型回包里的路径不会被打开。",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_reply_paths_are_not_read() {
        let set = PickedPathSet::default();
        let err = require_picked_path(&set, Path::new("/tmp/from-model.png")).unwrap_err();
        assert!(err.message.contains("文件框") || err.message.contains("回包"), "{}", err.message);
    }
}
