//! Shell-owned config.json. Keys never appear in command return values or tracing.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use bgx_prompt_optimize::OptimizeError;

use crate::error_ui;
use crate::paths;

/// Cloud kinds shown on the settings page, matching the host registry names.
pub const CLOUD_PROVIDER_KINDS: &[&str] = &[
    "text_openai_compat",
    "text_volc_ark_compat",
    "text_deepseek_compat",
    "text_custom",
    "text_qwen",
    "text_minimax_m3",
];

pub const LOCAL_PROVIDER_KIND: &str = "text_local_model";
pub const ADAPTER_PROVIDER_KIND: &str = "text_vendor_adapter";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ShellConfig {
    pub provider_kind: String,
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub saved_models: Vec<String>,
    pub api_key: Option<String>,
    /// Only applied when it is a positive number. Empty / zero means no whole-request timeout
    /// (same as the host long-running client). Connect timeout stays 30 seconds.
    pub timeout_secs: Option<u64>,
    /// Official catalog `caps_json.maxOutputTokens` (u32 > 0). Used for MiniMax
    /// when this shell has no model-directory cache. Never invent a default budget.
    pub max_output_tokens: Option<u32>,
    pub skill_library_root: Option<String>,
    pub internal_template_root: Option<String>,
    pub adapter_package_root: Option<String>,
    pub local_model_path: Option<String>,
    pub language: Option<String>,
    pub supports_vision: bool,
}

impl ShellConfig {
    pub fn from_json(text: &str) -> Result<Self, OptimizeError> {
        let value: ShellConfig = serde_json::from_str(text).map_err(|_| {
            error_ui::err("SHELL_CONFIG_INVALID", "配置文件不是有效的 JSON。")
        })?;
        value.validate()?;
        Ok(value)
    }

    pub fn load_file(path: &Path) -> Result<Self, OptimizeError> {
        if !path.is_file() {
            return Err(error_ui::text_not_configured());
        }
        let text = fs::read_to_string(path)
            .map_err(|_| error_ui::err("SHELL_CONFIG_READ", "无法读取配置文件。"))?;
        Self::from_json(&text)
    }

    pub fn save_file(&self, path: &Path) -> Result<(), OptimizeError> {
        self.validate()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                error_ui::err("SHELL_CONFIG_WRITE", format!("无法创建配置目录：{error}"))
            })?;
        }
        let text = serde_json::to_string_pretty(self)
            .map_err(|_| error_ui::err("SHELL_CONFIG_WRITE", "无法序列化配置。"))?;
        fs::write(path, text)
            .map_err(|error| error_ui::err("SHELL_CONFIG_WRITE", format!("无法写入配置：{error}")))?;
        restrict_to_current_user(path)?;
        Ok(())
    }

    pub fn public_view(&self) -> ShellConfigPublic {
        let mut saved_models = self.saved_models.clone();
        if let Some(model) = self.model_choice() {
            if !saved_models.contains(model) {
                saved_models.push(model.clone());
            }
        }
        ShellConfigPublic {
            provider_kind: self.provider_kind.clone(),
            base_url: self.base_url.clone(),
            model: self.model.clone(),
            saved_models,
            has_api_key: self
                .api_key
                .as_deref()
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false),
            timeout_secs: self.timeout_secs,
            max_output_tokens: self.official_max_output_tokens(),
            skill_library_root: self.skill_library_root.clone(),
            internal_template_root: self.internal_template_root.clone(),
            adapter_package_root: self.adapter_package_root.clone(),
            local_model_path: self.local_model_path.clone(),
            language: self.language.clone(),
            supports_vision: self.supports_vision,
        }
    }

    pub fn language(&self) -> &str {
        match self.language.as_deref() {
            Some("en") => "en",
            _ => "zh",
        }
    }

    pub fn is_cloud_kind(kind: &str) -> bool {
        CLOUD_PROVIDER_KINDS.contains(&kind)
    }

    pub fn remember_model(&mut self, previous: Option<Self>) {
        self.saved_models.clear();
        if let Some(previous) = previous.filter(|old| {
            old.provider_kind == self.provider_kind && old.base_url == self.base_url
                && (self.provider_kind != ADAPTER_PROVIDER_KIND
                    || old.adapter_package_root == self.adapter_package_root)
        }) {
            let previous_model = previous.model_choice().cloned();
            self.saved_models = previous.saved_models;
            if let Some(model) = previous_model {
                self.saved_models.push(model);
            }
            if !has_text(&self.api_key) {
                self.api_key = previous.api_key;
            }
        }
        if let Some(model) = self.model_choice().cloned() {
            self.saved_models.push(model);
        }
        self.saved_models.retain(|model| !model.trim().is_empty());
        self.saved_models.sort();
        self.saved_models.dedup();
    }

    fn model_choice(&self) -> Option<&String> {
        if self.provider_kind == LOCAL_PROVIDER_KIND {
            return self.local_model_path.as_ref();
        }
        self.model.as_ref()
    }

    pub fn validate(&self) -> Result<(), OptimizeError> {
        if self.provider_kind.trim().is_empty() {
            return Err(error_ui::text_not_configured());
        }
        match self.provider_kind.as_str() {
            kind if Self::is_cloud_kind(kind) => {
                if !has_text(&self.base_url) {
                    // T24: missing baseUrl is Err. The button shows the human sentence.
                    return Err(error_ui::text_not_configured());
                }
                if !has_text(&self.model) {
                    return Err(error_ui::text_not_configured());
                }
                if !has_text(&self.api_key) {
                    return Err(error_ui::text_not_configured());
                }
            }
            LOCAL_PROVIDER_KIND => {
                if !has_text(&self.local_model_path) {
                    return Err(error_ui::err(
                        "LOCAL_MODEL_PATH_REQUIRED",
                        "未指定本地模型",
                    ));
                }
                let path = PathBuf::from(self.local_model_path.as_deref().unwrap_or(""));
                paths::require_absolute(&path, "本地模型路径")?;
            }
            ADAPTER_PROVIDER_KIND => {
                if !has_text(&self.adapter_package_root) {
                    return Err(error_ui::err(
                        "ADAPTER_PACKAGE_ROOT_REQUIRED",
                        "未选择模型 / 找不到包",
                    ));
                }
                let path = PathBuf::from(self.adapter_package_root.as_deref().unwrap_or(""));
                paths::require_absolute(&path, "适配包目录")?;
            }
            _ => {
                return Err(error_ui::err(
                    "TEXT_PROVIDER_UNKNOWN",
                    format!("不支持的文本渠道：{}", self.provider_kind),
                ));
            }
        }
        if let Some(root) = self.skill_library_root.as_deref().filter(|s| !s.trim().is_empty()) {
            paths::require_absolute(Path::new(root), "技能库根")?;
        }
        if let Some(root) = self
            .internal_template_root
            .as_deref()
            .filter(|s| !s.trim().is_empty())
        {
            paths::require_absolute(Path::new(root), "内部模板根")?;
        }
        Ok(())
    }

    /// Whole-request timeout: only a positive number is a limit. Never invent a short default.
    pub fn request_timeout_secs(&self) -> Option<u64> {
        self.timeout_secs.filter(|s| *s > 0)
    }

    /// Official maxOutputTokens only when it is a positive u32. Zero / missing = unset.
    pub fn official_max_output_tokens(&self) -> Option<u32> {
        self.max_output_tokens.filter(|value| *value > 0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ShellConfigPublic {
    pub provider_kind: String,
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub saved_models: Vec<String>,
    pub has_api_key: bool,
    pub timeout_secs: Option<u64>,
    pub max_output_tokens: Option<u32>,
    pub skill_library_root: Option<String>,
    pub internal_template_root: Option<String>,
    pub adapter_package_root: Option<String>,
    pub local_model_path: Option<String>,
    pub language: Option<String>,
    pub supports_vision: bool,
}

fn has_text(value: &Option<String>) -> bool {
    value.as_deref().map(|s| !s.trim().is_empty()).unwrap_or(false)
}

fn restrict_to_current_user(path: &Path) -> Result<(), OptimizeError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(path)
            .map_err(|error| error_ui::err("SHELL_CONFIG_PERM", error.to_string()))?
            .permissions();
        perms.set_mode(0o600);
        fs::set_permissions(path, perms)
            .map_err(|error| error_ui::err("SHELL_CONFIG_PERM", error.to_string()))?;
    }
    #[cfg(windows)]
    {
        restrict_windows_acl(path)?;
    }
    Ok(())
}

