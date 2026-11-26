use std::fs;
use std::path::{Path, PathBuf};

use crate::error::PromptError;
use crate::limits::MAX_FILE_BYTES;

pub fn require_directory(root: &Path) -> Result<(), PromptError> {
    if !root.is_absolute() {
        return Err(PromptError::new(
            "root-not-absolute",
            "the resource root must be an absolute path",
        ));
    }
    let meta = fs::symlink_metadata(root).map_err(|_| {
        PromptError::new("root-unavailable", "the resource root cannot be inspected")
    })?;
    if meta.file_type().is_symlink() {
        return Err(PromptError::new(
            "root-is-link",
            "the resource root must not be a link",
        ));
    }
    if !meta.is_dir() {
        return Err(PromptError::new(
            "root-not-directory",
            "the resource root is not a directory",
        ));
    }
    Ok(())
}

/// `raw` is relative to the task directory and uses `/` only.
pub fn normalize_resource(task: &str, raw: &str) -> Result<String, PromptError> {
    if raw.is_empty() || raw.contains('\\') || raw.contains(':') || raw.starts_with('/') {
        return Err(PromptError::new("path-rejected", raw));
    }
    let mut parts = vec![task.to_owned()];
    for segment in raw.split('/') {
        match segment {
            "" => return Err(PromptError::new("path-rejected", raw)),
            "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err(PromptError::new("path-escape", raw));
                }
            }
            _ => parts.push(segment.to_owned()),
        }
    }
    if parts.is_empty() || parts.iter().any(|part| part.is_empty()) {
        return Err(PromptError::new("path-escape", raw));
    }
    Ok(parts.join("/"))
}

pub fn rooted(root: &Path, relative: &str) -> Result<PathBuf, PromptError> {
    if relative.is_empty()
        || relative.contains('\\')
        || relative.contains(':')
        || relative.starts_with('/')
        || relative.split('/').any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(PromptError::new("path-rejected", relative));
    }
    let mut path = root.to_path_buf();
    for part in relative.split('/') {
        path.push(part);
    }
    Ok(path)
}

pub fn read_text(root: &Path, relative: &str) -> Result<String, PromptError> {
    let path = rooted(root, relative)?;
    let meta = fs::symlink_metadata(&path)
        .map_err(|_| PromptError::new("cannot-read", relative))?;
    if meta.file_type().is_symlink() {
        return Err(PromptError::new("path-is-link", relative));
    }
    if !meta.is_file() {
        return Err(PromptError::new("not-a-file", relative));
    }
    if meta.len() > MAX_FILE_BYTES {
        return Err(PromptError::new("file-too-large", relative));
    }
    let bytes = fs::read(&path).map_err(|_| PromptError::new("cannot-read", relative))?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(PromptError::new("file-too-large", relative));
    }
    let again = fs::symlink_metadata(&path)
        .map_err(|_| PromptError::new("cannot-read", relative))?;
    if again.file_type().is_symlink() || !again.is_file() {
        return Err(PromptError::new("path-is-link", relative));
    }
    String::from_utf8(bytes).map_err(|_| PromptError::new("not-utf8", relative))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn a_file_edited_after_the_first_read_is_rejected() {
        let root = std::env::temp_dir().join(format!("bgx-ip-change-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("sample")).unwrap();
        fs::write(root.join("sample/note.md"), "first").unwrap();
        let mut files = BTreeMap::new();
        files.insert("sample/note.md".into(), read_text(&root, "sample/note.md").unwrap());
        fs::write(root.join("sample/note.md"), "second").unwrap();
        let error = reject_if_changed(&root, &files).unwrap_err();
        assert_eq!(error.reason, "rules-changed");
        let _ = fs::remove_dir_all(&root);
    }
}

pub fn reject_if_changed(
    root: &Path,
    files: &std::collections::BTreeMap<String, String>,
) -> Result<(), PromptError> {
    for (relative, text) in files {
        if &read_text(root, relative)? != text {
            return Err(PromptError::new(
                "rules-changed",
                format!("{relative} changed while it was being read"),
            ));
        }
    }
    Ok(())
}
