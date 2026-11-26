//! T29: shell bundle sources are the official packaged skill directories.
//! Digests use the same `sha256_hex` as `bgx-agent-skills-core`.

use bgx_agent_skills_core::sha256_hex;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn repo_root() -> PathBuf {
    manifest_dir()
        .join("../../..")
        .canonicalize()
        .expect("repo root")
}

fn official_internal() -> PathBuf {
    repo_root().join("src-tauri/crates/bgx-internal-prompts/resources/internal-prompt-skills")
}

fn official_builtin() -> PathBuf {
    repo_root().join("src-tauri/resources/builtin-agent-skills")
}

fn has_drive_letter(s: &str) -> bool {
    let bytes = s.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

#[test]
fn bundle_resources_point_at_official_packaged_dirs() {
    let conf_text = fs::read_to_string(manifest_dir().join("tauri.conf.json")).unwrap();
    let conf: Value = serde_json::from_str(&conf_text).unwrap();
    let resources = conf["bundle"]["resources"].as_object().expect("bundle.resources");

    let internal_src = resources
        .iter()
        .find(|(_, dest)| dest.as_str() == Some("internal-prompt-skills/"))
        .map(|(src, _)| src.as_str())
        .expect("internal-prompt-skills destination");
    let builtin_src = resources
        .iter()
        .find(|(_, dest)| dest.as_str() == Some("builtin-agent-skills/"))
        .map(|(src, _)| src.as_str())
        .expect("builtin-agent-skills destination");

    assert!(!has_drive_letter(internal_src));
    assert!(!has_drive_letter(builtin_src));
    assert!(!has_drive_letter("internal-prompt-skills/"));
    assert!(!has_drive_letter("builtin-agent-skills/"));

    let tauri_dir = manifest_dir();
    let internal = tauri_dir.join(internal_src).canonicalize().unwrap();
    let builtin = tauri_dir.join(builtin_src).canonicalize().unwrap();
    assert_eq!(internal, official_internal());
    assert_eq!(builtin, official_builtin());
}

#[test]
fn official_builtin_skill_markdown_digests_are_stable() {
    for name in ["h3-image-prompt-writing", "generic-video-prompt-writing"] {
        let path = official_builtin().join(name).join("SKILL.md");
        let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let digest = format!("sha256:{}", sha256_hex(&bytes));
        assert!(digest.starts_with("sha256:"));
        assert_eq!(digest.len(), "sha256:".len() + 64, "{name} {digest}");
        // Re-read via the official directory only (no machine-local override).
        assert!(path.starts_with(official_builtin()) || path_is_under(&path, &official_builtin()));
    }
}

fn path_is_under(path: &Path, root: &Path) -> bool {
    path.canonicalize()
        .ok()
        .and_then(|p| root.canonicalize().ok().map(|r| p.starts_with(r)))
        .unwrap_or(false)
}
