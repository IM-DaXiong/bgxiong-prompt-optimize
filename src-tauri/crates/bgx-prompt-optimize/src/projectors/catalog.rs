//! Production identity catalog used by `list_video_profiles`.
//!
//! Mode ids and default method names come from the existing projector constants and
//! `video_prompt_adapters::bindings` NativeProjection values. This is not a second
//! product table. LTX `default_method_name` is `None` until a capability descriptor
//! is supplied; the test-only `LTX_DEFAULT_METHOD_NAME` is not promoted.
//! `kling.omni@2` lists `KLING_OMNI_NEXTGEN_DECLARED_MODES` (no `reference_to_video`).
//! Required media kinds follow existing adapter mode parses (image_to_video is first
//! frame, not a generic "image").

use crate::error::error_codes;
use crate::error::OptimizeError;

/// One declared input mode of a production profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectorInputMode {
    pub id: &'static str,
    /// Neutral media kinds this mode requires (from existing adapter mode parses).
    pub required_media: &'static [&'static str],
}

/// One production projector identity. `key()` is `profile_id@version`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectorIdentity {
    pub profile_id: &'static str,
    pub profile_version: u64,
    pub default_method_name: Option<&'static str>,
    pub input_modes: &'static [ProjectorInputMode],
}

impl ProjectorIdentity {
    pub fn key(&self) -> String {
        format!("{}@{}", self.profile_id, self.profile_version)
    }

    pub fn supports_input_mode(&self, mode: &str) -> bool {
        self.input_modes.iter().any(|item| item.id == mode)
    }
}

/// Catalog of production identities, in §2.4 shell order.
#[derive(Debug, Clone)]
pub struct ProjectorRegistry {
    identities: &'static [ProjectorIdentity],
}

impl ProjectorRegistry {
    pub fn identities(&self) -> &'static [ProjectorIdentity] {
        self.identities
    }

    pub fn identity_of(&self, profile_id: &str, profile_version: u64) -> Option<&ProjectorIdentity> {
        self.identities
            .iter()
            .find(|item| item.profile_id == profile_id && item.profile_version == profile_version)
    }

    pub fn is_registered_profile(&self, profile_id: &str, profile_version: u64) -> bool {
        self.identity_of(profile_id, profile_version).is_some()
    }

    pub fn supports_input_mode(
        &self,
        profile_id: &str,
        profile_version: u64,
        mode: &str,
    ) -> bool {
        self.identity_of(profile_id, profile_version)
            .is_some_and(|identity| identity.supports_input_mode(mode))
    }

    pub fn require(
        &self,
        profile_id: &str,
        profile_version: u64,
    ) -> Result<&ProjectorIdentity, OptimizeError> {
        self.identity_of(profile_id, profile_version).ok_or_else(|| {
            OptimizeError::new(
                error_codes::VIDEO_PROMPT_PROJECTOR_MISSING,
                format!("没有 {profile_id}@{profile_version} 的生产投影器，不会改用通用档案。"),
            )
        })
    }
}

