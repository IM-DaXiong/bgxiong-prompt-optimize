//! Load Skill / internal template bytes from the authoritative directories.
//! Digests use `bgx_agent_skills_core::digest` — the same function as the host.
//! Resource paths that escape a package are rejected and never read.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use bgx_agent_skills_core::digest::package_digest;
use bgx_agent_skills_core::frontmatter::{parse_skill_md, FrontmatterOutcome};
use bgx_agent_skills_core::resource_path::parse_resource_ref;
use bgx_agent_skills_core::SKILL_MD;
use bgx_prompt_optimize::{error_codes, FrozenSkillBytes, OptimizeError};
use serde::Serialize;

use crate::error_ui;

pub use bgx_prompt_optimize::{
    CLIP_ENVIRONMENT_PROMPT, OPTIMIZE_IMAGE_PROMPT as HOST_IMAGE,
    OPTIMIZE_SEGMENT as HOST_OPTIMIZE_SEGMENT, OPTIMIZE_STORYBOARD_SHOT as HOST_STORYBOARD,
    VIDEO_PROMPT_PROJECTION as HOST_VIDEO_PROJECTION,
};

pub const TEMPLATE_IMAGE: &str = "image-prompt-optimize";
pub const TEMPLATE_LITERARY: &str = "text-prompt-optimize";
pub const TEMPLATE_VIDEO_EXPRESSION: &str = "video-expression";
pub const TEMPLATE_VIDEO_SOURCE: &str = "video-source-understanding";
pub const TEMPLATE_VIDEO_CONTENT: &str = "video-content-optimize";
pub const TEMPLATE_VIDEO_CONTENT_REVIEW: &str = "video-content-review";
pub const TEMPLATE_VIDEO_EXPRESSION_REVIEW: &str = "video-expression-review";
pub const TEMPLATE_LYRIC_SOURCE: &str = "lyric-source-understanding";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SkillListItem {
    pub stable_id: String,
    pub name: String,
    pub package_digest: String,
    pub hosts: Vec<String>,
    pub display_name: Option<String>,
}

pub fn list_skills(library_root: &Path, host: &str) -> Result<Vec<SkillListItem>, OptimizeError> {
    let mut items = Vec::new();
    // Official selector rule: an empty host string yields an empty list.
    if host.trim().is_empty() {
        return Ok(items);
    }
    if !library_root.is_dir() {
        return Ok(items);
    }
    let mut packages = collect_packages(library_root)?;
    packages.sort();
    for package in packages {
        let frozen = freeze_package(&package)?;
        let hosts = declared_hosts_from_body(&frozen.body);
        if !hosts.iter().any(|h| h == host) {
            continue;
        }
        let name = skill_name_from_body(&frozen.body).unwrap_or_else(|| frozen.stable_id.clone());
        let display = metadata_pair(&frozen.body, "bgx-display-name");
        items.push(SkillListItem {
            stable_id: frozen.stable_id,
            name,
            package_digest: frozen.package_digest,
            hosts,
            display_name: display,
        });
    }
    Ok(items)
}

pub fn freeze_named(library_root: &Path, stable_id: &str) -> Result<FrozenSkillBytes, OptimizeError> {
    let package = library_root.join(stable_id);
    if !package.is_dir() {
        return Err(error_ui::err(
            "SKILL_NOT_FOUND",
            format!("找不到技能包：{stable_id}"),
        ));
    }
    freeze_package(&package)
}

pub fn freeze_package(package_dir: &Path) -> Result<FrozenSkillBytes, OptimizeError> {
    let skill_md = package_dir.join(SKILL_MD);
    if !skill_md.is_file() {
        return Err(error_ui::err(
            "SKILL_NOT_FOUND",
            format!("技能包缺少 {SKILL_MD}"),
        ));
    }
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    collect_package_files(package_dir, package_dir, &mut files)?;
    let body = files
        .get(SKILL_MD)
        .ok_or_else(|| error_ui::err("SKILL_NOT_FOUND", format!("技能包缺少 {SKILL_MD}")))?
        .clone();
    let body_text = String::from_utf8(body).map_err(|_| {
        error_ui::err(
            error_codes::AGENT_SKILL_DIGEST_MISMATCH,
            format!("{SKILL_MD} 不是有效的 UTF-8"),
        )
    })?;
    let digest_pairs: Vec<(&str, &[u8])> = files
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_slice()))
        .collect();
    let package_digest = package_digest(digest_pairs);
    let stable_id = skill_name_from_body(&body_text)
        .unwrap_or_else(|| {
            package_dir
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "skill".into())
        });
    let mut resources = files;
    resources.remove(SKILL_MD);
    Ok(FrozenSkillBytes {
        stable_id,
        package_digest,
        body: body_text,
        resources,
    })
}

