//! SKILL WASM worker stdio protocol v1 (W00, `packages/app-contracts/v1/agent-skill-worker.json`).
//!
//! Pure message types and frame encoding shared by the worker (`bgx-skill-runtime`) and the GUI
//! transport (`bgx-skill-transport`). Limits, the import allowlist and error codes are generated
//! from the contracts by `build.rs`; nothing here does IO. Independent of the Adapter worker
//! protocol (no shared messages, broker or permission policy).

use base64::Engine as _;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

include!(concat!(env!("OUT_DIR"), "/agent_skill_worker_contract.rs"));

/// The worker contract text; its digest is exchanged in `helloAck` so both sides prove they were
/// built from the same contract (version check beyond `protocolVersion`).
pub const CONTRACT_JSON: &str =
    include_str!("../../../../packages/app-contracts/v1/agent-skill-worker.json");

/// Bytes of the `u32` little-endian length header.
pub const FRAME_HEADER_BYTES: usize = 4;

/// `sha256:<lowercase hex>` of `bytes` (contract `hashFormat`).
pub fn sha256_ref(bytes: &[u8]) -> String {
    let d = Sha256::digest(bytes);
    let mut s = String::with_capacity(7 + 64);
    s.push_str("sha256:");
    for b in d {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// Digest of [`CONTRACT_JSON`] with line endings normalised to `\n` (checkouts may differ).
pub fn contract_sha256() -> String {
    sha256_ref(CONTRACT_JSON.replace("\r\n", "\n").as_bytes())
}

/// Raw bytes carried in a message (`blob`): standard base64 plus the declared sha256.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Blob {
    pub bytes_base64: String,
    pub sha256: String,
}

impl Blob {
    pub fn from_bytes(bytes: &[u8]) -> Self {
        Blob {
            bytes_base64: base64::engine::general_purpose::STANDARD.encode(bytes),
            sha256: sha256_ref(bytes),
        }
    }

    /// Decode with the size gate applied before the hash check (contract `blob.rule`).
    /// `too_large` is the code reported when the decoded length exceeds `max_bytes`.
    pub fn decode_verified(
        &self,
        max_bytes: usize,
        too_large: WorkerErrorCode,
    ) -> Result<Vec<u8>, WorkerFailure> {
        // base64 expands 3 bytes to 4 chars; reject before allocating the decoded buffer.
        if self.bytes_base64.len() / 4 * 3 > max_bytes + 3 {
            return Err(WorkerFailure::new(
                too_large,
                format!("> {max_bytes} bytes"),
            ));
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(self.bytes_base64.as_bytes())
            .map_err(|e| {
                WorkerFailure::new(WorkerErrorCode::HashMismatch, format!("base64: {e}"))
            })?;
        if bytes.len() > max_bytes {
            return Err(WorkerFailure::new(
                too_large,
                format!("{} > {max_bytes} bytes", bytes.len()),
            ));
        }
        let actual = sha256_ref(&bytes);
        if actual != self.sha256 {
            return Err(WorkerFailure::new(
                WorkerErrorCode::HashMismatch,
                format!("declared {} actual {actual}", self.sha256),
            ));
        }
        Ok(bytes)
    }
}

/// Limits the worker enforces (echoed in `helloAck`; always the contract values).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkerLimits {
    pub memory_bytes: u64,
    pub fuel: u64,
    pub wall_ms: u64,
}

impl WorkerLimits {
    pub const CONTRACT: WorkerLimits = WorkerLimits {
        memory_bytes: WASM_MEMORY_BYTES as u64,
        fuel: WASM_FUEL,
        wall_ms: WASM_WALL_MS,
    };
}

/// Parent (GUI host) to worker.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum ParentMessage {
    #[serde(rename_all = "camelCase")]
    Hello {
        protocol_version: u32,
    },
    #[serde(rename_all = "camelCase")]
    ValidateComponent {
        request_id: String,
        component: Blob,
    },
    #[serde(rename_all = "camelCase")]
    ProducePlan {
        request_id: String,
        component: Blob,
        input: Blob,
    },
    #[serde(rename_all = "camelCase")]
    Cancel {
        request_id: String,
    },
    Shutdown {},
}

/// Worker to parent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum WorkerMessage {
    #[serde(rename_all = "camelCase")]
    HelloAck {
        protocol_version: u32,
        contract_sha256: String,
        worker_version: String,
        limits: WorkerLimits,
        abi_world: String,
    },
    #[serde(rename_all = "camelCase")]
    Validated {
        request_id: String,
        component_sha256: String,
        imports: Vec<String>,
    },
    #[serde(rename_all = "camelCase")]
    PlanProduced {
        request_id: String,
        component_sha256: String,
        input_sha256: String,
        output: Blob,
        fuel_consumed: u64,
    },
    #[serde(rename_all = "camelCase")]
    Failed {
        /// `None` only for failures not tied to a request (malformed frame, handshake).
        request_id: Option<String>,
        code: String,
        message: String,
    },
    #[serde(rename_all = "camelCase")]
    CancelAck {
        request_id: String,
        active: bool,
    },
    ShutdownAck {},
}

