//! Tauri commands for the independent shell.
//! T24: list_skills, optimize_image, optimize_video, save_config, pick_images.
//! T33 adds list_video_profiles. T27 adds save_result / cancel / literary / compress entries.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_dialog::{DialogExt, FilePath};
use tokio::sync::oneshot;

use bgx_prompt_optimize::image_job::{
    ImageOptimizeRequest, ImageOptimizeResult, TextVisionCaps, VisionChannel,
};
use bgx_prompt_optimize::literary_job::{LiteraryOptimizeRequest, LiteraryOptimizeResult};
use bgx_prompt_optimize::video_run::{
    VideoCompressRequest, VideoCompressResult, VideoOptimizeRequest, VideoOptimizeResult,
};
use bgx_prompt_optimize::{
    compress_video_prompt as shared_compress_video_prompt, production_registry,
    reject_if_over_submitted_cap, run_image, run_literary, run_video_optimize, FrozenSkillBytes,
    OptimizeError, TextCompleter, MAX_IPC_RELS,
};

use crate::attachments::{self, PickedPathSet, ReferencePick};
use crate::cancel;
use crate::config::{ShellConfig, ShellConfigPublic};
use crate::error_ui;
use crate::paths;
use crate::profiles::{self, VideoProfileDto};
use crate::skills::{self, SkillListItem};
use crate::snapshot::{self, VideoTargetForm};
use crate::text_port::ShellTextCompleter;
use bgx_prompt_optimize::sdk::{self, AvailableTarget, protocol::OptimizeArgs};
use prompt_optimize_runtime::materials::StandaloneMaterials;

pub struct ShellState {
    pub picked: PickedPathSet,
}

impl Default for ShellState {
    fn default() -> Self {
        Self {
            picked: PickedPathSet::default(),
        }
    }
}

fn map_err(error: OptimizeError) -> String {
    error_ui::to_command_error(error)
}

fn load_cfg(app: &AppHandle) -> Result<ShellConfig, OptimizeError> {
    let path = paths::config_path(app)?;
    ShellConfig::load_file(&path)
}

fn sdk_materials(app: &AppHandle, cfg: &ShellConfig) -> Result<StandaloneMaterials, OptimizeError> {
    StandaloneMaterials::new(
        cfg,
        &paths::shell_app_data_dir(app)?,
        paths::resource_dir(app).as_deref(),
        paths::exe_dir(app).as_deref(),
    )
}

#[tauri::command]
pub fn list_optimize_targets(app: AppHandle) -> Result<Vec<AvailableTarget>, String> {
    let cfg = if paths::config_path(&app).map_err(map_err)?.is_file() {
        load_cfg(&app).map_err(map_err)?
    } else {
        ShellConfig::default()
    };
    let materials = sdk_materials(&app, &cfg).map_err(map_err)?;
    let candidates = materials.skills.cached_candidates().map_err(map_err)?;
    sdk::list_targets(&materials, |id| materials.skills.can_install(&candidates, id)).map_err(map_err)
}

fn optional_sdk_materials(app: &AppHandle) -> Result<StandaloneMaterials, String> {
    let config = match load_cfg(app) {
        Ok(cfg) => cfg,
        Err(error) if error.code == "TEXT_MODEL_NOT_CONFIGURED" => ShellConfig::default(),
        Err(error) => return Err(map_err(error)),
    };
    sdk_materials(app, &config).map_err(map_err)
}

#[tauri::command]
pub async fn check_prompt_skills(app: AppHandle) -> Result<Vec<prompt_optimize_runtime::skill_store::Candidate>, String> {
    let materials = optional_sdk_materials(&app)?;
    tauri::async_runtime::spawn_blocking(move || materials.skills.check()).await
        .map_err(|_| "检查 Skill 更新时发生中断。".to_owned())?.map_err(map_err)
}

