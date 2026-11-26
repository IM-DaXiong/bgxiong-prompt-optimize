//! Schema-generated video-prompt projection DTOs (VS07 / R14a).

mod generated {
    include!(concat!(
        env!("OUT_DIR"),
        "/app_contracts_video_prompt_projection.rs"
    ));
}

pub use generated::*;

impl VideoPromptPipelineMode {
    /// Wire value of the mode (same string as the generated serde rename).
    pub fn as_str(&self) -> &'static str {
        match self {
            VideoPromptPipelineMode::Optimize => "optimize",
            VideoPromptPipelineMode::CompressOnly => "compress_only",
            VideoPromptPipelineMode::Generate => "generate",
            VideoPromptPipelineMode::ProjectExisting => "project_existing",
            VideoPromptPipelineMode::AdaptExpand => "adapt_expand",
        }
    }

    /// `true` for the modes that must carry `projectionMethodSelection` (CF-05); compress_only
    /// must not carry it.
    pub fn takes_projection_selection(&self) -> bool {
        !matches!(self, VideoPromptPipelineMode::CompressOnly)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipeline_mode_wire_and_selection() {
        assert_eq!(VideoPromptPipelineMode::Optimize.as_str(), "optimize");
        assert!(VideoPromptPipelineMode::Generate.takes_projection_selection());
        assert!(!VideoPromptPipelineMode::CompressOnly.takes_projection_selection());
    }
}