#[cfg(windows)]
fn restrict_windows_acl(path: &Path) -> Result<(), OptimizeError> {
    let path_str = path.to_string_lossy().to_string();
    let user = std::env::var("USERNAME").unwrap_or_default();
    if user.is_empty() {
        return Err(error_ui::err("SHELL_CONFIG_PERM", "无法确定当前用户，未能限制配置文件权限。"));
    }
    let grant = format!("{user}:(R,W)");
    let output = std::process::Command::new("icacls")
        .args([&path_str, "/inheritance:r", "/grant:r", &grant])
        .output()
        .map_err(|error| error_ui::err("SHELL_CONFIG_PERM", format!("无法设置配置权限：{error}")))?;
    if !output.status.success() {
        return Err(error_ui::err(
            "SHELL_CONFIG_PERM",
            "无法把配置文件权限限制为当前用户。",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_base_url_is_err() {
        let json = r#"{"providerKind":"text_qwen","model":"qwen-plus","apiKey":"secret"}"#;
        let err = ShellConfig::from_json(json).expect_err("baseUrl is required for cloud kinds");
        assert_eq!(err.message, error_ui::TEXT_MODEL_NOT_CONFIGURED);
    }

    #[test]
    fn public_view_does_not_echo_api_key() {
        let json = r#"{
            "providerKind":"text_openai_compat",
            "baseUrl":"https://example.invalid/v1",
            "model":"demo",
            "apiKey":"super-secret"
        }"#;
        let cfg = ShellConfig::from_json(json).expect("valid cloud config");
        let view = serde_json::to_value(cfg.public_view()).unwrap();
        assert!(view.get("apiKey").is_none());
        assert_eq!(view["hasApiKey"], true);
        assert_eq!(cfg.api_key.as_deref(), Some("super-secret"));
    }

    #[test]
    fn empty_timeout_does_not_invent_a_short_default() {
        let json = r#"{
            "providerKind":"text_openai_compat",
            "baseUrl":"https://example.invalid/v1",
            "model":"demo",
            "apiKey":"x"
        }"#;
        let cfg = ShellConfig::from_json(json).unwrap();
        assert_eq!(cfg.request_timeout_secs(), None);
    }

    #[test]
    fn local_model_without_path_uses_existing_wording() {
        let json = r#"{"providerKind":"text_local_model"}"#;
        let err = ShellConfig::from_json(json).expect_err("path required");
        assert_eq!(err.message, "未指定本地模型");
    }
}