#[tauri::command]
pub async fn install_prompt_skill(
    app: AppHandle, candidate_id: String, confirmed_new_install: bool,
) -> Result<prompt_optimize_runtime::skill_store::InstallReceipt, String> {
    let materials = optional_sdk_materials(&app)?;
    tauri::async_runtime::spawn_blocking(move || materials.skills.install(&candidate_id, confirmed_new_install)).await
        .map_err(|_| "安装 Skill 时发生中断。".to_owned())?.map_err(map_err)
}

#[tauri::command]
pub async fn optimize_prompt(app: AppHandle, input: OptimizeArgs) -> Result<sdk::OptimizeOutput, String> {
    let cfg = load_cfg(&app).map_err(map_err)?;
    let materials = sdk_materials(&app, &cfg).map_err(map_err)?;
    let job_id = cancel::start_job().map_err(map_err)?;
    let _ = app.emit("optimize-started", &job_id);
    let worker_id = job_id.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let mut text = ShellTextCompleter::new(cfg.clone(), false)?;
        sdk::optimize(&worker_id, input, cfg.language(), &materials, &mut text)
    }).await;
    cancel::finish_job(&job_id);
    result.map_err(|_| "优化任务意外中断。".to_owned())?.map_err(map_err)
}

fn skill_root(app: &AppHandle, cfg: &ShellConfig) -> Result<PathBuf, OptimizeError> {
    if let Some(root) = cfg
        .skill_library_root
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        let path = paths::require_absolute(Path::new(root), "技能库根")?;
        paths::reject_host_pointers(&path)?;
        return Ok(path);
    }
    let dir = paths::default_skill_library_dir(app)?;
    paths::reject_host_pointers(&dir)?;
    Ok(dir)
}

fn ensure_library(app: &AppHandle, cfg: &ShellConfig) -> Result<PathBuf, OptimizeError> {
    let root = skill_root(app, cfg)?;
    let builtin = paths::resolve_builtin_skills_root(paths::resource_dir(app).as_deref(), paths::exe_dir(app).as_deref());
    match builtin {
        Ok(builtin) => skills::seed_library_if_empty(&root, &builtin)?,
        Err(error) => {
            if !root.is_dir() {
                return Err(error);
            }
        }
    }
    Ok(root)
}

fn template_root(app: &AppHandle, cfg: &ShellConfig) -> Result<PathBuf, OptimizeError> {
    let configured = cfg
        .internal_template_root
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from);
    let root = paths::resolve_internal_prompt_root(
        configured.as_deref(),
        paths::resource_dir(app).as_deref(),
        paths::exe_dir(app).as_deref(),
    )?;
    paths::effective_internal_generation(&root)
}

#[tauri::command]
pub fn load_config(app: AppHandle) -> Result<ShellConfigPublic, String> {
    match load_cfg(&app) {
        Ok(cfg) => Ok(cfg.public_view()),
        Err(error) if error.code == "TEXT_MODEL_NOT_CONFIGURED" => Ok(ShellConfigPublic {
            provider_kind: String::new(),
            base_url: None,
            model: None,
            saved_models: Vec::new(),
            has_api_key: false,
            timeout_secs: None,
            max_output_tokens: None,
            skill_library_root: None,
            internal_template_root: None,
            adapter_package_root: None,
            local_model_path: None,
            language: Some("zh".into()),
            supports_vision: false,
        }),
        Err(error) => Err(map_err(error)),
    }
}

#[tauri::command]
pub fn save_config(app: AppHandle, mut config: ShellConfig) -> Result<ShellConfigPublic, String> {
    let path = paths::config_path(&app).map_err(map_err)?;
    config.remember_model(ShellConfig::load_file(&path).ok());
    config.save_file(&path).map_err(map_err)?;
    tracing::info!(
        model = config.model.as_deref().unwrap_or(""),
        provider = %config.provider_kind,
        "config saved"
    );
    Ok(config.public_view())
}

