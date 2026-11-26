use std::path::{Path, PathBuf};

use bgx_prompt_optimize::sdk::OptimizeMaterials;
use bgx_prompt_optimize::{FrozenSkillBytes, OptimizeError};

use crate::{config::ShellConfig, paths, skill_store::SkillStore, skills};

pub struct StandaloneMaterials {
    templates: PathBuf,
    language: String,
    pub skills: SkillStore,
}

impl StandaloneMaterials {
    pub fn new(
        config: &ShellConfig,
        data_dir: &Path,
        resource_dir: Option<&Path>,
        exe_dir: Option<&Path>,
    ) -> Result<Self, OptimizeError> {
        let templates = paths::resolve_internal_prompt_root(
            config.internal_template_root.as_deref().map(Path::new),
            resource_dir,
            exe_dir,
        )?;
        let builtin = match paths::resolve_builtin_skills_root(resource_dir, exe_dir) {
            Ok(root) => Some(root),
            Err(error) if error.code == "BUILTIN_SKILLS_MISSING" => None,
            Err(error) => return Err(error),
        };
        let skills = SkillStore::new(
            data_dir,
            builtin.as_deref(),
            config.skill_library_root.as_deref().map(Path::new),
        )?;
        Ok(Self {
            templates: paths::effective_internal_generation(&templates)?,
            language: config.language().into(),
            skills,
        })
    }
}

impl OptimizeMaterials for StandaloneMaterials {
    fn template(&self, id: &str) -> Result<FrozenSkillBytes, OptimizeError> {
        skills::freeze_internal_task(&self.templates, id, &self.language)
    }

    fn method(&self, id: &str, host: &str) -> Result<Option<FrozenSkillBytes>, OptimizeError> {
        self.skills.resolve(id, host)
    }
}
