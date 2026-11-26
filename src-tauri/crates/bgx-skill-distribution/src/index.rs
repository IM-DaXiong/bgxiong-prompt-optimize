use super::{error, sha256, DistributionError, INDEX_MAX_BYTES};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Deserialize;


#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Envelope {
    key_id: String,
    payload_b64: String,
    signature_b64: String,
}

pub struct IndexTrust<'a> {
    pub key_id: &'a str,
    pub pubkey_b64: &'a str,
}

pub fn verify_envelope(bytes: &[u8], trust: &IndexTrust<'_>) -> Result<(Vec<u8>, String), DistributionError> {
    let invalid = |detail| error("ONLINE_INDEX_INVALID", detail);
    if bytes.len() as u64 > INDEX_MAX_BYTES {
        return Err(invalid("Index byte limit exceeded".to_string()));
    }
    let envelope: Envelope = serde_json::from_slice(bytes).map_err(|e| invalid(e.to_string()))?;
    if envelope.key_id != trust.key_id {
        return Err(invalid("Unexpected keyId".into()));
    }
    let payload = STANDARD.decode(&envelope.payload_b64).map_err(|e| invalid(e.to_string()))?;
    verify_signature(&payload, trust.pubkey_b64, &envelope.signature_b64)
        .map_err(|detail| error("ONLINE_INDEX_SIGNATURE_INVALID", detail))?;
    let digest = sha256(&payload);
    Ok((payload, digest))
}

fn verify_signature(payload: &[u8], public: &str, signature: &str) -> Result<(), String> {
    let decode = |text| {
        let bytes = STANDARD.decode(text).map_err(|e| e.to_string())?;
        String::from_utf8(bytes).map_err(|e| e.to_string())
    };
    let key = minisign_verify::PublicKey::decode(&decode(public)?).map_err(|e| e.to_string())?;
    let signature = minisign_verify::Signature::decode(&decode(signature)?).map_err(|e| e.to_string())?;
    key.verify(payload, &signature, true).map_err(|e| e.to_string())
}

pub fn accept_sequence(
    last_sequence: u64,
    last_digest: Option<&str>,
    sequence: u64,
    digest: &str,
) -> Result<(), DistributionError> {
    if sequence < last_sequence
        || (sequence == last_sequence && last_digest.is_some_and(|value| value != digest))
    {
        return Err(error("ONLINE_INDEX_ROLLBACK", "Index sequence rollback or reuse"));
    }
    Ok(())
}
