use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use bgx_agent_skills_core::{frontmatter::parse_skill_md, SKILL_MD};
use bgx_prompt_optimize::sdk::target_catalog;
use bgx_prompt_optimize::{FrozenSkillBytes, OptimizeError};
use bgx_skill_distribution::catalog::{
    parse_catalog, Catalog, Release, OFFICIAL_INDEX, OFFICIAL_KEY, OFFICIAL_KEY_ID,
};
use bgx_skill_distribution::{
    archive::ReadPackage, index::accept_sequence, is_sha256, package::verify_zip,
    DistributionError, INDEX_MAX_BYTES,
};
use serde::{Deserialize, Serialize};

use crate::{paths, skills};

#[derive(Clone)]
pub struct SkillStore {
    root: PathBuf,
    builtin: Option<PathBuf>,
    user_library: PathBuf,
}

#[derive(Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Installed {
    index_sha256: String,
    archive_sha256: String,
}

type Active = BTreeMap<String, Installed>;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    candidate_id: String,
    package_id: String,
    display_name: String,
    version: String,
    sha256: String,
    source_sha256: String,
    needs_confirm: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallReceipt {
    package_id: String,
    version: String,
    package_digest: String,
}

impl SkillStore {
    pub fn new(
        data_dir: &Path,
        builtin: Option<&Path>,
        user_library: Option<&Path>,
    ) -> Result<Self, OptimizeError> {
        paths::reject_host_pointers(data_dir)?;
        let user_library = match user_library {
            Some(path) => paths::require_absolute(path, "技能库根")?,
            None => data_dir.join(paths::SKILL_LIBRARY_DIR_NAME),
        };
        paths::reject_host_pointers(&user_library)?;
        Ok(Self {
            root: data_dir.join("official-skills"),
            builtin: builtin.map(Path::to_path_buf),
            user_library,
        })
    }

    pub fn resolve(&self, id: &str, host: &str) -> Result<Option<FrozenSkillBytes>, OptimizeError> {
        let skill = self.current(id)?;
        Ok(skill.filter(|skill| {
            skills::declared_hosts_from_body(&skill.body)
                .iter()
                .any(|entry| entry == host)
        }))
    }

    fn current(&self, id: &str) -> Result<Option<FrozenSkillBytes>, OptimizeError> {
        let active = self.load_active()?;
        if let Some(entry) = active.get(id) {
            return self.read_active(id, entry).map(Some);
        }
        self.builtin(id)
    }

    fn builtin(&self, id: &str) -> Result<Option<FrozenSkillBytes>, OptimizeError> {
        let Some(root) = &self.builtin else {
            return Ok(None);
        };
        if !root.join(id).is_dir() {
            return Ok(None);
        }
        skills::freeze_named(root, id).map(Some)
    }

    pub fn check(&self) -> Result<Vec<Candidate>, OptimizeError> {
        let bytes = get(OFFICIAL_INDEX, INDEX_MAX_BYTES)?;
        let (catalog, digest) = decode(&bytes)?;
        let _lock = self.lock()?;
        if let Some(prior) = read_optional(&self.root.join("latest.json"), INDEX_MAX_BYTES)? {
            let (prior, previous_digest) = decode(&prior)?;
            accept_sequence(
                prior.sequence,
                Some(&previous_digest),
                catalog.sequence,
                &digest,
            )
            .map_err(distribution_error)?;
        }
        replace(&self.root.join("latest.json"), &bytes)?;
        self.candidates(&catalog)
    }

    pub fn cached_candidates(&self) -> Result<Vec<Candidate>, OptimizeError> {
        let Some(bytes) = read_optional(&self.root.join("latest.json"), INDEX_MAX_BYTES)? else {
            return Ok(Vec::new());
        };
        self.candidates(&decode(&bytes)?.0)
    }

