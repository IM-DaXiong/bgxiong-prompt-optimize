//! CT03/C08: typed contract violation. `Display` keeps the historical `CODE: detail` text so
//! existing String error channels carry the full reason; the fields are the machine diagnosis.

use serde::Serialize;

/// Upper bound of branch reasons kept per combinator (C08: bounded detail; full text in logs).
pub const MAX_BRANCH_REASONS: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContractViolation {
    /// Stable code, e.g. `CREATOR_CONTRACT_INVALID`.
    pub code: String,
    /// Contract file the violation belongs to (repository-relative name, never a machine path).
    pub contract_file: String,
    /// Schema or example name (`requestProjection`, `schemas/requestReplyArgs`, ...).
    pub schema_ref: String,
    /// Where in the instance / schema the violation sits (boxed: keeps `Result<_, ContractViolation>` small).
    #[serde(flatten)]
    pub location: Box<ViolationLocation>,
    /// Human-readable structural reason.
    pub message: String,
}

/// Instance and schema position of a [`ContractViolation`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ViolationLocation {
    /// Path inside the checked instance (`requestProjection.explanation.acceptedReply`).
    pub instance_path: String,
    /// Keyword path inside the schema (`oneOf`, `required`, ...).
    pub schema_path: String,
    /// One line per rejected combinator branch (bounded by MAX_BRANCH_REASONS).
    pub branch_reasons: Vec<String>,
}

pub const CREATOR_CONTRACT_INVALID: &str = "CREATOR_CONTRACT_INVALID";

impl ContractViolation {
    pub fn new(contract_file: &str, schema_ref: &str, message: impl Into<String>) -> Self {
        Self {
            code: CREATOR_CONTRACT_INVALID.to_string(),
            contract_file: contract_file.to_string(),
            schema_ref: schema_ref.to_string(),
            location: Box::default(),
            message: message.into(),
        }
    }

    pub fn with_value_error(mut self, e: crate::schema_value::ValueError) -> Self {
        self.location.instance_path = e.instance_path;
        self.location.schema_path = e.schema_path;
        self.location.branch_reasons = e.branch_reasons;
        self.message = format!("{}{}", self.message, e.message);
        self
    }
}

impl std::fmt::Display for ContractViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {} {}", self.code, self.contract_file, self.message)?;
        if !self.schema_ref.is_empty() {
            write!(f, " [schemaRef={}]", self.schema_ref)?;
        }
        if !self.location.instance_path.is_empty() {
            write!(f, " [instancePath={}]", self.location.instance_path)?;
        }
        if !self.location.schema_path.is_empty() {
            write!(f, " [schemaPath={}]", self.location.schema_path)?;
        }
        if !self.location.branch_reasons.is_empty() {
            write!(f, " [branches: {}]", self.location.branch_reasons.join(" | "))?;
        }
        Ok(())
    }
}

impl std::error::Error for ContractViolation {}