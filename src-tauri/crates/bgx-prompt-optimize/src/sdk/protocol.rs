use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::OptimizeError;

pub const PROTOCOL_VERSION: u64 = 1;

#[derive(Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TextMode {
    #[default]
    Configured,
    Callback,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OptimizeArgs {
    pub target_id: String,
    pub source_prompt: String,
    #[serde(default)]
    pub text_mode: TextMode,
    #[serde(default)]
    pub expected_method_digest: Option<String>,
}

impl OptimizeArgs {
    pub fn validate(&self) -> Result<(), OptimizeError> {
        if self.target_id.is_empty() || self.source_prompt.trim().is_empty() {
            return Err(OptimizeError::new(
                "INVALID_FRAME",
                "请选择目标并输入原始提示词。",
            ));
        }
        if self.expected_method_digest.as_ref().is_some_and(|digest| {
            !digest.strip_prefix("sha256:").is_some_and(|hex| {
                hex.len() == 64
                    && hex
                        .bytes()
                        .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
            })
        }) {
            return Err(OptimizeError::new("INVALID_FRAME", "无效的 Skill 摘要。"));
        }
        Ok(())
    }
}

pub fn success(id: &str, result: impl Serialize) -> Value {
    json!({"v":PROTOCOL_VERSION,"id":id,"event":"done","result":result})
}

pub fn failure(id: Option<&str>, error: &OptimizeError) -> Value {
    json!({"v":PROTOCOL_VERSION,"id":id,"event":"error","error":{
        "code":error.code,"message":error.message
    }})
}
