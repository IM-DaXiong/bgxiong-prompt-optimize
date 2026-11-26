//! 模块：video_prompt_adapters::h3
//! 作用：MiniMax H3 视频目标的提示词投影（`h3@1`）。H3 专属规则只在本目录：五种官方模式与对齐头
//! （`mode`，VS30）、三 / 六段布局与唯一编译器（`compile`，VS31）、音频段落 / 人声行 / `<d>` 语法
//! （`audio`，VS32）。

pub mod audio;
pub mod compile;
pub mod mode;

/// Official profile id from ideo_prompt_adapters::bindings (not a second table).
pub const H3_PROFILE_ID: &str = "h3";
/// Official profile version in bindings is u32 = 1; compile uses u64::from.
pub const H3_PROFILE_VERSION: u32 = 1;