/// Use the same language-aware immutable template snapshots as the main program.
pub fn freeze_internal_task(root: &Path, task: &str, language: &str) -> Result<FrozenSkillBytes, OptimizeError> {
    let language = match language {
        "en" => bgx_internal_prompts::Language::En,
        "zh" => bgx_internal_prompts::Language::Zh,
        _ => return Err(error_ui::err(error_codes::INTERNAL_PROMPT_CONFIG, "无效的内部模板语言。")),
    };
    let pack = |error: String| error_ui::err(error_codes::INTERNAL_PROMPT_CONFIG, error);
    let rules = bgx_internal_prompts::freeze_set(root, language, &[task]).map_err(|err| pack(err.to_string()))?;
    let material = FrozenSkillBytes {
        stable_id: task.into(),
        package_digest: rules.digest.clone(),
        body: serde_json::to_string(&rules).map_err(|err| pack(err.to_string()))?,
        resources: BTreeMap::new(),
    };
    bgx_prompt_optimize::template::unpack_rules(&material)?;
    Ok(material)
}

pub fn seed_library_if_empty(library_root: &Path, builtin_root: &Path) -> Result<(), OptimizeError> {
    fs::create_dir_all(library_root).map_err(|error| {
        error_ui::err("SKILL_LIBRARY", format!("无法创建技能库目录：{error}"))
    })?;
    let has_package = collect_packages(library_root).map(|list| !list.is_empty())?;
    if has_package {
        return Ok(());
    }
    copy_builtin_tree(builtin_root, library_root)
}

fn copy_builtin_tree(from: &Path, to: &Path) -> Result<(), OptimizeError> {
    let packages = collect_packages(from)?;
    for package in packages {
        let name = package
            .file_name()
            .ok_or_else(|| error_ui::err("BUILTIN_SKILLS_MISSING", "内置技能目录无效"))?;
        let dest = to.join(name);
        copy_dir_no_escape(&package, &dest)?;
    }
    Ok(())
}

fn copy_dir_no_escape(from: &Path, to: &Path) -> Result<(), OptimizeError> {
    fs::create_dir_all(to)
        .map_err(|error| error_ui::err("SKILL_LIBRARY", format!("无法复制内置技能：{error}")))?;
    let mut files = BTreeMap::new();
    collect_package_files(from, from, &mut files)?;
    for (rel, bytes) in files {
        let dest = to.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                error_ui::err("SKILL_LIBRARY", format!("无法复制内置技能：{error}"))
            })?;
        }
        fs::write(&dest, bytes)
            .map_err(|error| error_ui::err("SKILL_LIBRARY", format!("无法复制内置技能：{error}")))?;
    }
    Ok(())
}

fn collect_packages(root: &Path) -> Result<Vec<PathBuf>, OptimizeError> {
    let mut out = Vec::new();
    let entries = match fs::read_dir(root) {
        Ok(v) => v,
        Err(_) => return Ok(out),
    };
    for entry in entries {
        let entry = entry.map_err(|error| {
            error_ui::err("SKILL_LIBRARY", format!("无法读取技能库：{error}"))
        })?;
        let path = entry.path();
        if path.is_dir() && path.join(SKILL_MD).is_file() {
            out.push(path);
        }
    }
    Ok(out)
}

fn collect_package_files(
    package_root: &Path,
    current: &Path,
    files: &mut BTreeMap<String, Vec<u8>>,
) -> Result<(), OptimizeError> {
    let entries = fs::read_dir(current).map_err(|error| {
        error_ui::err("SKILL_LIBRARY", format!("无法读取技能包：{error}"))
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| {
            error_ui::err("SKILL_LIBRARY", format!("无法读取技能包：{error}"))
        })?;
        let path = entry.path();
        let rel = path
            .strip_prefix(package_root)
            .map_err(|_| error_ui::err("SKILL_PATH_ESCAPE", "技能资源路径逃出了包目录"))?
            .to_string_lossy()
            .replace('\\', "/");
        if path.is_dir() {
            // Directory names must also be valid resource path segments if they would be used as refs.
            if !rel.is_empty() {
                parse_resource_ref(&format!("{rel}/placeholder")).map_err(|_| {
                    error_ui::err(
                        "SKILL_PATH_ESCAPE",
                        format!("资源路径非法，已拒绝读取：{rel}"),
                    )
                })?;
            }
            collect_package_files(package_root, &path, files)?;
            continue;
        }
        parse_resource_ref(&rel).map_err(|_| {
            error_ui::err(
                "SKILL_PATH_ESCAPE",
                format!("资源路径非法，已拒绝读取：{rel}"),
            )
        })?;
        let bytes = fs::read(&path)
            .map_err(|error| error_ui::err("SKILL_LIBRARY", format!("无法读取技能文件：{error}")))?;
        files.insert(rel, bytes);
    }
    Ok(())
}

