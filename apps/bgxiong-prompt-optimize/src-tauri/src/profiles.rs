//! `list_video_profiles` is sourced only from `bgx_prompt_optimize::production_registry`.
//! The shell does not keep a second handwritten roster. HappyHorse is absent
//! because the registry does not include it.

use serde::Serialize;

use bgx_prompt_optimize::{production_registry, OptimizeError};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct VideoInputModeDto {
    pub id: String,
    /// Media kinds this mode requires, copied from the registry declaration.
    pub required_media: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct VideoProfileDto {
    pub profile_id: String,
    pub profile_version: u64,
    pub display_key: String,
    pub default_method_name: Option<String>,
    pub input_modes: Vec<VideoInputModeDto>,
}

pub fn list_video_profiles() -> Result<Vec<VideoProfileDto>, OptimizeError> {
    let registry = production_registry()?;
    Ok(registry
        .identities()
        .iter()
        .map(|identity| VideoProfileDto {
            profile_id: identity.profile_id.to_string(),
            profile_version: identity.profile_version,
            display_key: identity.key(),
            default_method_name: identity.default_method_name.map(str::to_string),
            input_modes: identity
                .input_modes
                .iter()
                .map(|mode| VideoInputModeDto {
                    id: mode.id.to_string(),
                    required_media: mode
                        .required_media
                        .iter()
                        .map(|item| (*item).to_string())
                        .collect(),
                })
                .collect(),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bgx_prompt_optimize::production_registry;

    #[test]
    fn list_video_profiles_mirrors_the_core_registry() {
        let list = list_video_profiles().expect("registry is exported");
        let keys: Vec<&str> = list.iter().map(|item| item.display_key.as_str()).collect();
        let registry_keys: Vec<String> = production_registry()
            .expect("registry")
            .identities()
            .iter()
            .map(|item| item.key())
            .collect();
        assert_eq!(keys, registry_keys.iter().map(String::as_str).collect::<Vec<_>>());
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
        assert!(list.iter().all(|item| item.profile_id != "happyhorse"));
        let omni_new = list
            .iter()
            .find(|item| item.display_key == "kling.omni@2")
            .expect("omni nextgen");
        assert!(
            !omni_new
                .input_modes
                .iter()
                .any(|mode| mode.id == "reference_to_video"),
            "kling.omni@2 must not list reference_to_video"
        );
        let ltx = list
            .iter()
            .find(|item| item.display_key == "ltx@1")
            .expect("ltx");
        assert_eq!(
            ltx.default_method_name,
            None,
            "LTX has no default method; the shell must not invent one or fall back to generic"
        );
    }
}
