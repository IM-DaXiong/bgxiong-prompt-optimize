mod dispatch;
pub mod protocol;
mod targets;

pub use crate::run::{
    run_image as optimize_frozen_image, run_literary as optimize_frozen_literary,
};
pub use crate::video_run::run_video_optimize as optimize_frozen_video;

pub use dispatch::{
    list_targets, optimize, optimize_frozen, target_method, Availability, AvailableTarget,
    FrozenOptimize, FrozenResult, OptimizeMaterials, OptimizeOutput, OptimizeReceipt,
};
pub use targets::{target_catalog, Target, TargetKind};
