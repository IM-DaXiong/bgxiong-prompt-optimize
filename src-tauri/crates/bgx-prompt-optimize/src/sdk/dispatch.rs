use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::{
    FrozenSkillBytes, ImageOptimizeRequest, ImageOptimizeResult, LiteraryOptimizeRequest,
    LiteraryOptimizeResult, MotherAuthorBind, OptimizeError, TextCompleter, TextVisionCaps,
    VideoOptimizeRequest, VideoOptimizeResult, VisionChannel,
};

use super::protocol::OptimizeArgs;
use super::targets::{target_catalog, Target, TargetKind};
use super::{optimize_frozen_image, optimize_frozen_literary, optimize_frozen_video};

pub enum FrozenOptimize {
    Image(ImageOptimizeRequest),
    Literary(LiteraryOptimizeRequest),
    Video(Box<VideoOptimizeRequest>),
}

pub enum FrozenResult {
    Image(ImageOptimizeResult),
    Literary(LiteraryOptimizeResult),
    Video(Box<VideoOptimizeResult>),
}

pub trait OptimizeMaterials {
    fn template(&self, id: &str) -> Result<FrozenSkillBytes, OptimizeError>;
    fn method(&self, id: &str, host: &str) -> Result<Option<FrozenSkillBytes>, OptimizeError>;
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OptimizeReceipt {
    pub request_id: String,
    pub target_id: String,
    pub method_id: Option<String>,
    pub method_digest: Option<String>,
    pub core_version: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OptimizeOutput {
    pub optimized_prompt: String,
    pub receipt: OptimizeReceipt,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Availability {
    Ready,
    Installable,
    Unavailable,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailableTarget {
    #[serde(flatten)]
    pub target: Target,
    pub method_digest: Option<String>,
    pub availability: Availability,
}

pub fn target_method(
    target: &Target,
    materials: &impl OptimizeMaterials,
) -> Result<Option<FrozenSkillBytes>, OptimizeError> {
    let Some(id) = &target.method_id else {
        return Ok(None);
    };
    let host = match target.kind {
        TargetKind::Image => crate::OPTIMIZE_IMAGE_PROMPT,
        TargetKind::Literary => crate::OPTIMIZE_STORYBOARD_SHOT,
        TargetKind::Video => crate::VIDEO_PROMPT_PROJECTION,
    };
    materials.method(id, host)
}

pub fn list_targets(
    materials: &impl OptimizeMaterials,
    is_installable: impl Fn(&str) -> bool,
) -> Result<Vec<AvailableTarget>, OptimizeError> {
    target_catalog()?
        .into_iter()
        .map(|target| {
            let method_digest =
                target_method(&target, materials)?.map(|skill| skill.package_digest);
            let availability = match target.method_id.as_deref() {
                _ if method_digest.is_some() => Availability::Ready,
                None => Availability::Ready,
                Some(id) if is_installable(id) => Availability::Installable,
                Some(_) => Availability::Unavailable,
            };
            Ok(AvailableTarget {
                target,
                method_digest,
                availability,
            })
        })
        .collect()
}

pub fn optimize_frozen(
    request: FrozenOptimize,
    text: &mut impl TextCompleter,
) -> Result<FrozenResult, OptimizeError> {
    match request {
        FrozenOptimize::Image(request) => {
            optimize_frozen_image(request, text).map(FrozenResult::Image)
        }
        FrozenOptimize::Literary(request) => {
            optimize_frozen_literary(request, text).map(FrozenResult::Literary)
        }
        FrozenOptimize::Video(request) => optimize_frozen_video(*request, text)
            .map(Box::new)
            .map(FrozenResult::Video),
    }
}

pub fn optimize(
    id: &str,
    args: OptimizeArgs,
    language: &str,
    materials: &impl OptimizeMaterials,
    text: &mut impl TextCompleter,
) -> Result<OptimizeOutput, OptimizeError> {
    args.validate()?;
    text.check_cancel()?;
    let target = target_catalog()?
        .into_iter()
        .find(|target| target.target_id == args.target_id)
        .ok_or_else(|| OptimizeError::new("UNKNOWN_TARGET", "未找到所选目标模型。"))?;
    let method = target_method(&target, materials)?;
    let method_digest = method.as_ref().map(|skill| skill.package_digest.clone());
    if args.expected_method_digest.is_some() && args.expected_method_digest != method_digest {
        return Err(OptimizeError::new(
            "SKILL_CHANGED",
            "Skill 已变化，请刷新后重新选择。",
        ));
    }
    if target.method_id.is_some() && method.is_none() {
        return Err(OptimizeError::new(
            "SKILL_NOT_FOUND",
            "请安装该目标模型使用的 Skill。",
        ));
    }
    let request = match target.kind {
        TargetKind::Image => FrozenOptimize::Image(ImageOptimizeRequest {
            task_id: id.into(),
            user_text: args.source_prompt,
            method,
            template: materials.template("image-prompt-optimize")?,
            language: language.into(),
            reference_image_count: 0,
            images: Vec::new(),
            text_vision: TextVisionCaps {
                supports_vision: false,
            },
            channel: VisionChannel::ReferenceOnly,
        }),
        TargetKind::Literary => FrozenOptimize::Literary(LiteraryOptimizeRequest {
            task_id: id.into(),
            open: json!({"prompt":args.source_prompt}),
            template: materials.template("text-prompt-optimize")?,
            creative_method: method,
            language: language.into(),
            output_contract: None,
            image_data_urls: Vec::new(),
        }),
        TargetKind::Video => FrozenOptimize::Video(Box::new(video_request(
            args.source_prompt,
            &target,
            method.ok_or_else(|| OptimizeError::new("SKILL_NOT_FOUND", "缺少视频格式 Skill。"))?,
            language,
            materials,
        )?)),
    };
    let optimized_prompt = match optimize_frozen(request, text)? {
        FrozenResult::Image(result) => result.optimized_prompt,
        FrozenResult::Literary(result) => result.prompt,
        FrozenResult::Video(result) => {
            if let Some(rejected) = result.rejected {
                return Err(OptimizeError::new(rejected.code, rejected.detail));
            }
            result.final_prompt.ok_or_else(no_final_prompt)?
        }
    };
    if optimized_prompt.trim().is_empty() {
        return Err(no_final_prompt());
    }
    text.check_cancel()?;
    Ok(OptimizeOutput {
        optimized_prompt,
        receipt: OptimizeReceipt {
            request_id: id.into(),
            target_id: target.target_id,
            method_id: target.method_id,
            method_digest,
            core_version: env!("CARGO_PKG_VERSION"),
        },
    })
}

fn no_final_prompt() -> OptimizeError {
    OptimizeError::new("OPTIMIZE_NO_FINAL_PROMPT", "未取得优化后的提示词。")
}

fn video_request(
    source: String,
    target: &Target,
    format_method: FrozenSkillBytes,
    language: &str,
    materials: &impl OptimizeMaterials,
) -> Result<VideoOptimizeRequest, OptimizeError> {
    let mut snapshot = json!({
        "profileId":target.profile_id,
        "profileVersion":target.profile_version,
        "inputMode":"text_to_video",
        "durationSeconds":null,
        "expressionLanguage":language,
        "referenceUseIds":[],
    });
    let digest = hex_digest(snapshot.to_string().as_bytes());
    snapshot["capsDigest"] = Value::String(digest);
    let resources: Vec<_> = format_method
        .resources
        .iter()
        .map(|(path, bytes)| json!({"resourceRef":path,"sha256":hex_digest(bytes)}))
        .collect();
    let method = serde_json::from_value(json!({
        "selectionKind":"custom",
        "activationId":format_method.stable_id,
        "stableId":format_method.stable_id,
        "packageDigest":format_method.package_digest,
        "name":format_method.stable_id,
        "bodySha256":hex_digest(format_method.body.as_bytes()),
        "resources":resources,
        "bindingProfileId":target.profile_id,
        "bindingProfileVersion":target.profile_version,
    }))
    .map_err(|error| OptimizeError::new("OPTIMIZE_METHOD_INVALID", error.to_string()))?;
    let templates = [
        "video-content-optimize",
        "video-content-review",
        "video-expression",
        "video-expression-review",
    ]
    .into_iter()
    .map(|task| materials.template(task).map(|bytes| (task.into(), bytes)))
    .collect::<Result<_, _>>()?;
    Ok(VideoOptimizeRequest {
        source_text: source.clone(),
        change_request: None,
        target_snapshot: snapshot,
        format_method,
        content_method: None,
        templates,
        reference_inventory: Vec::new(),
        expansion_policy: None,
        negative_context: None,
        mother_author: Some(MotherAuthorBind {
            task_context: json!({"currentDraft":source,"changeRequest":null,"references":[]}),
            expected_shot_ids: vec!["shot-1".into()],
            source_unit_ids: Vec::new(),
            quote_candidates: Vec::new(),
            source_scope: None,
            source_mode: "optimize_existing".into(),
            subject_ids: Vec::new(),
        }),
        mother_review_base: None,
        method,
        identity_locks: Vec::new(),
        allow_inferred: false,
        existing_mother: None,
    })
}

fn hex_digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
