//! 模块：Creator 工具与规则渐进发现；保持同一合同、执行内核与离线可验证边界。
//! CT03: the one pure verifier of the Creator contracts. build.rs (build-dependency), the
//! runtime host (dependency) and the `verify-creator-contract` CLI call the same functions, so a
//! contract that fails here fails the build before any constant or client pack is generated and
//! can never reach a running host. No Tauri, AppContainer, database, network, model or UI.

pub mod catalog;
pub mod discovery;
pub mod diagnostics;
pub mod effect_policy;
pub mod report;
pub mod schema_value;
pub mod stage_materials;
pub mod turn;
pub mod text_fields;

pub use catalog::{assert_wire_injective, encode_wire_name, CatalogToolMeta, CreatorCatalog, DiscoveryMeta};
pub use diagnostics::ContractViolation;
pub use report::{verify_root, VerificationReport};
pub use schema_value::{
    check_document_value, resolve_schema_document, validate_schema_value, verify_schema_document, verify_schema_keywords,
    SchemaDocument,
};
pub use stage_materials::{StageEntry, StageMaterials};
pub use turn::{CreatorTurnContract, ReviewRuleDefinition, TurnBudgets};

/// File name of the turn contract under `packages/app-contracts/v1/`.
pub const TURN_CONTRACT_FILE: &str = "creator-agent-turn-contract.json";
/// File name of the tool catalog under `packages/app-contracts/v1/`.
pub const CATALOG_FILE: &str = "creator-tool-catalog.json";
