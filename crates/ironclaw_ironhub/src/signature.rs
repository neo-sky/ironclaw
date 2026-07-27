use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ed25519_dalek::{Signature, VerifyingKey};

use crate::model::SignedManifestEnvelope;

pub(super) fn verify_signed_manifest_with_keys(
    envelope_bytes: &[u8],
    verify_keys: &[(&str, &str)],
) -> Result<Vec<u8>, String> {
    let env: SignedManifestEnvelope = serde_json::from_slice(envelope_bytes)
        .map_err(|error| format!("envelope parse failed: {error}"))?;
    if env.v != 1 {
        return Err(format!("unsupported signed-manifest version {}", env.v));
    }
    let key_hex = verify_keys
        .iter()
        .find(|(id, _)| *id == env.key_id)
        .map(|(_, key)| *key)
        .ok_or_else(|| format!("unknown manifest signing key_id '{}'", env.key_id))?;
    let verifying_key = verifying_key_from_hex(key_hex)?;
    let manifest_bytes = URL_SAFE_NO_PAD
        .decode(env.manifest_b64.as_bytes())
        .map_err(|error| format!("manifest_b64 decode failed: {error}"))?;
    let sig_bytes = URL_SAFE_NO_PAD
        .decode(env.sig.as_bytes())
        .map_err(|error| format!("signature decode failed: {error}"))?;
    let signature = Signature::from_slice(&sig_bytes)
        .map_err(|error| format!("signature malformed: {error}"))?;
    verifying_key
        .verify_strict(&manifest_bytes, &signature)
        .map_err(|_| "manifest signature verification failed".to_string())?;
    Ok(manifest_bytes)
}

fn verifying_key_from_hex(hex: &str) -> Result<VerifyingKey, String> {
    let raw = hex::decode(hex).map_err(|error| format!("verify key is not valid hex: {error}"))?;
    let raw: [u8; 32] = raw
        .try_into()
        .map_err(|_| "verify key must be 32 bytes".to_string())?;
    VerifyingKey::from_bytes(&raw).map_err(|error| format!("invalid verify key: {error}"))
}
