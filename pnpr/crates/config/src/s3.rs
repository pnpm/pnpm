use super::{
    AmazonS3Builder, AmazonS3ConfigKey, Arc, Deserialize, ObjectStore, fmt, redact_url_credentials,
};

/// The YAML `s3:` block. Selects the object store for hosted packages and
/// enabled shared artifacts.
/// Credentials fall back to the standard AWS environment variables
/// (`AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY`) when not set here,
/// so an operator can keep secrets out of the config file. Whole-file
/// `${ENV}` substitution still runs first, so inline `${...}` values
/// work too.
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct S3Settings {
    /// Bucket the hosted packages live in.
    pub bucket: String,
    /// Region. AWS S3 needs a real region; Cloudflare R2 uses `auto`.
    #[serde(default)]
    pub region: Option<String>,
    /// Custom endpoint for S3-compatible providers. Omit for AWS S3;
    /// for R2 this is `https://<account-id>.r2.cloudflarestorage.com`.
    #[serde(default)]
    pub endpoint: Option<String>,
    /// Key prefix every object is stored under (e.g. `packages`).
    /// Lets one bucket hold more than just the hosted store.
    #[serde(default)]
    pub prefix: Option<String>,
    #[serde(default)]
    pub access_key_id: Option<String>,
    #[serde(default)]
    pub secret_access_key: Option<String>,
    /// Force path-style addressing (`endpoint/bucket/key`) instead of
    /// virtual-hosted (`bucket.endpoint/key`). `MinIO` typically needs
    /// this; AWS and R2 work with the default.
    #[serde(default)]
    pub force_path_style: Option<bool>,
    /// Allow plain-HTTP endpoints — needed for a local `MinIO` over
    /// `http://`. Defaults to HTTPS-only.
    #[serde(default)]
    pub allow_http: Option<bool>,
}

/// Hand-written so no credential renders. `Debug` on this type is reachable
/// from `Debug` on [`crate::Config`], so a diagnostic dump of the whole configuration
/// would otherwise carry the operator's S3 secret in plaintext. The key fields
/// are masked outright; `endpoint` is operator-supplied and can carry
/// `user:pass@` userinfo or a token query parameter, so it goes through the
/// same redaction the error type uses. Same rule as [`crate::RedactedHeaders`]: a
/// credential must never reach a log line, span, or diagnostic dump.
impl fmt::Debug for S3Settings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("S3Settings")
            .field("bucket", &self.bucket)
            .field("region", &self.region)
            .field("endpoint", &self.endpoint.as_deref().map(redact_url_credentials))
            .field("prefix", &self.prefix)
            .field("access_key_id", &self.access_key_id.as_ref().map(|_| "<redacted>"))
            .field("secret_access_key", &self.secret_access_key.as_ref().map(|_| "<redacted>"))
            .field("force_path_style", &self.force_path_style)
            .field("allow_http", &self.allow_http)
            .finish()
    }
}

impl S3Settings {
    /// The configured key prefix, normalized to either `""` or a value
    /// ending in `/` so it can be string-concatenated onto object keys.
    #[must_use]
    pub fn normalized_prefix(&self) -> String {
        normalize_key_prefix(self.prefix.as_deref())
    }
}

/// Build the object-store client from the YAML `s3:` settings. The
/// builder seeds from the AWS environment first so env credentials
/// work out of the box, then the explicit YAML values override.
/// Failures here are config errors surfaced at startup, not over HTTP.
pub fn build_s3_store(settings: &S3Settings) -> pnpr_error::Result<Arc<dyn ObjectStore>> {
    let store = s3_builder(settings).build().map_err(|err| {
        pnpr_error::RegistryError::InvalidConfig { reason: format!("invalid s3 config: {err}") }
    })?;
    Ok(Arc::new(store))
}

/// The configured builder, split out so tests can assert which keys the YAML
/// settled without opening a connection.
pub(super) fn s3_builder(settings: &S3Settings) -> AmazonS3Builder {
    let mut builder = AmazonS3Builder::from_env().with_bucket_name(&settings.bucket);
    if let Some(region) = &settings.region {
        builder = builder.with_region(region);
    }
    if let Some(endpoint) = &settings.endpoint {
        // Set the S3-specific key, not `with_endpoint`: `from_env` above imports
        // `AWS_ENDPOINT_URL_S3` into `s3_endpoint`, which object_store resolves
        // ahead of `endpoint` regardless of the order they were set. Writing the
        // plain endpoint would leave a stray environment variable silently
        // redirecting every request away from the configured bucket host.
        builder = builder.with_config(AmazonS3ConfigKey::S3Endpoint, endpoint);
    }
    if let Some(key) = &settings.access_key_id {
        builder = builder.with_access_key_id(key);
    }
    if let Some(secret) = &settings.secret_access_key {
        builder = builder.with_secret_access_key(secret);
    }
    if let Some(force_path_style) = settings.force_path_style {
        builder = builder.with_virtual_hosted_style_request(!force_path_style);
    }
    // Always decide this here rather than only when the YAML sets it: `from_env`
    // honours `AWS_ALLOW_HTTP`, so leaving it alone would let the environment
    // downgrade the connection to plaintext behind an operator who never asked
    // for it. The documented default is HTTPS-only, so make it so.
    builder.with_allow_http(settings.allow_http.unwrap_or(false))
}

/// A key prefix normalized to either `""` or a value ending in `/`, so the
/// store can concatenate it onto an object key. Every prefix reaching a store
/// goes through this — a raw `packages` would otherwise key `packagesfoo/…`.
#[must_use]
pub fn normalize_key_prefix(prefix: Option<&str>) -> String {
    match prefix.map(str::trim).filter(|text| !text.is_empty()) {
        None => String::new(),
        Some(prefix) => {
            let trimmed = prefix.trim_matches('/');
            if trimmed.is_empty() { String::new() } else { format!("{trimmed}/") }
        }
    }
}

/// The resolved hosted-store backend. Like [`crate::BackendConfig`], this carries
/// settings rather than a live client, so a parsed config stays plain data —
/// nothing here holds a constructed `ObjectStore` that a reader has to reason
/// about the lifetime of. `Storage::new` builds the client.
#[derive(Debug, Clone)]
pub enum HostedStoreConfig {
    /// Local directory — [`crate::Config::storage`].
    Fs,
    /// S3-compatible bucket, as declared by the YAML `s3:` block.
    S3(S3Settings),
    /// A caller-supplied object store. Parsing never produces this variant —
    /// it is how an embedder brings its own [`ObjectStore`]: a provider pnpr
    /// has no settings shape for, or an in-memory one under test. `prefix` is
    /// normalized on the way in, so a raw `packages` works.
    ObjectStore { store: Arc<dyn ObjectStore>, prefix: String },
}
