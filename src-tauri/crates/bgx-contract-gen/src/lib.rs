//! 构建期合同生成公共工具：读取 `packages/app-contracts` 等 JSON 合同、做结构校验并输出 Rust 常量源码。
//! 供应用和内部 crate 的 build.rs 共用。

use std::fs;
use std::path::Path;

pub mod schema_types;
pub mod asr;
pub mod local_gpu_occupancy;
pub mod static_frame;
pub mod story_creation;
pub mod video_resolution;
pub mod comfy_prompt_origin;

pub fn read_contract_json(path: &Path) -> serde_json::Value {
    println!("cargo:rerun-if-changed={}", path.display());
    let text = fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("read app-contract {}: {e}", path.display()));
    let mut value: serde_json::Value = serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("parse app-contract {}: {e}", path.display()));
    // Canonicalize objects even when another build dependency enables
    // serde_json/preserve_order. Arrays retain their contract-defined order.
    value.sort_all_objects();
    value
}

pub fn require_schema_v1(v: &serde_json::Value, name: &str) {
    let ver = v
        .get("schemaVersion")
        .and_then(|x| x.as_u64())
        .unwrap_or_else(|| panic!("{name} missing schemaVersion"));
    if ver != 1 {
        panic!("{name} schemaVersion must be 1 (got {ver})");
    }
}

pub fn require_u64_in_range(v: &serde_json::Value, key: &str, meta_max: u64) -> u64 {
    let n = v
        .get(key)
        .and_then(|x| x.as_u64())
        .unwrap_or_else(|| panic!("app-contract missing integer field {key}"));
    if n < 1 || n > meta_max {
        panic!("{key} must be 1..={meta_max} (build meta cap, not product SSOT); got {n}");
    }
    n
}

pub fn require_ascii_template(v: &serde_json::Value, key: &str, placeholder: &str) -> String {
    let s = v
        .get(key)
        .and_then(|x| x.as_str())
        .unwrap_or_else(|| panic!("app-contract missing string field {key}"));
    if s.is_empty() {
        panic!("{key} must be non-empty");
    }
    if s.chars().any(|c| !c.is_ascii() || c.is_ascii_control()) {
        panic!("{key} must be printable ASCII");
    }
    if !s.contains(placeholder) {
        panic!("{key} must contain {placeholder}");
    }
    s.to_string()
}

pub fn require_ascii_token(v: &serde_json::Value, key: &str) -> String {
    let s = v
        .get(key)
        .and_then(|x| x.as_str())
        .unwrap_or_else(|| panic!("app-contract missing string field {key}"));
    if s.is_empty() {
        panic!("{key} must be non-empty");
    }
    if s.chars()
        .any(|c| !c.is_ascii() || c.is_ascii_control() || c.is_ascii_whitespace())
    {
        panic!("{key} must be non-empty printable ASCII without whitespace");
    }
    s.to_string()
}

pub fn require_bool(v: &serde_json::Value, key: &str) -> bool {
    v.get(key)
        .and_then(|x| x.as_bool())
        .unwrap_or_else(|| panic!("app-contract missing boolean field {key}"))
}

pub fn rust_string_lit(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

pub fn json_str_array_csv(raw: &serde_json::Value, path_hint: &str) -> String {
    raw.as_array()
        .unwrap_or_else(|| panic!("{path_hint} must be array"))
        .iter()
        .filter_map(|v| v.as_str())
        .collect::<Vec<_>>()
        .join(",")
}

pub fn require_u64_named(v: &serde_json::Value, key: &str, min: u64, max: u64) -> u64 {
    let n = v
        .get(key)
        .and_then(|x| x.as_u64())
        .unwrap_or_else(|| panic!("app-contract missing integer field {key}"));
    if n < min || n > max {
        panic!("{key} must be {min}..={max}; got {n}");
    }
    n
}

pub fn require_str_array(v: &serde_json::Value, key: &str) -> Vec<String> {
    let arr = v
        .get(key)
        .and_then(|x| x.as_array())
        .unwrap_or_else(|| panic!("app-contract missing array field {key}"));
    if arr.is_empty() {
        panic!("{key} must be non-empty");
    }
    arr.iter()
        .map(|item| {
            let s = item
                .as_str()
                .unwrap_or_else(|| panic!("{key} entries must be strings"));
            if s.is_empty()
                || s.chars()
                    .any(|c| !c.is_ascii() || c.is_ascii_control() || c.is_ascii_whitespace())
            {
                panic!("{key} entries must be printable ASCII without whitespace");
            }
            s.to_string()
        })
        .collect()
}

pub fn rust_str_slice(items: &[String]) -> String {
    let inner = items
        .iter()
        .map(|s| rust_string_lit(s))
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{inner}]")
}

/// Checked twin read for build scripts of pure-core crates (which may not touch std::fs themselves).
/// Missing file reads as empty so the caller's byte-equality check fails loudly. Line endings normalised.
pub fn read_twin_text(path: &Path) -> String {
    println!("cargo:rerun-if-changed={}", path.display());
    fs::read_to_string(path).unwrap_or_default().replace("\r\n", "\n")
}

