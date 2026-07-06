//! Bridging pacquet's [`miette::Diagnostic`] errors to JavaScript errors.
//!
//! pnpm's TypeScript consumers depend on a stable error contract: a thrown
//! error carries a `code` (the `ERR_PNPM_*` string) and optionally a `hint`.
//! Bit's `pnpm-error-to-bit-error.ts` reads exactly those properties. napi's
//! [`napi::Error`] only carries a status and a reason string, so we encode the
//! structured fields as a JSON envelope in the reason; a small JS shim in the
//! wrapper package's `index.js` unpacks it back into real properties on the
//! thrown JS `Error`.
//!
//! The envelope is `PNPM_ERR_JSON:{"code":...,"message":...,"hint":...}`. The
//! shim recognizes the prefix, sets `err.code` / `err.hint`, and restores
//! `err.message` to the human string. An error that never round-trips through
//! the shim (e.g. a panic caught by napi) still surfaces as a normal `Error`
//! with a readable message.

use miette::Diagnostic;
use serde::Serialize;

/// JSON envelope carried in a [`napi::Error`] reason so the JS shim can lift
/// the structured fields onto the thrown `Error`.
#[derive(Serialize)]
struct ErrorEnvelope {
    code: Option<String>,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    hint: Option<String>,
}

/// Prefix marking a reason string as a JSON envelope. Kept in sync with the
/// JS shim in the wrapper package's `index.js`.
const ENVELOPE_PREFIX: &str = "PNPM_ERR_JSON:";

/// Convert any pacquet error implementing [`miette::Diagnostic`] into a
/// [`napi::Error`] whose reason carries the `code` / `message` / `hint`
/// envelope.
pub fn to_napi_error<Diag: Diagnostic>(error: &Diag) -> napi::Error {
    let code = error.code().map(|code| code.to_string());
    let hint = error.help().map(|help| help.to_string());
    let message = error.to_string();
    let envelope = ErrorEnvelope { code, message, hint };
    let reason = match serde_json::to_string(&envelope) {
        Ok(json) => format!("{ENVELOPE_PREFIX}{json}"),
        // If the envelope itself fails to serialize, fall back to the plain
        // message — the JS side then sees a normal Error with no `code`.
        Err(_) => envelope.message,
    };
    napi::Error::from_reason(reason)
}

/// Build a structured [`napi::Error`] for an engine operation that the Rust
/// binding does not implement yet, carrying the `code`
/// `ERR_PNPM_NAPI_UNIMPLEMENTED` so a consumer's `PnpmError` translation
/// surfaces it like any other pnpm error rather than as an opaque crash.
pub fn unimplemented_error(operation: &str) -> napi::Error {
    let envelope = ErrorEnvelope {
        code: Some("ERR_PNPM_NAPI_UNIMPLEMENTED".to_string()),
        message: format!(
            "`{operation}` is not yet implemented in the pnpm Rust engine binding. \
             See pacquet/plans/NAPI.md.",
        ),
        hint: None,
    };
    match serde_json::to_string(&envelope) {
        Ok(json) => napi::Error::from_reason(format!("{ENVELOPE_PREFIX}{json}")),
        Err(_) => napi::Error::from_reason(envelope.message),
    }
}

/// Build a structured [`napi::Error`] for an option that the binding does not
/// implement yet. Unsupported options fail closed instead of being silently
/// ignored, because many install options affect script execution, auth,
/// registry policy, or lockfile shape.
pub fn unsupported_option_error(operation: &str, option: &str) -> napi::Error {
    let envelope = ErrorEnvelope {
        code: Some("ERR_PNPM_NAPI_UNSUPPORTED_OPTION".to_string()),
        message: format!(
            "`{option}` is not supported by `{operation}` in the pnpm Rust engine binding yet.",
        ),
        hint: Some("Remove the option or keep using the TypeScript engine for this call.".into()),
    };
    match serde_json::to_string(&envelope) {
        Ok(json) => napi::Error::from_reason(format!("{ENVELOPE_PREFIX}{json}")),
        Err(_) => napi::Error::from_reason(envelope.message),
    }
}