#[tauri::command]
pub fn list_text_channels() -> Vec<TextChannelDto> {
    vec![
        TextChannelDto::new("text_openai_compat", "OpenAI 兼容", true, "cloud"),
        TextChannelDto::new("text_volc_ark_compat", "火山方舟（OpenAI 兼容）", true, "cloud"),
        TextChannelDto::new("text_deepseek_compat", "DeepSeek（OpenAI 兼容）", true, "cloud"),
        TextChannelDto::new("text_custom", "自定义（OpenAI 兼容）", true, "cloud"),
        TextChannelDto::new("text_qwen", "通义千问", true, "cloud"),
        TextChannelDto::new("text_minimax_m3", "MiniMax-M3", true, "cloud"),
        TextChannelDto::new("text_local_model", "本地 GGUF", false, "local"),
        TextChannelDto::new("text_vendor_adapter", "适配包文本", false, "adapter"),
    ]
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextChannelDto {
    pub provider_kind: String,
    pub label: String,
    pub needs_base_url: bool,
    pub group: String,
}

impl TextChannelDto {
    fn new(provider_kind: &str, label: &str, needs_base_url: bool, group: &str) -> Self {
        Self {
            provider_kind: provider_kind.into(),
            label: label.into(),
            needs_base_url,
            group: group.into(),
        }
    }
}

#[tauri::command]
pub fn list_skills(app: AppHandle, host: String) -> Result<Vec<SkillListItem>, String> {
    let cfg = load_cfg(&app).or_else(|error| {
        if error.code == "TEXT_MODEL_NOT_CONFIGURED" {
            Ok(ShellConfig::default())
        } else {
            Err(error)
        }
    }).map_err(map_err)?;
    let root = ensure_library(&app, &cfg).map_err(map_err)?;
    skills::list_skills(&root, &host).map_err(map_err)
}

#[tauri::command]
pub fn list_video_profiles() -> Result<Vec<VideoProfileDto>, String> {
    profiles::list_video_profiles().map_err(map_err)
}

#[tauri::command]
pub async fn pick_images(app: AppHandle, state: State<'_, ShellState>) -> Result<Vec<String>, String> {
    pick_files(app, state, &["png", "jpg", "jpeg", "webp", "gif", "bmp", "tif", "tiff"]).await
}

#[tauri::command]
pub async fn pick_references(app: AppHandle, state: State<'_, ShellState>) -> Result<Vec<String>, String> {
    pick_files(
        app,
        state,
        &[
            "png", "jpg", "jpeg", "webp", "gif", "bmp", "mp4", "mov", "webm", "mkv", "wav", "mp3",
            "m4a", "aac", "flac",
        ],
    )
    .await
}

async fn pick_files(
    app: AppHandle,
    state: State<'_, ShellState>,
    extensions: &'static [&'static str],
) -> Result<Vec<String>, String> {
    let (tx, rx) = oneshot::channel();
    app.dialog()
        .file()
        .add_filter("媒体文件", extensions)
        .set_title("选择文件")
        .pick_files(move |paths| {
            let _ = tx.send(paths);
        });
    let picked = rx.await.map_err(|_| "已取消选择文件".to_string())?;
    let Some(paths) = picked else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    let mut remembered = Vec::new();
    for path in paths {
        let path = match path {
            FilePath::Path(p) => p,
            FilePath::Url(url) => PathBuf::from(url.path()),
        };
        if !path.is_absolute() {
            return Err("只接受系统文件框返回的绝对路径".into());
        }
        out.push(path.to_string_lossy().into_owned());
        remembered.push(path);
    }
    state.picked.remember(&remembered);
    Ok(out)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageOptimizeInput {
    pub user_text: String,
    pub method_id: Option<String>,
    pub language: Option<String>,
    pub image_paths: Vec<String>,
}

#[tauri::command]
pub async fn optimize_image(
    app: AppHandle,
    state: State<'_, ShellState>,
    input: ImageOptimizeInput,
) -> Result<OptimizeOutput, String> {
    let cfg = load_cfg(&app).map_err(map_err)?;
    let job_id = cancel::start_job().map_err(map_err)?;
    let _ = app.emit("optimize-started", &job_id);
    let result = run_image_job(&app, &state, &cfg, input);
    cancel::finish_job(&job_id);
    result
        .map(|mut output| {
            output.job_id = job_id;
            output
        })
        .map_err(map_err)
}

fn run_image_job(
    app: &AppHandle,
    state: &ShellState,
    cfg: &ShellConfig,
    input: ImageOptimizeInput,
) -> Result<OptimizeOutput, OptimizeError> {
    if input.user_text.trim().is_empty() {
        return Err(error_ui::err(
            "IMAGE_PROMPT_USER_INPUT_REQUIRED",
            "请先填写原稿。",
        ));
    }
    let library = ensure_library(app, cfg)?;
    let templates = template_root(app, cfg)?;
    let template = skills::freeze_internal_task(&templates, skills::TEMPLATE_IMAGE, input.language.as_deref().unwrap_or_else(|| cfg.language()))?;
    let method = match input.method_id.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(id) => Some(skills::freeze_named(&library, id)?),
        None => None,
    };
    for path in &input.image_paths {
        attachments::require_picked_path(&state.picked, Path::new(path))?;
    }
    let language = input.language.unwrap_or_else(|| cfg.language().to_string());
    let request = ImageOptimizeRequest {
        task_id: String::new(),
        user_text: input.user_text,
        method,
        template,
        language,
        reference_image_count: input.image_paths.len() as u32,
        images: Vec::new(),
        text_vision: TextVisionCaps {
            supports_vision: cfg.supports_vision,
        },
        channel: VisionChannel::ReferenceOnly,
    };
    bgx_prompt_optimize::reject_if_over_submitted_cap(request.reference_image_count)?;
    let mut completer = ShellTextCompleter::new(cfg.clone(), false)?;
    invoke_run_image(request, &mut completer)
}