    fn candidates(&self, catalog: &Catalog) -> Result<Vec<Candidate>, OptimizeError> {
        let targets: BTreeSet<_> = target_catalog()?
            .into_iter()
            .filter_map(|target| target.method_id)
            .collect();
        let host = host_version()?;
        Ok(catalog
            .packages
            .iter()
            .filter(|row| targets.contains(&row.package_id) && row.check_host(&host).is_ok())
            .filter_map(|row| {
                let current = self.current(&row.package_id).ok()?;
                if current.as_ref().is_some_and(|skill| {
                    skill.package_digest == format!("sha256:{}", row.source_sha256)
                }) {
                    return None;
                }
                if self.check_upgrade(row, current.as_ref()).is_err() {
                    return None;
                }
                Some(Candidate {
                    candidate_id: row.candidate_id(),
                    package_id: row.package_id.clone(),
                    display_name: row.display_name.clone(),
                    version: row.version.to_string(),
                    sha256: row.sha256.clone(),
                    source_sha256: row.source_sha256.clone(),
                    needs_confirm: current.is_none(),
                })
            })
            .collect())
    }

    pub fn can_install(&self, candidates: &[Candidate], id: &str) -> bool {
        candidates.iter().any(|row| row.package_id == id)
    }

    pub fn install(&self, id: &str, confirmed: bool) -> Result<InstallReceipt, OptimizeError> {
        let latest_path = self.root.join("latest.json");
        let envelope = read_optional(&latest_path, INDEX_MAX_BYTES)?.ok_or_else(stale)?;
        let (catalog, index_sha256) = decode(&envelope)?;
        let release = catalog
            .packages
            .iter()
            .find(|release| release.candidate_id() == id)
            .ok_or_else(stale)?;
        if !target_catalog()?
            .iter()
            .any(|target| target.method_id.as_deref() == Some(&release.package_id))
        {
            return Err(stale());
        }
        release
            .check_host(&host_version()?)
            .map_err(distribution_error)?;
        let current = self.current(&release.package_id)?;
        let package_digest = format!("sha256:{}", release.source_sha256);
        let receipt = InstallReceipt {
            package_id: release.package_id.clone(),
            version: release.version.to_string(),
            package_digest: package_digest.clone(),
        };
        if current
            .as_ref()
            .is_some_and(|old| old.package_digest == package_digest)
        {
            return Ok(receipt);
        }
        self.check_upgrade(release, current.as_ref())?;
        if current.is_none() && !confirmed {
            return Err(OptimizeError::new(
                "ONLINE_INSTALL_CONFIRMATION_REQUIRED",
                "首次安装需要确认。",
            ));
        }
        let url = reqwest::Url::parse(OFFICIAL_INDEX)
            .and_then(|address| address.join(&release.file))
            .map_err(|_| stale())?;
        let bytes = get(url.as_str(), release.bytes)?;
        let read = verify_zip(release, &bytes).map_err(distribution_error)?;
        let skill = frozen(release, read)?;
        validate_methods(current.as_ref(), &skill)?;

        let _lock = self.lock()?;
        let latest = read_optional(&latest_path, INDEX_MAX_BYTES)?.ok_or_else(stale)?;
        if decode(&latest)?.1 != index_sha256
            || self
                .current(&release.package_id)?
                .map(|skill| skill.package_digest)
                != current.as_ref().map(|skill| skill.package_digest.clone())
        {
            return Err(stale());
        }
        self.check_upgrade(release, current.as_ref())?;
        let index_file = self
            .root
            .join("indexes")
            .join(format!("{index_sha256}.json"));
        let archive_sha256 = release.sha256.clone();
        let package_file = self
            .root
            .join("packages")
            .join(format!("{archive_sha256}.zip"));
        write_object(&index_file, &envelope)?;
        write_object(&package_file, &bytes)?;
        let mut active = self.load_active()?;
        active.insert(
            release.package_id.clone(),
            Installed {
                index_sha256,
                archive_sha256,
            },
        );
        let next = serde_json::to_vec(&active).map_err(store_error)?;
        replace(&self.root.join("active.json"), &next)?;
        Ok(receipt)
    }

