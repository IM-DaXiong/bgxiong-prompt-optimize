//! Explicit adapter-package read port owned by the shell.
//!
//! `bgx-text-http` is an `http-client` crate and must not use `std::fs`.
//! The shell reads the package tree here, then hands already-read bytes to
//! `MemoryPackageFileReader`. `package_root` remains an identity argument on
//! the frozen sync `adapter::complete`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use bgx_prompt_optimize::OptimizeError;

use crate::error_ui;
use crate::paths;

pub struct AdapterPackageBytes {
    pub root: PathBuf,
    pub files: BTreeMap<String, Vec<u8>>,
}

pub fn read_adapter_package(root: &Path) -> Result<AdapterPackageBytes, OptimizeError> {
    let root = paths::require_absolute(root, "适配包目录")?;
    let meta = fs::symlink_metadata(&root).map_err(|_| {
        error_ui::err("ADAPTER_PACKAGE_ROOT_REQUIRED", "未选择模型 / 找不到包")
    })?;
    if meta.file_type().is_symlink() {
        return Err(error_ui::err(
            "ADAPTER_PACKAGE_PATH",
            "适配包目录不能是链接。",
        ));
    }
    if !meta.is_dir() {
        return Err(error_ui::err(
            "ADAPTER_PACKAGE_ROOT_REQUIRED",
            "未选择模型 / 找不到包",
        ));
    }
    let mut files = BTreeMap::new();
    collect(&root, &root, &mut files)?;
    if files.is_empty() {
        return Err(error_ui::err(
            "ADAPTER_PACKAGE_ROOT_REQUIRED",
            "未选择模型 / 找不到包",
        ));
    }
    Ok(AdapterPackageBytes { root, files })
}

/// Keys are the absolute paths the frozen crate will pass to `PackageFileReader`.
pub fn memory_reader(package: &AdapterPackageBytes) -> bgx_text_http::MemoryPackageFileReader {
    let mut reader = bgx_text_http::MemoryPackageFileReader::new();
    for (rel, bytes) in &package.files {
        let mut full = package.root.clone();
        for seg in rel.split(['/', '\\']) {
            if seg.is_empty() || seg == "." {
                continue;
            }
            full.push(seg);
        }
        reader.insert(&full, bytes.clone());
    }
    reader
}

fn collect(
    package_root: &Path,
    current: &Path,
    files: &mut BTreeMap<String, Vec<u8>>,
) -> Result<(), OptimizeError> {
    let entries = fs::read_dir(current).map_err(|error| {
        error_ui::err("ADAPTER_PACKAGE_READ", format!("无法读取适配包：{error}"))
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| {
            error_ui::err("ADAPTER_PACKAGE_READ", format!("无法读取适配包：{error}"))
        })?;
        let path = entry.path();
        let rel = path
            .strip_prefix(package_root)
            .map_err(|_| error_ui::err("ADAPTER_PACKAGE_PATH", "适配包路径逃出了配置的目录"))?
            .to_string_lossy()
            .replace('\\', "/");
        if rel.split('/').any(|seg| seg == "." || seg == "..") {
            return Err(error_ui::err(
                "ADAPTER_PACKAGE_PATH",
                format!("适配包路径非法，已拒绝读取：{rel}"),
            ));
        }
        if path.is_dir() {
            collect(package_root, &path, files)?;
            continue;
        }
        let bytes = fs::read(&path).map_err(|error| {
            error_ui::err("ADAPTER_PACKAGE_READ", format!("无法读取适配包文件：{error}"))
        })?;
        files.insert(rel, bytes);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_and_escaping_roots_are_rejected() {
        let err = match read_adapter_package(Path::new("relative/pack")) {
            Err(error) => error,
            Ok(_) => panic!("relative adapter root must be rejected"),
        };
        assert!(err.message.contains("绝对路径"), "{}", err.message);
    }
}