fn invoke_run_image(
    request: ImageOptimizeRequest,
    completer: &mut impl TextCompleter,
) -> Result<OptimizeOutput, OptimizeError> {
    let result = run_image(request, completer)?;
    Ok(output_from_image(result))
}

fn output_from_image(result: ImageOptimizeResult) -> OptimizeOutput {
    OptimizeOutput {
        kind: "image".into(),
        text: result.optimized_prompt,
        raw_reply: Some(result.raw_reply),
        job_id: String::new(),
        profile_id: None,
        profile_version: None,
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoOptimizeInput {
    pub source_text: String,
    pub change_request: Option<String>,
    pub target: VideoTargetForm,
    pub format_method_id: Option<String>,
    pub content_method_id: Option<String>,
    pub references: Vec<ReferencePick>,
}

#[tauri::command]
pub async fn optimize_video(
    app: AppHandle,
    state: State<'_, ShellState>,
    input: VideoOptimizeInput,
) -> Result<OptimizeOutput, String> {
    let cfg = load_cfg(&app).map_err(map_err)?;
    let job_id = cancel::start_job().map_err(map_err)?;
    let _ = app.emit("optimize-started", &job_id);
    let result = run_video_job(&app, &state, &cfg, input);
    cancel::finish_job(&job_id);
    result
        .map(|mut output| {
            output.job_id = job_id;
            output
        })
        .map_err(map_err)
}

fn run_video_job(
    app: &AppHandle,
    state: &ShellState,
    cfg: &ShellConfig,
    input: VideoOptimizeInput,
) -> Result<OptimizeOutput, OptimizeError> {
    if input.source_text.trim().is_empty() {
        return Err(error_ui::err(
            "VIDEO_SOURCE_REQUIRED",
            "请先粘贴要优化的正文。",
        ));
    }
    let built = snapshot::build_target_snapshot(&input.target)?;
    let library = ensure_library(app, cfg)?;
    let templates = template_root(app, cfg)?;
    let format_method = match input
        .format_method_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(id) => skills::freeze_named(&library, id)?,
        None => {
            return Err(missing_format_method_error(
                &input.target.profile_id,
                input.target.profile_version,
            ));
        }
    };
    let content_method = match input
        .content_method_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(id) => Some(skills::freeze_named(&library, id)?),
        None => None,
    };
    let mut reference_inventory = Vec::with_capacity(input.references.len());
    let mut reference_ordinals = BTreeMap::new();
    let mut reference_context = Vec::with_capacity(input.references.len());
    let mut has_video = false;
    for (index, item) in input.references.iter().enumerate() {
        let path = Path::new(&item.path);
        attachments::require_picked_path(&state.picked, path)?;
        let item_id = format!("ref-{}", index + 1);
        if item.media_kind == "reference_video" || item.usage.contains("video") {
            has_video = true;
        }
        let ordinal = reference_ordinals.entry(item.media_kind.as_str()).or_insert(0);
        *ordinal += 1;
        let audio_usage = match item.usage.as_str() {
            "voice_timbre" => Some("voice_timbre".into()),
            "audio_copy" => Some("soundtrack_copy".into()),
            _ => None,
        };
        reference_context.push(serde_json::json!({
            "itemId": item_id,
            "mediaKind": item.media_kind,
            "ordinalWithinKind": *ordinal,
            "usage": item.usage,
        }));
        reference_inventory.push(bgx_video_prompt_contract::inventory::ReferenceMediaInventoryItem {
            item_id: item_id.clone(),
            media_kind: item.media_kind.clone(),
            ordinal_within_kind: *ordinal,
            audio_usage,
            ..Default::default()
        });
    }
    let mut template_set = BTreeMap::new();
    for task in [
        skills::TEMPLATE_VIDEO_CONTENT,
        skills::TEMPLATE_VIDEO_CONTENT_REVIEW,
        skills::TEMPLATE_VIDEO_EXPRESSION,
        skills::TEMPLATE_VIDEO_EXPRESSION_REVIEW,
    ] {
        let language = if input.target.expression_language == "en" { "en" } else { "zh" };
        if let Ok(frozen) = skills::freeze_internal_task(&templates, task, language) {
            template_set.insert(task.to_string(), frozen);
        }
    }
    if !template_set.contains_key(skills::TEMPLATE_VIDEO_EXPRESSION) {
        return Err(error_ui::err(
            "INTERNAL_PROMPT_CONFIG",
            "找不到 video-expression 内部模板。",
        ));
    }
    let method = frozen_method_from_skill(
        &format_method,
        &input.target.profile_id,
        input.target.profile_version,
    );
    let change_request = input.change_request
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    let request = VideoOptimizeRequest {
        source_text: input.source_text.clone(),
        change_request: change_request.clone(),
        target_snapshot: built.snapshot,
        format_method,
        content_method,
        templates: template_set,
        reference_inventory,
        // Current standalone entry has no 扩写 policy and no frozen negative context.
        expansion_policy: None,
        negative_context: None,
        mother_author: Some(bgx_prompt_optimize::MotherAuthorBind {
            task_context: serde_json::json!({
                "currentDraft": input.source_text,
                "changeRequest": change_request,
                "references": reference_context,
            }),
            expected_shot_ids: vec!["shot-1".into()],
            source_unit_ids: vec![],
            quote_candidates: vec![],
            source_scope: None,
            source_mode: "optimize_existing".into(),
            subject_ids: vec![],
        }),
        mother_review_base: None,
        method,
        identity_locks: Vec::new(),
        allow_inferred: false,
        existing_mother: None,
    };
    let mut completer = ShellTextCompleter::new(cfg.clone(), has_video)?;
    completer.reject_unsupported_video()?;
    invoke_run_video(request, &mut completer)
}


