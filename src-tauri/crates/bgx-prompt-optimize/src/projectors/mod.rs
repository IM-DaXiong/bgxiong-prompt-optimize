//! Production projectors moved from `video_prompt_adapters` in T18a-f.
//! `production_registry` must keep the same profile set; missing profiles must not fall back to generic.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use bgx_video_prompt_contract::descriptor::{
    INPUT_MODE_IDS, PROMPT_PROJECTION_ID_MAX_CHARS, PROMPT_PROJECTION_VERSION_MAX,
};

use self::port::VideoPromptProjectionPort;

pub mod catalog;
/// Frozen public selector identity (profile + default method + declared modes).
/// The compiler implementation identity is [`ProjectorCompilerIdentity`].
pub use catalog::{production_registry, ProjectorIdentity, ProjectorInputMode, ProjectorRegistry};
/// Port/compiler identity: includes `compiler_revision` and default/mode resource refs.
/// This is not the frozen catalog selector [`ProjectorIdentity`].
pub use self::port::ProjectorIdentity as ProjectorCompilerIdentity;
pub mod cite_tokens;
pub mod generic;
pub mod h3;
pub mod kling;
pub mod ltx;
pub mod seedance;
pub mod wan;
pub mod h3_audio_usage;
pub mod port;

/// Default-method facts taken from a projector identity (VS24). Not a second official table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectorDefaults {
    pub input_modes: Vec<String>,
    pub default_resource_refs: Vec<String>,
}

/// Why the projector directory cannot be built or used.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProjectorRegistryError {
    DuplicateProfile {
        profile_id: String,
        profile_version: u64,
    },
    InvalidIdentity {
        profile_id: String,
        profile_version: u64,
    },
    NotComposed,
}

impl ProjectorRegistryError {
    pub fn code(&self) -> &'static str {
        match self {
            ProjectorRegistryError::DuplicateProfile { .. } => {
                "VIDEO_PROMPT_PROJECTOR_DUPLICATE_PROFILE"
            }
            ProjectorRegistryError::InvalidIdentity { .. } => {
                "VIDEO_PROMPT_PROJECTOR_INVALID_IDENTITY"
            }
            ProjectorRegistryError::NotComposed => "VIDEO_PROMPT_PROJECTOR_NOT_COMPOSED",
        }
    }
}

impl fmt::Display for ProjectorRegistryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProjectorRegistryError::DuplicateProfile {
                profile_id,
                profile_version,
            } => write!(
                f,
                "duplicate projector profile {profile_id}@{profile_version}"
            ),
            ProjectorRegistryError::InvalidIdentity {
                profile_id,
                profile_version,
            } => write!(
                f,
                "invalid projector identity {profile_id}@{profile_version}"
            ),
            ProjectorRegistryError::NotComposed => {
                write!(f, "video prompt projector directory is not composed")
            }
        }
    }
}

impl std::error::Error for ProjectorRegistryError {}

pub struct VideoPromptProjectorRegistry {
    projectors: BTreeMap<(String, u64), Arc<dyn VideoPromptProjectionPort>>,
}

impl VideoPromptProjectorRegistry {
    pub fn new(
        projectors: Vec<Arc<dyn VideoPromptProjectionPort>>,
    ) -> Result<Self, ProjectorRegistryError> {
        let mut map: BTreeMap<(String, u64), Arc<dyn VideoPromptProjectionPort>> = BTreeMap::new();
        for projector in projectors {
            let identity = projector.identity();
            let declaration_ok = identity.declaration_is_consistent(INPUT_MODE_IDS);
            let ProjectorCompilerIdentity {
                profile_id,
                profile_version,
                ..
            } = identity;
            let id_ok = !profile_id.is_empty()
                && profile_id.chars().count() <= PROMPT_PROJECTION_ID_MAX_CHARS;
            let version_ok =
                (1..=u64::from(PROMPT_PROJECTION_VERSION_MAX)).contains(&profile_version);
            if !id_ok || !version_ok || !declaration_ok {
                return Err(ProjectorRegistryError::InvalidIdentity {
                    profile_id,
                    profile_version,
                });
            }
            let key = (profile_id, profile_version);
            if map.contains_key(&key) {
                return Err(ProjectorRegistryError::DuplicateProfile {
                    profile_id: key.0,
                    profile_version: key.1,
                });
            }
            map.insert(key, projector);
        }
        Ok(VideoPromptProjectorRegistry { projectors: map })
    }

    /// Exact profile+version only. Missing profiles do not fall back to generic.
    pub fn projector(
        &self,
        profile_id: &str,
        profile_version: u64,
    ) -> Option<Arc<dyn VideoPromptProjectionPort>> {
        self.projectors
            .get(&(profile_id.to_string(), profile_version))
            .cloned()
    }

    pub fn is_registered_profile(&self, profile_id: &str, version: u32) -> bool {
        self.projectors
            .contains_key(&(profile_id.to_string(), u64::from(version)))
    }

    pub fn identities(&self) -> Vec<ProjectorCompilerIdentity> {
        self.projectors.values().map(|p| p.identity()).collect()
    }

    pub fn identity_of(&self, profile_id: &str, profile_version: u64) -> Option<ProjectorCompilerIdentity> {
        self.projector(profile_id, profile_version)
            .map(|p| p.identity())
    }

    pub fn is_registered_library_profile(&self, profile_id: &str, profile_version: u64) -> bool {
        u32::try_from(profile_version)
            .is_ok_and(|version| self.is_registered_profile(profile_id, version))
    }

    pub fn supports_input_mode(&self, profile_id: &str, profile_version: u64, mode: &str) -> bool {
        self.identity_of(profile_id, profile_version)
            .is_some_and(|identity| identity.supports_input_mode(mode))
    }

    pub fn projector_defaults(
        &self,
        profile_id: &str,
        profile_version: u64,
    ) -> Option<ProjectorDefaults> {
        self.identity_of(profile_id, profile_version)
            .map(|identity| ProjectorDefaults {
                input_modes: identity.input_modes,
                default_resource_refs: identity.default_resource_refs,
            })
    }
}

/// Exact compiler directory for `run_video_expression` / T18. The frozen public roster is `production_registry`.
pub fn compiler_registry() -> Result<VideoPromptProjectorRegistry, ProjectorRegistryError> {
    VideoPromptProjectorRegistry::new(vec![
        Arc::new(generic::GenericProjector) as Arc<dyn VideoPromptProjectionPort>,
        Arc::new(wan::WanProjector) as Arc<dyn VideoPromptProjectionPort>,
        Arc::new(seedance::SeedanceProjector) as Arc<dyn VideoPromptProjectionPort>,
        Arc::new(kling::KlingProjector::new(kling::KlingTarget::Classic))
            as Arc<dyn VideoPromptProjectionPort>,
        Arc::new(kling::KlingProjector::new(kling::KlingTarget::OmniLegacy))
            as Arc<dyn VideoPromptProjectionPort>,
        Arc::new(kling::KlingProjector::new(kling::KlingTarget::OmniNextgen))
            as Arc<dyn VideoPromptProjectionPort>,
        Arc::new(h3::compile::H3Projector) as Arc<dyn VideoPromptProjectionPort>,
        Arc::new(ltx::LtxProjector) as Arc<dyn VideoPromptProjectionPort>,
    ])
}