impl WorkerMessage {
    /// `terminalMessages`: exactly one per requestId.
    pub fn terminal_request_id(&self) -> Option<&str> {
        match self {
            WorkerMessage::Validated { request_id, .. }
            | WorkerMessage::PlanProduced { request_id, .. } => Some(request_id),
            WorkerMessage::Failed {
                request_id: Some(id),
                ..
            } => Some(id),
            _ => None,
        }
    }

    pub fn failed(request_id: Option<&str>, f: &WorkerFailure) -> Self {
        WorkerMessage::Failed {
            request_id: request_id.map(str::to_string),
            code: f.code.as_str().to_string(),
            message: f.message.clone(),
        }
    }
}

/// A typed failure: contract code plus a bounded human message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkerFailure {
    pub code: WorkerErrorCode,
    pub message: String,
}

impl WorkerFailure {
    pub fn new(code: WorkerErrorCode, message: impl Into<String>) -> Self {
        WorkerFailure {
            code,
            message: truncate_chars(&message.into(), STDERR_TAIL_MAX_CHARS),
        }
    }
}

impl std::fmt::Display for WorkerFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for WorkerFailure {}

/// Keep at most `max` chars (the tail is dropped; a marker notes the cut).
pub fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Keep the last `max` chars (diagnostic tails such as stderr).
pub fn tail_chars(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max {
        return s.to_string();
    }
    s.chars().skip(n - max).collect()
}

/// Frame-level errors (header/body), independent of IO.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FrameError {
    Oversize { len: usize, max: usize },
    Malformed(String),
}

impl FrameError {
    pub fn code(&self) -> WorkerErrorCode {
        match self {
            FrameError::Oversize { .. } => WorkerErrorCode::FrameOversize,
            FrameError::Malformed(_) => WorkerErrorCode::FrameMalformed,
        }
    }
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FrameError::Oversize { len, max } => write!(f, "frame {len} > {max} bytes"),
            FrameError::Malformed(m) => write!(f, "malformed frame: {m}"),
        }
    }
}

/// Encode `msg` as `u32 LE length + JSON body`.
pub fn encode_frame<T: Serialize>(msg: &T) -> Result<Vec<u8>, FrameError> {
    let body = serde_json::to_vec(msg).map_err(|e| FrameError::Malformed(e.to_string()))?;
    if body.len() > FRAME_MAX_BYTES {
        return Err(FrameError::Oversize {
            len: body.len(),
            max: FRAME_MAX_BYTES,
        });
    }
    let mut out = Vec::with_capacity(FRAME_HEADER_BYTES + body.len());
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&body);
    Ok(out)
}

/// Body length announced by a header; oversize is rejected before any allocation.
pub fn frame_body_len(header: [u8; FRAME_HEADER_BYTES]) -> Result<usize, FrameError> {
    let len = u32::from_le_bytes(header) as usize;
    if len > FRAME_MAX_BYTES {
        return Err(FrameError::Oversize {
            len,
            max: FRAME_MAX_BYTES,
        });
    }
    Ok(len)
}

pub fn decode_frame_body<T: DeserializeOwned>(body: &[u8]) -> Result<T, FrameError> {
    serde_json::from_slice(body).map_err(|e| FrameError::Malformed(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blob_roundtrip_and_gates() {
        let b = Blob::from_bytes(b"{}");
        assert_eq!(
            b.decode_verified(10, WorkerErrorCode::InputTooLarge)
                .unwrap(),
            b"{}"
        );
        let err = b
            .decode_verified(1, WorkerErrorCode::InputTooLarge)
            .unwrap_err();
        assert_eq!(err.code, WorkerErrorCode::InputTooLarge);
        let mut bad = b.clone();
        bad.sha256 = sha256_ref(b"x");
        assert_eq!(
            bad.decode_verified(10, WorkerErrorCode::InputTooLarge)
                .unwrap_err()
                .code,
            WorkerErrorCode::HashMismatch
        );
        let junk = Blob {
            bytes_base64: "!!!".into(),
            sha256: b.sha256.clone(),
        };
        assert_eq!(
            junk.decode_verified(10, WorkerErrorCode::InputTooLarge)
                .unwrap_err()
                .code,
            WorkerErrorCode::HashMismatch
        );
    }

    #[test]
    fn frame_header_oversize_is_rejected_before_allocation() {
        let h = ((FRAME_MAX_BYTES + 1) as u32).to_le_bytes();
        assert_eq!(
            frame_body_len(h).unwrap_err().code(),
            WorkerErrorCode::FrameOversize
        );
    }

    #[test]
    fn truncation_helpers_count_chars() {
        assert_eq!(truncate_chars("比格熊abc", 4), "比格熊…");
        assert_eq!(tail_chars("比格熊abc", 2), "bc");
        assert_eq!(tail_chars("ab", 5), "ab");
    }
}
