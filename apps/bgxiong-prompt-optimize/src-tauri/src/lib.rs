//! 提示词优化独立壳。统一编译验收前不得称为可用。

mod attachments;
use prompt_optimize_runtime::cancel;
mod commands;
use prompt_optimize_runtime::config;
use prompt_optimize_runtime::error_ui;
mod paths;
mod profiles;
use prompt_optimize_runtime::skills;
use prompt_optimize_runtime::snapshot;
use prompt_optimize_runtime::text_port;

/// Path constants for T25 loopback tests. Not a second HTTP protocol.
pub mod text_port_paths {
    pub use crate::text_port::{minimax_path as minimax, openai_compat_path as openai_compat, qwen_multimodal_path as qwen_multimodal, qwen_text_path as qwen_text};
}

use commands::ShellState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .manage(ShellState::default())
        .invoke_handler(tauri::generate_handler![
            commands::load_config,
            commands::save_config,
            commands::list_text_channels,
            commands::list_skills,
            commands::list_video_profiles,
            commands::list_optimize_targets,
            commands::optimize_prompt,
            commands::check_prompt_skills,
            commands::install_prompt_skill,
            commands::pick_images,
            commands::pick_references,
            commands::optimize_image,
            commands::optimize_video,
            commands::optimize_literary,
            commands::compress_video_prompt,
            commands::cancel_optimize,
            commands::save_result,
        ])
        .run(tauri::generate_context!())
        .expect("error while running 提示词优化");
}