    fn check_upgrade(
        &self,
        release: &Release,
        current: Option<&FrozenSkillBytes>,
    ) -> Result<(), OptimizeError> {
        let local = self.user_library.join(&release.package_id);
        if local.is_dir() {
            let old = skills::freeze_package(&local)?;
            let matches_bundle = self
                .builtin(&release.package_id)?
                .is_some_and(|baseline| baseline.package_digest == old.package_digest);
            if !matches_bundle {
                return Err(OptimizeError::new(
                    "ONLINE_SKILL_NAME_CONFLICT",
                    "已有同名自定义 Skill。",
                ));
            }
        }
        match current {
            Some(old) if !release.allows_upgrade_from(&old.package_digest) => {
                Err(OptimizeError::new(
                    "ONLINE_SKILL_SOURCE_CHANGED",
                    "当前 Skill 与官方升级来源不一致。",
                ))
            }
            None if !release.new_install => Err(OptimizeError::new(
                "ONLINE_INSTALL_CONFIRMATION_REQUIRED",
                "此 Skill 当前不能首次安装。",
            )),
            _ => Ok(()),
        }
    }

    fn read_active(&self, id: &str, entry: &Installed) -> Result<FrozenSkillBytes, OptimizeError> {
        if !is_sha256(&entry.index_sha256) || !is_sha256(&entry.archive_sha256) {
            return Err(store_error("活动 Skill 摘要无效。"));
        }
        let index = self
            .root
            .join("indexes")
            .join(format!("{}.json", entry.index_sha256));
        let (catalog, index_sha256) =
            decode(&read_optional(&index, INDEX_MAX_BYTES)?.ok_or_else(stale)?)?;
        let release = catalog
            .packages
            .iter()
            .find(|row| {
                row.package_id == id
                    && row.sha256 == entry.archive_sha256
                    && entry.index_sha256 == index_sha256
            })
            .ok_or_else(stale)?;
        release
            .check_host(&host_version()?)
            .map_err(distribution_error)?;
        let path = self
            .root
            .join("packages")
            .join(format!("{}.zip", entry.archive_sha256));
        let bytes = read_optional(&path, release.bytes)?.ok_or_else(stale)?;
        frozen(
            release,
            verify_zip(release, &bytes).map_err(distribution_error)?,
        )
    }

    fn load_active(&self) -> Result<Active, OptimizeError> {
        match read_optional(&self.root.join("active.json"), INDEX_MAX_BYTES)? {
            Some(bytes) => serde_json::from_slice(&bytes).map_err(store_error),
            None => Ok(Active::new()),
        }
    }

    fn lock(&self) -> Result<File, OptimizeError> {
        fs::create_dir_all(&self.root).map_err(store_error)?;
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(self.root.join("install.lock"))
            .map_err(store_error)?;
        file.lock().map_err(store_error)?;
        Ok(file)
    }
}

fn validate_methods(
    old: Option<&FrozenSkillBytes>,
    new: &FrozenSkillBytes,
) -> Result<(), OptimizeError> {
    let new_hosts = skills::declared_hosts_from_body(&new.body);
    let allowed_hosts = [
        bgx_prompt_optimize::OPTIMIZE_IMAGE_PROMPT,
        bgx_prompt_optimize::VIDEO_PROMPT_PROJECTION,
    ];
    let prior_hosts = old
        .map(|old| skills::declared_hosts_from_body(&old.body))
        .unwrap_or_default();
    let has_tools = parse_skill_md(new.body.as_bytes())
        .parsed
        .and_then(|parsed| {
            parsed
                .frontmatter
                .allowed_tools
                .as_str()
                .map(|value| !value.trim().is_empty())
        })
        .unwrap_or(false);
    if has_tools
        || !new_hosts
            .iter()
            .any(|host| allowed_hosts.contains(&host.as_str()))
        || prior_hosts.iter().any(|host| !new_hosts.contains(host))
    {
        return Err(OptimizeError::new(
            "ONLINE_SKILL_NEEDS_REVIEW",
            "Skill 的用途或工具权限已变化。",
        ));
    }
    Ok(())
}