pub fn write_generated(path: &Path, body: &str) {
    // Unchanged output is not rewritten: keeps mtimes stable so dependants do not rebuild.
    if fs::read_to_string(path)
        .map(|old| old == body)
        .unwrap_or(false)
    {
        return;
    }
    fs::write(path, body).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
}

pub fn require_u64_any(raw: &serde_json::Value, key: &str, file: &str) -> u64 {
    raw.get(key)
        .and_then(|v| v.as_u64())
        .unwrap_or_else(|| panic!("{file} missing {key}"))
}

pub fn require_ascii_marker(v: &serde_json::Value, key: &str) -> String {
    let s = v
        .get(key)
        .and_then(|x| x.as_str())
        .unwrap_or_else(|| panic!("prompt-fidelity-manifest.json missing string field {key}"));
    if s.trim().is_empty() || s.len() < 2 {
        panic!("{key} must be a non-trivial marker");
    }
    if !s.is_ascii() {
        panic!("{key} must be ASCII-only");
    }
    s.to_string()
}

/// Full host runtime contract, including network policy and endpoint kinds.
pub fn emit_comfy_endpoint(v1: &Path, out: &Path) {
    emit_comfy_endpoint_projection(v1, out, ComfyEndpointProjection::Runtime);
}

/// Installer preset validation consumes endpoint kinds, not runtime network policy.
/// The entire source contract still undergoes the same validation as the host.
pub fn emit_comfy_endpoint_kinds(v1: &Path, out: &Path) {
    emit_comfy_endpoint_projection(v1, out, ComfyEndpointProjection::Kinds);
}

enum ComfyEndpointProjection { Runtime, Kinds }

fn emit_comfy_endpoint_projection(v1: &Path, out: &Path, projection: ComfyEndpointProjection) {
    let raw = read_contract_json(&v1.join("comfy-endpoint.json"));
    require_schema_v1(&raw, "comfy-endpoint.json");
    let kinds = require_str_array(&raw, "endpointKinds");
    if kinds.len() != 2 || kinds[0] != "managedLocal" || kinds[1] != "lanExternal" {
        panic!("comfy-endpoint.json endpointKinds must be [managedLocal, lanExternal]");
    }
    let default_url = require_ascii_token(&raw, "defaultManagedLocalBaseUrl");
    if default_url != "http://127.0.0.1:8188" {
        panic!("comfy-endpoint.json defaultManagedLocalBaseUrl must be http://127.0.0.1:8188");
    }
    let h_local = require_u64_named(&raw, "healthTimeoutSecsManagedLocal", 1, 60);
    let h_lan = require_u64_named(&raw, "healthTimeoutSecsLanExternal", 1, 120);
    let c_local = require_u64_named(&raw, "healthConnectTimeoutMsManagedLocal", 100, 10_000);
    let c_lan = require_u64_named(&raw, "healthConnectTimeoutMsLanExternal", 500, 30_000);
    let meta = require_u64_named(&raw, "metadataTimeoutSecs", 30, 600);
    let snap = require_u64_named(&raw, "snapshotTimeoutSecs", 5, 120);
    let submit = require_u64_named(&raw, "submitTimeoutSecs", 30, 600);
    let media = require_u64_named(&raw, "mediaByteXferTimeoutSecs", 1800, 86_400);
    let net = require_u64_named(&raw, "networkRecoveryThresholdSecs", 5, 600);
    if matches!(projection, ComfyEndpointProjection::Kinds) {
        write_generated(
            &out.join("app_contracts_comfy_endpoint.rs"),
            &format!(
                "pub const KIND_MANAGED_LOCAL: &str = {};\npub const KIND_LAN_EXTERNAL: &str = {};\n",
                rust_string_lit(&kinds[0]), rust_string_lit(&kinds[1]),
            ),
        );
        return;
    }
    write_generated(
        &out.join("app_contracts_comfy_endpoint.rs"),
        &format!(
            "pub const KIND_MANAGED_LOCAL: &str = \"managedLocal\";\n\
             pub const KIND_LAN_EXTERNAL: &str = \"lanExternal\";\n\
             pub const DEFAULT_MANAGED_LOCAL_BASE_URL: &str = {default};\n\
             pub const HEALTH_TIMEOUT_SECS_MANAGED_LOCAL: u64 = {h_local};\n\
             pub const HEALTH_TIMEOUT_SECS_LAN_EXTERNAL: u64 = {h_lan};\n\
             pub const HEALTH_CONNECT_TIMEOUT_MS_MANAGED_LOCAL: u64 = {c_local};\n\
             pub const HEALTH_CONNECT_TIMEOUT_MS_LAN_EXTERNAL: u64 = {c_lan};\n\
             pub const METADATA_TIMEOUT_SECS: u64 = {meta};\n\
             pub const SNAPSHOT_TIMEOUT_SECS: u64 = {snap};\n\
             pub const SUBMIT_TIMEOUT_SECS: u64 = {submit};\n\
             pub const MEDIA_BYTE_XFER_TIMEOUT_SECS: u64 = {media};\n\
             pub const NETWORK_RECOVERY_THRESHOLD_SECS: u64 = {net};\n",
            default = rust_string_lit(&default_url),
        ),
    );
}