fn sha256_hex(bytes: impl AsRef<[u8]>) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes.as_ref())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn frozen_method_from_skill(
    skill: &FrozenSkillBytes,
    profile_id: &str,
    profile_version: u64,
) -> bgx_video_prompt_contract::projection::VideoPromptFrozenMethod {
    let body_sha256 = sha256_hex(skill.body.as_bytes());
    let resources: Vec<serde_json::Value> = skill
        .resources
        .iter()
        .map(|(path, bytes)| {
            serde_json::json!({
                "resourceRef": path,
                "sha256": sha256_hex(bytes),
            })
        })
        .collect();
    serde_json::from_value(serde_json::json!({
        "selectionKind": "custom",
        "activationId": skill.stable_id,
        "stableId": skill.stable_id,
        "packageDigest": skill.package_digest,
        "name": skill.stable_id,
        "bodySha256": body_sha256,
        "resources": resources,
        "bindingProfileId": profile_id,
        "bindingProfileVersion": profile_version
    }))
    .expect("frozen skill bytes must map to official VideoPromptFrozenMethod")
}

fn missing_format_method_error(profile_id: &str, profile_version: u64) -> OptimizeError {
    // Never invent a default method or silently fall back to generic.
    let default_missing = production_registry()
        .ok()
        .and_then(|registry| {
            registry
                .identities()
                .iter()
                .find(|item| item.profile_id == profile_id && item.profile_version == profile_version)
                .map(|item| item.default_method_name.is_none())
        })
        .unwrap_or(true);
    if default_missing {
        error_ui::err("VIDEO_FORMAT_METHOD_REQUIRED", error_ui::LTX_METHOD_REQUIRED)
    } else {
        error_ui::err(
            "VIDEO_FORMAT_METHOD_REQUIRED",
            "请选择一个写法 Skill。壳不会改用通用档案。",
        )
    }
}

