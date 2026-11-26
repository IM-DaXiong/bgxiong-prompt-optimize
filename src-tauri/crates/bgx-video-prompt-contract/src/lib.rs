//! Neutral video-prompt contracts (PLAN 20261013 R14a/b/c + R17).
//! R17: project-format prompt compiler + mother input projection (structural kernel).

pub mod author_normalize;
pub mod channels;
pub mod closed_set;
pub mod compile;
pub mod descriptor;
pub mod document;
pub mod identity;
pub mod inventory;
pub mod mother_view;
pub mod projection;
pub mod validate;

pub use channels::{
    allowed_source_ids, allows, expanded_document, negative_generation_incomplete, resolve_channels,
    FrozenNegativeContext,
};
pub use closed_set::{ProjectionClosedSet, ShotIds};
pub use compile::render_project_format;
pub use author_normalize::{
    ledger_source_provenance, normalize_author_reply, normalize_model_draft, parse_model_draft,
    partition_author_outcome, AUTHOR_BATCH_OWNERSHIP_UNREADABLE, DraftIssueV1, ModelDraftV1,
    ShotAuthorPartition,
};
pub use document::{
    contract, contract_identity, kind_slot_compatible, validate_draft, DocumentDraftV1,
    PromptFactV1, QuoteIntentV1, SemanticContract, ShotDraftV1,
};
pub use identity::{
    compile_identity_lock_lines, lock_template_for_language, neutral_lock_template_for_language,
    parse_lock_template, parse_neutral_lock_template, render_lock_template, FrozenInventoryItem,
    IdentityLockLine, IdentityReferenceBindingV2, LockCompileView, LockTemplatePart,
    ReferenceTokenResolver, ReferenceUseV2, ShotSubjectV1, IDENTITY_LOCK_TEMPLATE_EN,
    IDENTITY_LOCK_TEMPLATE_NEUTRAL_EN, IDENTITY_LOCK_TEMPLATE_NEUTRAL_ZH, IDENTITY_LOCK_TEMPLATE_ZH,
    MOTHER_PROMPT_VERSION, PROJECT_FORMAT_VERSION, PROJECTOR_VERSION, REVIEW_PROMPT_VERSION,
    SEMANTIC_CONTRACT_VERSION,
};
pub use inventory::{DeclaredIdentityV1, ReferenceMediaInventoryItem};
pub use mother_view::project_mother_input;
pub use validate::{
    check_count, check_member, check_members, check_min_u64, check_sha256_hex, check_text, invalid,
    limits, validate_projection_draft, ContractResult, Limits, ProjectionContractError,
};
