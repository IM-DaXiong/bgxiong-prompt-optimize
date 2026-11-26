use std::fmt;

/// Stable machine error. `detail` names the rule or relative resource, never author text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptError {
    pub reason: &'static str,
    pub detail: String,
}

impl PromptError {
    pub fn new(reason: &'static str, detail: impl Into<String>) -> Self {
        Self {
            reason,
            detail: detail.into(),
        }
    }
}

impl fmt::Display for PromptError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "INTERNAL_PROMPT_CONFIG: {}: {}",
            self.reason, self.detail
        )
    }
}

impl std::error::Error for PromptError {}