fn invoke_run_video(
    request: VideoOptimizeRequest,
    completer: &mut impl TextCompleter,
) -> Result<OptimizeOutput, OptimizeError> {
    let profile_id = request
        .target_snapshot
        .get("profileId")
        .and_then(|value| value.as_str())
        .map(str::to_string);
    let profile_version = request
        .target_snapshot
        .get("profileVersion")
        .and_then(|value| value.as_u64());
    let result = run_video_optimize(request, completer)?;
    output_from_video(result, profile_id, profile_version)
}

fn output_from_video(
    result: VideoOptimizeResult,
    profile_id: Option<String>,
    profile_version: Option<u64>,
) -> Result<OptimizeOutput, OptimizeError> {
    if let Some(rejected) = result.rejected {
        let mut error = OptimizeError::new(rejected.code, rejected.detail);
        if let Some(raw) = rejected.raw_reply {
            error = error.with_raw_reply(raw);
        }
        return Err(error);
    }
    let Some(final_prompt) = result
        .final_prompt
        .map(|text| text)
        .filter(|text| !text.trim().is_empty())
    else {
        // Do not invent a prompt from `draft`.
        return Err(error_ui::err(
            "VIDEO_NO_FINAL_PROMPT",
            error_ui::VIDEO_NO_FINAL_PROMPT,
        ));
    };
    Ok(OptimizeOutput {
        kind: "video".into(),
        text: final_prompt,
        raw_reply: None,
        job_id: String::new(),
        profile_id,
        profile_version,
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiteraryOptimizeInput {
    pub source_text: String,
    pub creative_method_id: Option<String>,
    pub language: Option<String>,
}

#[tauri::command]
pub async fn optimize_literary(app: AppHandle, input: LiteraryOptimizeInput) -> Result<OptimizeOutput, String> {
    let cfg = load_cfg(&app).map_err(map_err)?;
    let job_id = cancel::start_job().map_err(map_err)?;
    let _ = app.emit("optimize-started", &job_id);
    let result = run_literary_job(&app, &cfg, input);
    cancel::finish_job(&job_id);
    result
        .map(|mut output| {
            output.job_id = job_id;
            output
        })
        .map_err(map_err)
}

fn run_literary_job(
    app: &AppHandle,
    cfg: &ShellConfig,
    input: LiteraryOptimizeInput,
) -> Result<OptimizeOutput, OptimizeError> {
    if input.source_text.trim().is_empty() {
        return Err(error_ui::err(
            "LITERARY_SOURCE_REQUIRED",
            "请先粘贴要优化的正文。独立壳不读取故事库，也不填写邻居镜头。",
        ));
    }
    let library = ensure_library(app, cfg)?;
    let templates = template_root(app, cfg)?;
    let template = skills::freeze_internal_task(&templates, skills::TEMPLATE_LITERARY, input.language.as_deref().unwrap_or_else(|| cfg.language()))?;
    let creative_method = match input
        .creative_method_id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(id) => Some(skills::freeze_named(&library, id)?),
        None => None,
    };
    let language = input.language.unwrap_or_else(|| cfg.language().to_string());
    // LiteraryPromptOpen (content-authoring-stages-v1) only requires `prompt` and
    // forbids additional properties. Neighbor / frame / pad context stays absent
    // rather than being invented as empty story fields.
    let open = serde_json::json!({ "prompt": input.source_text });
    let request = LiteraryOptimizeRequest {
        task_id: String::new(),
        open,
        template,
        creative_method,
        language,
        output_contract: None,
        image_data_urls: Vec::new(),
    };
    let mut completer = ShellTextCompleter::new(cfg.clone(), false)?;
    invoke_run_literary(request, &mut completer)
}

fn invoke_run_literary(
    request: LiteraryOptimizeRequest,
    completer: &mut impl TextCompleter,
) -> Result<OptimizeOutput, OptimizeError> {
    let result = run_literary(request, completer)?;
    Ok(output_from_literary(result))
}

fn output_from_literary(result: LiteraryOptimizeResult) -> OptimizeOutput {
    OptimizeOutput {
        kind: "literary".into(),
        text: result.prompt,
        raw_reply: Some(result.raw_reply),
        job_id: String::new(),
        profile_id: None,
        profile_version: None,
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoCompressInput {
    pub source_text: String,
    pub target_max_chars: u32,
}

#[tauri::command]
pub async fn compress_video_prompt(
    app: AppHandle,
    input: VideoCompressInput,
) -> Result<OptimizeOutput, String> {
    let cfg = load_cfg(&app).map_err(map_err)?;
    let job_id = cancel::start_job().map_err(map_err)?;
    let _ = app.emit("optimize-started", &job_id);
    let result = run_compress_job(&cfg, input);
    cancel::finish_job(&job_id);
    result
        .map(|mut output| {
            output.job_id = job_id;
            output
        })
        .map_err(map_err)
}

fn run_compress_job(
    cfg: &ShellConfig,
    input: VideoCompressInput,
) -> Result<OptimizeOutput, OptimizeError> {
    if input.source_text.trim().is_empty() {
        return Err(error_ui::err(
            "VIDEO_SOURCE_REQUIRED",
            "请先粘贴要压缩的正文。",
        ));
    }
    if input.target_max_chars == 0 {
        return Err(error_ui::err(
            "COMPRESS_CAP_REQUIRED",
            error_ui::COMPRESS_CAP_REQUIRED,
        ));
    }
    // Compression does not load a Skill and must not enter run_video_optimize.
    let request = VideoCompressRequest {
        source_text: input.source_text,
        target_max_chars: input.target_max_chars,
    };
    let mut completer = ShellTextCompleter::new(cfg.clone(), false)?;
    let result = shared_compress_video_prompt(request, &mut completer)?;
    Ok(output_from_compress(result))
}

fn output_from_compress(result: VideoCompressResult) -> OptimizeOutput {
    OptimizeOutput {
        kind: "compress".into(),
        text: result.compressed_text,
        raw_reply: Some(result.raw_reply),
        job_id: String::new(),
        profile_id: None,
        profile_version: None,
    }
}

#[tauri::command]
pub fn cancel_optimize(job_id: String) -> Result<(), String> {
    cancel::request_cancel(&job_id).map_err(map_err)
}

#[tauri::command]
pub async fn save_result(app: AppHandle, text: String) -> Result<(), String> {
    if text.trim().is_empty() {
        return Err("没有可保存的成稿。".into());
    }
    let (tx, rx) = oneshot::channel();
    app.dialog()
        .file()
        .add_filter("文本", &["txt"])
        .set_file_name("optimized-prompt.txt")
        .set_title("保存成稿")
        .save_file(move |path| {
            let _ = tx.send(path);
        });
    let picked = rx.await.map_err(|_| "保存已取消".to_string())?;
    let Some(FilePath::Path(path)) = picked else {
        return Ok(());
    };
    std::fs::write(&path, text).map_err(|error| format!("无法保存成稿：{error}"))?;
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OptimizeOutput {
    pub kind: String,
    pub text: String,
    pub raw_reply: Option<String>,
    pub job_id: String,
    pub profile_id: Option<String>,
    pub profile_version: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use bgx_prompt_optimize::image_job::SubmittedImage;

    #[test]
    fn image_output_uses_optimized_prompt() {
        let output = output_from_image(ImageOptimizeResult {
            optimized_prompt: "out".into(),
            raw_reply: "raw".into(),
            finish_reason: Some("stop".into()),
            output_incomplete: false,
            attached_image_count: 0,
            vision_truncated: false,
            vision_skip_reason: String::new(),
            input_mode: "text_to_image".into(),
        });
        assert_eq!(output.kind, "image");
        assert_eq!(output.text, "out");
        assert_eq!(output.raw_reply.as_deref(), Some("raw"));
    }

    #[test]
    fn literary_output_uses_prompt_field() {
        let output = output_from_literary(LiteraryOptimizeResult {
            prompt: "literary-out".into(),
            raw_reply: "raw".into(),
            finish_reason: None,
            output_incomplete: false,
        });
        assert_eq!(output.kind, "literary");
        assert_eq!(output.text, "literary-out");
    }

    #[test]
    fn video_output_requires_final_prompt_and_does_not_use_draft() {
        let err = output_from_video(
            bgx_prompt_optimize::VideoExpressionResult {
                draft: None,
                rejected: None,
                final_prompt: None,
                source_map: Vec::new(),
                diagnostics: Vec::new(),
            }
            .into(),
            Some("ltx".into()),
            Some(1),
        )
        .unwrap_err();
        assert_eq!(err.message, error_ui::VIDEO_NO_FINAL_PROMPT);
        let rejected = output_from_video(
            bgx_prompt_optimize::VideoExpressionResult {
                draft: None,
                rejected: Some(bgx_prompt_optimize::video_job::VideoExpressionRejected {
                    code: "VIDEO_PROMPT_PROJECTOR_MISSING".into(),
                    detail: "missing".into(),
                    raw_reply: Some("raw".into()),
                }),
                final_prompt: Some("should-not-be-used".into()),
                source_map: Vec::new(),
                diagnostics: Vec::new(),
            }
            .into(),
            None,
            None,
        )
        .unwrap_err();
        assert_eq!(rejected.code, "VIDEO_PROMPT_PROJECTOR_MISSING");
        assert_eq!(rejected.message, "missing");
        let ok = output_from_video(
            bgx_prompt_optimize::VideoExpressionResult {
                draft: None,
                rejected: None,
                final_prompt: Some("final".into()),
                source_map: Vec::new(),
                diagnostics: Vec::new(),
            }
            .into(),
            Some("h3".into()),
            Some(1),
        )
        .unwrap();
        assert_eq!(ok.text, "final");
        assert_eq!(ok.profile_id.as_deref(), Some("h3"));
    }

    #[test]
    fn ltx_without_a_format_method_does_not_fall_back_to_generic() {
        let err = missing_format_method_error("ltx", 1);
        assert_eq!(err.message, error_ui::LTX_METHOD_REQUIRED);
        assert!(!err.message.contains("generic"));
    }

    #[test]
    fn shell_does_not_truncate_over_cap_images_and_uses_core_reject() {
        let submitted = MAX_IPC_RELS + 1;
        let images: Vec<_> = (1..=submitted)
            .map(|order| SubmittedImage {
                bytes: b"not-a-real-decode".to_vec(),
                declared_order: order,
            })
            .collect();
        assert_eq!(images.len() as u32, submitted);
        let err = reject_if_over_submitted_cap(images.len() as u32).unwrap_err();
        assert_eq!(err.code, "IMAGE_PROMPT_REFERENCE_LIMIT");
        assert_eq!(
            err.message,
            "参考图超过应用请求数量上限，未发送本次请求。"
        );
        assert_eq!(images.len() as u32, MAX_IPC_RELS + 1);
        assert_eq!(images.first().map(|item| item.declared_order), Some(1));
        assert_eq!(images.last().map(|item| item.declared_order), Some(submitted));
    }
}