fn host_version() -> Result<semver::Version, OptimizeError> {
    let lock: serde_json::Value =
        serde_json::from_str(include_str!("../../../deps/host-inputs.lock.json"))
            .map_err(store_error)?;
    semver::Version::parse(
        lock["hostVersion"]
            .as_str()
            .ok_or_else(|| store_error("缺少宿主兼容版本。"))?,
    )
    .map_err(store_error)
}

fn frozen(release: &Release, read: ReadPackage) -> Result<FrozenSkillBytes, OptimizeError> {
    let mut resources: BTreeMap<_, _> = read
        .files
        .into_iter()
        .map(|file| (file.path, file.bytes))
        .collect();
    let body = resources
        .remove(SKILL_MD)
        .ok_or_else(|| store_error("缺少 SKILL.md。"))?;
    Ok(FrozenSkillBytes {
        stable_id: release.package_id.clone(),
        package_digest: format!("sha256:{}", release.source_sha256),
        body: String::from_utf8(body).map_err(store_error)?,
        resources,
    })
}

fn decode(bytes: &[u8]) -> Result<(Catalog, String), OptimizeError> {
    parse_catalog(bytes, OFFICIAL_KEY_ID, OFFICIAL_KEY).map_err(distribution_error)
}

fn get(url: &str, limit: u64) -> Result<Vec<u8>, OptimizeError> {
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(60))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(download_error)?;
    let response = client
        .get(url)
        .send()
        .and_then(|response| response.error_for_status())
        .map_err(download_error)?;
    if response.status() != reqwest::StatusCode::OK {
        return Err(download_error(response.status()));
    }
    read_bounded(response, limit)
}

fn read_optional(path: &Path, limit: u64) -> Result<Option<Vec<u8>>, OptimizeError> {
    match File::open(path) {
        Ok(file) => read_bounded(file, limit).map(Some),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(store_error(error)),
    }
}

fn read_bounded(reader: impl Read, limit: u64) -> Result<Vec<u8>, OptimizeError> {
    let mut bytes = Vec::new();
    reader
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(store_error)?;
    if bytes.len() as u64 > limit {
        return Err(store_error("文件超过允许的大小。"));
    }
    Ok(bytes)
}

fn write_object(path: &Path, bytes: &[u8]) -> Result<(), OptimizeError> {
    if let Some(prior) = read_optional(path, bytes.len() as u64)? {
        return if prior == bytes {
            Ok(())
        } else {
            Err(store_error("已存对象与摘要不一致。"))
        };
    }
    replace(path, bytes)
}

fn replace(path: &Path, bytes: &[u8]) -> Result<(), OptimizeError> {
    let parent = path
        .parent()
        .ok_or_else(|| store_error("无效的 Skill 目录。"))?;
    fs::create_dir_all(parent).map_err(store_error)?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(store_error)?;
    file.write_all(bytes)
        .and_then(|_| file.as_file().sync_all())
        .map_err(store_error)?;
    file.persist(path).map_err(store_error)?;
    Ok(())
}

fn stale() -> OptimizeError {
    OptimizeError::new(
        "ONLINE_CANDIDATE_STALE",
        "Skill 更新候选已变化，请重新检查。",
    )
}
fn distribution_error(error: DistributionError) -> OptimizeError {
    OptimizeError::new(error.code, error.message)
}
fn store_error(error: impl std::fmt::Display) -> OptimizeError {
    OptimizeError::new("ONLINE_SKILL_STATE", error.to_string())
}
fn download_error(error: impl std::fmt::Display) -> OptimizeError {
    OptimizeError::new("ONLINE_SKILL_DOWNLOAD_FAILED", error.to_string())
}
