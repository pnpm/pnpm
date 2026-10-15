use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use pnpr_error::RegistryError;
use serde_json::Value;
use std::path::Path;

/// Write a sigstore bundle to `dest`. Accepts either raw JSON string or
/// base64-encoded JSON, verifies it parses as a valid JSON object, and ensures
/// declared length matches when present.
pub fn write_sigstore_bundle(
    filename: &str,
    data: &str,
    declared_length: Option<u64>,
    dest: &Path,
) -> Result<u64, RegistryError> {
    let invalid = |reason: String| RegistryError::InvalidAttachment {
        filename: filename.to_string(),
        reason,
    };
    let json_bytes = if let Ok(val) = serde_json::from_str::<Value>(data) {
        if !val.is_object() {
            return Err(invalid("sigstore bundle must be a JSON object".to_string()));
        }
        data.as_bytes().to_vec()
    } else {
        let decoded = BASE64
            .decode(data.trim())
            .map_err(|err| invalid(format!("invalid base64: {err}")))?;
        let val: Value = serde_json::from_slice(&decoded)
            .map_err(|err| invalid(format!("invalid JSON in base64: {err}")))?;
        if !val.is_object() {
            return Err(invalid("sigstore bundle must be a JSON object".to_string()));
        }
        decoded
    };
    if let Some(expected_len) = declared_length
        && json_bytes.len() as u64 != expected_len
    {
        return Err(invalid(format!(
            "length mismatch: declared {expected_len}, got {}",
            json_bytes.len(),
        )));
    }
    if let Err(err) = std::fs::write(dest, &json_bytes) {
        let _ = std::fs::remove_file(dest);
        return Err(RegistryError::Io(err));
    }
    Ok(json_bytes.len() as u64)
}

/// Extract predicate type from sigstore bundle, defaulting to SLSA provenance v1.
#[must_use]
pub fn extract_predicate_type(data: &str) -> String {
    const DEFAULT_PREDICATE: &str = "https://slsa.dev/provenance/v1";
    let val: Value = if let Ok(val) = serde_json::from_str::<Value>(data) {
        val
    } else if let Ok(decoded) = BASE64.decode(data.trim()) {
        serde_json::from_slice(&decoded).unwrap_or(Value::Null)
    } else {
        Value::Null
    };

    if let Some(pred) = val
        .get("dsseEnvelope")
        .and_then(|e| e.get("payload"))
        .and_then(Value::as_str)
        .and_then(|payload_str| BASE64.decode(payload_str.trim()).ok())
        .and_then(|decoded| serde_json::from_slice::<Value>(&decoded).ok())
        .and_then(|statement| {
            statement
                .get("predicateType")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
    {
        return pred;
    }
    if let Some(pred) = val.get("predicateType").and_then(Value::as_str) {
        return pred.to_string();
    }
    DEFAULT_PREDICATE.to_string()
}

#[cfg(test)]
mod tests;
