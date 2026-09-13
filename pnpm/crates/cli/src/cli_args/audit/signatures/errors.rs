#[derive(Debug, derive_more::Display, derive_more::Error, miette::Diagnostic)]
#[non_exhaustive]
pub(in super::super) enum SignaturesError {
    // `reason` is the registry error already passed through
    // `redact_url_credentials`; the raw `reqwest::Error` is not carried as a
    // diagnostic source, so embedded `user:pass@` credentials cannot leak via
    // its `Display` or the miette cause chain.
    #[display("Failed to request the registry keys endpoint (at {url}): {reason}")]
    #[diagnostic(code(ERR_PNPM_AUDIT_SIGNATURE_KEYS_FETCH_FAIL))]
    KeysNetwork { url: String, reason: String },

    #[display("The registry keys endpoint (at {url}) responded with {status}: {body}")]
    #[diagnostic(code(ERR_PNPM_AUDIT_SIGNATURE_KEYS_FETCH_FAIL))]
    KeysBadStatus {
        url: String,
        status: u16,
        body: String,
    },

    #[display(
        "The registry keys endpoint (at {url}) returned invalid JSON: {reason}. Response body: {body}"
    )]
    #[diagnostic(code(ERR_PNPM_AUDIT_SIGNATURE_KEYS_FETCH_FAIL))]
    KeysInvalidJson {
        url: String,
        reason: String,
        body: String,
    },

    #[display(
        "The registry keys endpoint (at {url}) returned an unexpected body. Expected an object with a keys array; got: {body}"
    )]
    #[diagnostic(code(ERR_PNPM_AUDIT_SIGNATURE_KEYS_FETCH_FAIL))]
    KeysUnexpectedBody { url: String, body: String },

    /// See [`SignaturesError::KeysNetwork`] for why the error is stored as a
    /// pre-redacted string rather than a `reqwest::Error` source.
    #[display("Failed to request the packument endpoint (at {url}): {reason}")]
    #[diagnostic(code(ERR_PNPM_AUDIT_SIGNATURE_PACKUMENT_FETCH_FAIL))]
    PackumentNetwork { url: String, reason: String },

    #[display("The packument endpoint (at {url}) responded with {status}: {body}")]
    #[diagnostic(code(ERR_PNPM_AUDIT_SIGNATURE_PACKUMENT_FETCH_FAIL))]
    PackumentBadStatus {
        url: String,
        status: u16,
        body: String,
    },

    #[display(
        "The packument endpoint (at {url}) returned invalid JSON: {reason}. Response body: {body}"
    )]
    #[diagnostic(code(ERR_PNPM_AUDIT_SIGNATURE_PACKUMENT_FETCH_FAIL))]
    PackumentInvalidJson {
        url: String,
        reason: String,
        body: String,
    },

    #[display(
        "The packument endpoint (at {url}) returned an unexpected body. Expected an object with versions; got: {body}"
    )]
    #[diagnostic(code(ERR_PNPM_AUDIT_SIGNATURE_PACKUMENT_FETCH_FAIL))]
    PackumentUnexpectedBody { url: String, body: String },
}
