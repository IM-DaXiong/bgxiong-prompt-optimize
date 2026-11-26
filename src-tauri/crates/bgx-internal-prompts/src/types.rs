use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::error::PromptError;

pub const FORMAT_VERSION: u32 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Zh,
    En,
}

impl Language {
    pub fn key(self) -> &'static str {
        match self {
            Self::Zh => "zh",
            Self::En => "en",
        }
    }

    pub fn parse(value: &str) -> Result<Self, PromptError> {
        match value {
            "zh" => Ok(Self::Zh),
            "en" => Ok(Self::En),
            _ => Err(PromptError::new("missing-language", value)),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskKind {
    Request,
    Material,
}

impl TaskKind {
    pub fn key(self) -> &'static str {
        match self {
            Self::Request => "request",
            Self::Material => "material",
        }
    }
}

/// Complete rule bytes for one language. Restore does not take a directory.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FrozenRuleSet {
    pub format_version: u32,
    pub language: Language,
    pub tasks: BTreeMap<String, TaskKind>,
    pub files: BTreeMap<String, String>,
    pub digest: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RenderedMessage {
    pub id: String,
    pub role: String,
    pub content: String,
}
