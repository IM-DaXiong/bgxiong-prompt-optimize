//! Internal prompt packages.
//!
//! The crate checks declarations, freezes the bytes a task needs, restores that
//! snapshot without a directory, and renders request messages or one material slot.
//! It does not call a model, open a database, or choose a resource root.

mod error;
mod limits;
mod package;
mod publish;
mod snapshot;
mod source;
mod types;

pub use error::PromptError;
pub use limits::{MAX_FILE_BYTES, MAX_FILES, MAX_TOTAL_BYTES};
pub use publish::{stage_root, validate_root, StageReport};
pub use snapshot::{freeze_set, render_material, render_method_body, render_request, restore, restore_value};
pub use types::{FrozenRuleSet, Language, RenderedMessage, TaskKind, FORMAT_VERSION};
