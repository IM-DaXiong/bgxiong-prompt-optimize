//! Official skill host-filter ids. These strings come from existing skill frontmatter
//! and ContentSkillOptimizePicker (`optimize.storyboard_shot`); this is not a second table.

/// Image optimize selector. A skill that also declares `clip-environment-prompt` stays included.
pub const OPTIMIZE_IMAGE_PROMPT: &str = "optimize-image-prompt";

/// Video prompt projection / writing-method host.
pub const VIDEO_PROMPT_PROJECTION: &str = "video-prompt-projection";

/// Segment literary optimize host.
pub const OPTIMIZE_SEGMENT: &str = "optimize-segment";

/// Storyboard-shot literary optimize host. The literary selector must filter with this id.
/// An empty host string makes the selector empty. This id is not in `currentAuthoringHosts`.
pub const OPTIMIZE_STORYBOARD_SHOT: &str = "optimize-storyboard-shot";

/// Clip environment prompt host. Image-page filtering must not drop a skill that also has this.
pub const CLIP_ENVIRONMENT_PROMPT: &str = "clip-environment-prompt";

/// Scene storyboard literary host (existing authoring surface, same official spelling).
pub const SCENE_STORYBOARD: &str = "scene-storyboard";
