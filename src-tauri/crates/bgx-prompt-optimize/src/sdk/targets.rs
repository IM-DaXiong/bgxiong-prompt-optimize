use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{compiler_registry, production_registry, OptimizeError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TargetKind {
    Image,
    Video,
    Literary,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Target {
    pub target_id: String,
    pub label: String,
    pub kind: TargetKind,
    pub method_id: Option<String>,
    pub profile_id: Option<String>,
    pub profile_version: Option<u64>,
    pub input_modes: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ImageTarget {
    target_id: String,
    label: String,
    method_id: String,
}

#[derive(Deserialize)]
struct WorkflowCatalog {
    choices: Vec<WorkflowChoice>,
}

#[derive(Deserialize)]
struct WorkflowChoice {
    id: String,
    label: String,
    descriptor: WorkflowDescriptor,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkflowDescriptor {
    profile_id: String,
    version: u64,
    default_method_name: String,
    input_modes: Vec<String>,
}

pub fn target_catalog() -> Result<Vec<Target>, OptimizeError> {
    let bad_catalog = |message: String| OptimizeError::new("TARGET_CATALOG_INVALID", message);
    let images: Vec<ImageTarget> =
        serde_json::from_str(include_str!("../../resources/image-targets.v1.json"))
            .map_err(|err| bad_catalog(err.to_string()))?;
    let workflows: WorkflowCatalog = serde_json::from_str(include_str!(
        "../../../../resources/video-prompt-adapters/workflow-projections.json"
    ))
    .map_err(|err| bad_catalog(err.to_string()))?;
    let production = production_registry()?;
    let compiler = compiler_registry().map_err(|err| bad_catalog(format!("{err:?}")))?;
    let mut targets = vec![Target {
        target_id: "image/general".into(),
        label: "通用图片".into(),
        kind: TargetKind::Image,
        method_id: None,
        profile_id: None,
        profile_version: None,
        input_modes: Vec::new(),
    }];
    targets.extend(images.into_iter().map(|image| Target {
        target_id: image.target_id,
        label: image.label,
        kind: TargetKind::Image,
        method_id: Some(image.method_id),
        profile_id: None,
        profile_version: None,
        input_modes: Vec::new(),
    }));
    for choice in workflows.choices {
        let descriptor = choice.descriptor;
        let identity = production
            .identity_of(&descriptor.profile_id, descriptor.version)
            .ok_or_else(|| bad_catalog(format!("未注册的视频档案 {}", choice.id)))?;
        if choice.id != identity.key()
            || descriptor.default_method_name.is_empty()
            || identity
                .default_method_name
                .is_some_and(|method| method != descriptor.default_method_name)
            || compiler
                .projector(&descriptor.profile_id, descriptor.version)
                .is_none()
        {
            return Err(bad_catalog(format!("视频档案声明不一致 {}", choice.id)));
        }
        let input_modes: Vec<_> = identity
            .input_modes
            .iter()
            .filter(|mode| mode.required_media.is_empty() && mode.id == "text_to_video")
            .filter(|mode| descriptor.input_modes.iter().any(|id| id == mode.id))
            .map(|mode| mode.id.to_owned())
            .collect();
        if input_modes.is_empty() {
            continue;
        }
        targets.push(Target {
            target_id: format!("video/{}", choice.id),
            label: choice.label,
            kind: TargetKind::Video,
            method_id: Some(descriptor.default_method_name),
            profile_id: Some(descriptor.profile_id),
            profile_version: Some(descriptor.version),
            input_modes,
        });
    }
    targets.push(Target {
        target_id: "literary/general".into(),
        label: "文学提示词".into(),
        kind: TargetKind::Literary,
        method_id: None,
        profile_id: None,
        profile_version: None,
        input_modes: Vec::new(),
    });
    let mut unique = BTreeSet::new();
    for target in &targets {
        if !unique.insert(&target.target_id) {
            return Err(bad_catalog(format!("目标重复 {}", target.target_id)));
        }
    }
    Ok(targets)
}
