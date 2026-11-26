// Mirrored from packages/app-contracts/v1/network-resilience.json
// (host build.rs writes this into OUT_DIR; this crate has no build.rs hook).
pub const NETWORK_INTERRUPTION_WINDOW_MINUTES: u64 = 30;
pub const NETWORK_BACKOFF_BASE_MS: u64 = 2000;
pub const NETWORK_BACKOFF_CAP_MS: u64 = 30000;
