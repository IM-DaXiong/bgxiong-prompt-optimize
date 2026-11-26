//! Shared prompt-optimize core. This crate must stay `pure-core`.
//! Public exports are frozen in PLAN 20261115 appendix B plus the coordinator
//! supplements of 2026-11-16. Function bodies come from lane A true-source
//! ports. Do not invent a second API.

pub mod error;
pub mod hosts;
pub mod image_job;
pub mod literary_job;
pub mod reference_guidance;
mod reference_numbers;
pub mod run;
pub mod sdk;
pub mod skill_material;
pub mod source_normalize;
pub mod template;
pub mod text_port;
pub mod video_job;
pub mod video_run;
pub mod vision;

pub mod projectors;

pub use error::{OptimizeError, error_codes};
pub use hosts::{
    CLIP_ENVIRONMENT_PROMPT, OPTIMIZE_IMAGE_PROMPT, OPTIMIZE_SEGMENT, OPTIMIZE_STORYBOARD_SHOT,
    SCENE_STORYBOARD, VIDEO_PROMPT_PROJECTION,
};
pub use image_job::{
    ImageOptimizeRequest, ImageOptimizeResult, SubmittedImage, TextVisionCaps, VisionChannel,
};
pub use literary_job::{LiteraryOptimizeRequest, LiteraryOptimizeResult};
pub use projectors::{
    compiler_registry, production_registry, ProjectorIdentity, ProjectorInputMode, ProjectorRegistry,
};
pub use reference_numbers::ReferenceNumbers;
pub use run::{run_image, run_literary, run_video_expression};
pub use skill_material::FrozenSkillBytes;
pub use source_normalize::{
    run_source_normalize, SourceNormalizeRequest, SourceNormalizeResult, STAGE_SOURCE_NORMALIZE,
};
pub use template::{render as render_template, RenderedMessage};
pub use text_port::{TextCall, TextCompleter, TextImageCountPolicy, TextMessage, TextReply};
pub use video_job::{
    draft_schema_for, expression_compile_document, review_schema_for, selected_quote_document,
    semantic_fingerprint, VideoExpressionRejected, VideoExpressionRequest, VideoExpressionResult,
};
pub use video_run::{
    author_batch_envelope, bind_mother_source_scopes, compress_video_prompt, run_video_optimize,
    MotherAuthorBind, MotherSourceMode, MotherSourceScopeBind, MotherSourceUnitScope,
    MotherSourceWriteScope, VideoCompressRequest, VideoCompressResult, VideoOptimizePending, VideoOptimizeRequest,
    VideoOptimizeResult,
};
pub use vision::{
    cap_optional_channel, decide, encode_original_image_bytes, reject_if_over_submitted_cap,
    EncodedLlmImage, VisionAttachDecision, VisionInputCaps, MAX_IPC_RELS,
};