const fn mode(id: &'static str, required_media: &'static [&'static str]) -> ProjectorInputMode {
    ProjectorInputMode { id, required_media }
}

const NONE: &[&str] = &[];
const FIRST: &[&str] = &["first_frame"];
const FIRST_LAST: &[&str] = &["first_frame", "last_frame"];
const LAST: &[&str] = &["last_frame"];
const REF_IMAGE: &[&str] = &["reference_image"];

// Mode id lists match the moved projector constants (and bindings for omni nextgen).
const H3_MODES: &[ProjectorInputMode] = &[
    mode("text_to_video", NONE),
    mode("image_to_video", FIRST),
    mode("first_last_frame", FIRST_LAST),
    mode("last_frame_only", LAST),
    mode("multi_image_to_video", REF_IMAGE),
    mode("reference_to_video", REF_IMAGE),
];
const SEEDANCE_MODES: &[ProjectorInputMode] = H3_MODES;
const KLING_CLASSIC_MODES: &[ProjectorInputMode] = &[
    mode("text_to_video", NONE),
    mode("image_to_video", FIRST),
    mode("first_last_frame", FIRST_LAST),
    mode("multi_image_to_video", REF_IMAGE),
];
const KLING_OMNI_MODES: &[ProjectorInputMode] = &[
    mode("text_to_video", NONE),
    mode("image_to_video", FIRST),
    mode("first_last_frame", FIRST_LAST),
    mode("multi_image_to_video", REF_IMAGE),
    mode("reference_to_video", REF_IMAGE),
];
/// Official `KLING_OMNI_NEXTGEN_DECLARED_MODES` from bindings.rs (CF-20).
const KLING_OMNI_NEXTGEN_MODES: &[ProjectorInputMode] = &[
    mode("text_to_video", NONE),
    mode("image_to_video", FIRST),
    mode("first_last_frame", FIRST_LAST),
    mode("multi_image_to_video", REF_IMAGE),
];
const WAN_MODES: &[ProjectorInputMode] = &[
    mode("text_to_video", NONE),
    mode("image_to_video", FIRST),
    mode("first_last_frame", FIRST_LAST),
    mode("reference_to_video", REF_IMAGE),
];
const LTX_MODES: &[ProjectorInputMode] = &[
    mode("text_to_video", NONE),
    mode("image_to_video", FIRST),
    mode("first_last_frame", FIRST_LAST),
];
const GENERIC_MODES: &[ProjectorInputMode] = &[
    mode("text_to_video", NONE),
    mode("image_to_video", FIRST),
    mode("multi_image_to_video", REF_IMAGE),
    mode("first_last_frame", FIRST_LAST),
    mode("last_frame_only", LAST),
    mode("reference_to_video", REF_IMAGE),
    mode("editor_to_video", NONE),
];

/// §2.4 shell order: H3, Seedance, Kling classic, Omni legacy, Omni nextgen, Wan, LTX, generic.
const PRODUCTION: &[ProjectorIdentity] = &[
    ProjectorIdentity {
        profile_id: "h3",
        profile_version: 1,
        default_method_name: Some("h3-prompt-writing"),
        input_modes: H3_MODES,
    },
    ProjectorIdentity {
        profile_id: "seedance",
        profile_version: 1,
        default_method_name: Some("seedance-prompt-writing"),
        input_modes: SEEDANCE_MODES,
    },
    ProjectorIdentity {
        profile_id: "kling.classic",
        profile_version: 1,
        default_method_name: Some("kling-prompt-writing"),
        input_modes: KLING_CLASSIC_MODES,
    },
    ProjectorIdentity {
        profile_id: "kling.omni",
        profile_version: 1,
        default_method_name: Some("kling-prompt-writing"),
        input_modes: KLING_OMNI_MODES,
    },
    ProjectorIdentity {
        profile_id: "kling.omni",
        profile_version: 2,
        default_method_name: Some("kling-prompt-writing"),
        input_modes: KLING_OMNI_NEXTGEN_MODES,
    },
    ProjectorIdentity {
        profile_id: "wan",
        profile_version: 1,
        default_method_name: Some("wan-prompt-writing"),
        input_modes: WAN_MODES,
    },
    ProjectorIdentity {
        profile_id: "ltx",
        profile_version: 1,
        default_method_name: None,
        input_modes: LTX_MODES,
    },
    ProjectorIdentity {
        profile_id: "generic",
        profile_version: 1,
        default_method_name: Some("generic-video-prompt-writing"),
        input_modes: GENERIC_MODES,
    },
];

/// Frozen public roster. Missing profiles do not fall back to generic.
pub fn production_registry() -> Result<ProjectorRegistry, OptimizeError> {
    Ok(ProjectorRegistry {
        identities: PRODUCTION,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_roster_is_the_eight_real_projectors() {
        let registry = production_registry().unwrap();
        let keys: Vec<String> = registry.identities().iter().map(|i| i.key()).collect();
        assert_eq!(
            keys,
            [
                "h3@1",
                "seedance@1",
                "kling.classic@1",
                "kling.omni@1",
                "kling.omni@2",
                "wan@1",
                "ltx@1",
                "generic@1",
            ]
        );
        assert!(registry.identity_of("happyhorse", 1).is_none());
        assert!(!registry
            .identity_of("kling.omni", 2)
            .unwrap()
            .supports_input_mode("reference_to_video"));
        assert!(registry
            .identity_of("kling.omni", 1)
            .unwrap()
            .supports_input_mode("reference_to_video"));
        assert!(registry
            .identity_of("ltx", 1)
            .unwrap()
            .default_method_name
            .is_none());
        assert!(registry.require("missing", 1).is_err());
    }
}