pub fn declared_hosts_from_body(body: &str) -> Vec<String> {
    metadata_pair(body, "bgx-hosts")
        .map(|raw| {
            raw.split_whitespace()
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn skill_name_from_body(body: &str) -> Option<String> {
    let outcome: FrontmatterOutcome<'_> = parse_skill_md(body.as_bytes());
    let parsed = outcome.parsed?;
    parsed.frontmatter.name.as_str().map(str::to_string)
}

fn metadata_pair(body: &str, key: &str) -> Option<String> {
    let outcome: FrontmatterOutcome<'_> = parse_skill_md(body.as_bytes());
    let parsed = outcome.parsed?;
    parsed
        .frontmatter
        .metadata_pairs
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.clone())
}

/// Used by tests: digest of a package directory via the shared algorithm.
pub fn digest_of_dir(package_dir: &Path) -> Result<String, OptimizeError> {
    Ok(freeze_package(package_dir)?.package_digest)
}

pub fn reject_escaping_resource(package_rel: &str) -> Result<(), OptimizeError> {
    parse_resource_ref(package_rel).map(|_| ()).map_err(|_| {
        error_ui::err(
            "SKILL_PATH_ESCAPE",
            format!("资源路径非法，已拒绝读取：{package_rel}"),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bgx_agent_skills_core::digest::is_package_digest;

    fn repo_builtin(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../src-tauri/resources/builtin-agent-skills")
            .join(name)
    }

    #[test]
    fn digest_of_h3_image_prompt_writing_uses_shared_algorithm() {
        let dir = repo_builtin("h3-image-prompt-writing");
        if !dir.is_dir() {
            panic!("authoritative builtin package missing at {}", dir.display());
        }
        let first = digest_of_dir(&dir).expect("digest");
        let second = digest_of_dir(&dir).expect("digest");
        assert!(is_package_digest(&first), "{first}");
        assert_eq!(first, second);
        let frozen = freeze_package(&dir).unwrap();
        assert!(
            declared_hosts_from_body(&frozen.body)
                .iter()
                .any(|h| h == HOST_IMAGE),
            "h3-image-prompt-writing must stay on the image host"
        );
    }

    #[test]
    fn digest_of_generic_video_prompt_writing_uses_shared_algorithm() {
        let dir = repo_builtin("generic-video-prompt-writing");
        if !dir.is_dir() {
            panic!("authoritative builtin package missing at {}", dir.display());
        }
        let digest = digest_of_dir(&dir).expect("digest");
        assert!(is_package_digest(&digest), "{digest}");
        let frozen = freeze_package(&dir).unwrap();
        assert!(
            declared_hosts_from_body(&frozen.body)
                .iter()
                .any(|h| h == HOST_VIDEO_PROJECTION),
            "generic-video-prompt-writing must stay on the video projection host"
        );
    }

    #[test]
    fn escaping_resource_paths_are_rejected() {
        let err = reject_escaping_resource("../secret.txt").expect_err("escape");
        assert!(err.message.contains("secret.txt") || err.message.contains("非法"), "{}", err.message);
        let err = reject_escaping_resource("/tmp/x").expect_err("absolute");
        assert!(!err.message.is_empty());
        reject_escaping_resource("prompts/method.en.md").expect("safe relative path");
    }

    #[test]
    fn qwen_image_keeps_image_host_even_with_a_second_host() {
        let dir = repo_builtin("qwen-image-2-1-prompt-writing");
        if !dir.is_dir() {
            return;
        }
        let frozen = freeze_package(&dir).unwrap();
        let hosts = declared_hosts_from_body(&frozen.body);
        assert!(hosts.iter().any(|h| h == HOST_IMAGE), "{hosts:?}");
        assert!(
            hosts.iter().any(|h| h == CLIP_ENVIRONMENT_PROMPT),
            "qwen also declares the second host; the image filter must still keep it: {hosts:?}"
        );
        let listed = list_skills(dir.parent().expect("builtin root"), HOST_IMAGE).expect("list");
        assert!(
            listed.iter().any(|item| item.name == "qwen-image-2-1-prompt-writing"
                || item.stable_id == "qwen-image-2-1-prompt-writing"),
            "image picker must not drop a package that also declares clip-environment-prompt: {listed:?}"
        );
    }

    #[test]
    fn empty_host_filter_yields_no_skills() {
        let dir = repo_builtin("h3-image-prompt-writing");
        if !dir.is_dir() {
            return;
        }
        let listed = list_skills(dir.parent().expect("builtin root"), "").expect("list");
        assert!(listed.is_empty(), "an empty host string must not invent a selector: {listed:?}");
    }
}
