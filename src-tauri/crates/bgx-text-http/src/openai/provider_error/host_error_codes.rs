//! Host `open_platform::error_codes` string values used by finish-enrich.
//! Copied verbatim so this crate does not depend on the main package.

pub mod workflow {
    pub const RESOLUTION_CONTRACT_INVALID: &str = "workflowResolutionContractInvalid";
    pub const MAPPING_STALE: &str = "workflowMappingStale";
    pub const MAPPING_NEEDS_CONFIRMATION: &str = "workflowMappingNeedsConfirmation";
    pub const PROMPT_ENHANCER_REQUIRES_CONFIGURATION: &str = "workflowPromptEnhancerRequiresConfiguration";
}

pub mod comfyui_http {
    pub const PARAM_CONTRACT_AMBIGUOUS: &str = "comfyuiParamContractAmbiguous";
    pub const VDN_NODE_MISSING: &str = "comfyuiVdnNodeMissing";
    pub const MISSING_NODE_TYPE: &str = "comfyuiMissingNodeType";
}

pub mod generic {
    pub const MISSING_NODES: &str = "genericMissingNodes";
}
