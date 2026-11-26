//! User-facing messages. Prefer existing host wording when one is specified.

use bgx_prompt_optimize::{error_codes, OptimizeError};

pub const TEXT_MODEL_NOT_CONFIGURED: &str = "还没有配置文本模型";
pub const LTX_METHOD_REQUIRED: &str =
    "请明确选择写法 Skill。该档案没有默认写法包，不会改用通用档案。";
pub const COMPRESS_CAP_REQUIRED: &str =
    "请填写大于 0 的目标字数上限。壳不会编造默认上限。";
pub const VIDEO_NO_FINAL_PROMPT: &str = "这次视频优化没有成稿。";
pub const NO_STORY_CONTEXT: &str = "独立壳不读取故事库，也不填写邻居镜头或角色锁。";

pub fn err(code: &str, message: impl Into<String>) -> OptimizeError {
    OptimizeError::new(code, message.into())
}

pub fn cancelled() -> OptimizeError {
    err(error_codes::OPTIMIZE_CANCELLED, "已取消")
}

pub fn timeout() -> OptimizeError {
    err(error_codes::OPTIMIZE_TIMEOUT, "优化超时")
}

pub fn text_not_configured() -> OptimizeError {
    err("TEXT_MODEL_NOT_CONFIGURED", TEXT_MODEL_NOT_CONFIGURED)
}

pub fn to_command_error(error: OptimizeError) -> String {
    // Commands return the user-facing message. The machine code stays in logs only.
    tracing::error!(code = %error.code, "prompt-optimize command failed");
    if error.code == error_codes::VIDEO_PROMPT_MODEL_REFERENCE_INVALID {
        return "文本模型返回了无效的素材编号，当前正文已保留。".into();
    }
    error.message
}
