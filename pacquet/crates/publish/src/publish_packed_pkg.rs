//! Port of the request half of `publishPackedPkg.ts` plus the
//! `libnpmpublish`-equivalent PUT: assemble the publish document, send it
//! (driving any OTP challenge through [`pacquet_network_web_auth`]), and turn
//! the registry's response into a [`PublishSummary`].

use std::collections::BTreeMap;

use pacquet_diagnostics::miette::{self, Diagnostic};
use pacquet_network::{AuthHeaders, ThrottledClient};
use pacquet_network_web_auth::{
    Host as WebAuthHost, OtpChallenge, OtpError, OtpErrorBody, WebAuthFetchOptions,
    WebAuthRetryOptions, WithOtpError, with_otp_handling,
};
use pacquet_reporter::Reporter;
use serde_json::{Map, Value};
use ssri::{Algorithm, IntegrityOpts};

use crate::{
    capabilities::{CiInfo, Clock, EnvVar, OidcFetch},
    failed_to_publish_error::FailedToPublishError,
    global_log::{global_info, global_warn},
    oidc::{OidcHttpOptions, escaped_package_name},
    publish_options::{
        Access, CreatePublishOptionsError, CreatePublishOptionsInput, create_publish_options,
    },
    publish_summary::{PackedPkgInfo, PublishSummary, create_publish_summary},
    registry_config_keys::NormalizedRegistryUrl,
};

/// The packed package handed to [`publish_packed_pkg`]: the manifest that was
/// packed, the tarball's bytes, and the file listing / unpacked size used for
/// the summary. Ports the `Pick<PackResult, ...>` first argument.
pub struct PackedPkg<'a> {
    pub published_manifest: &'a Value,
    pub tarball_data: &'a [u8],
    pub tarball_path: &'a str,
    pub contents: &'a [String],
    pub unpacked_size: u64,
}

/// The configuration `publishPackedPkg` reads. Ports the relevant subset of TS
/// `PublishPackedPkgOptions`; credential and TLS resolution is handled by
/// pacquet's shared [`AuthHeaders`] / [`ThrottledClient`] rather than per-field
/// options.
pub struct PublishPackedPkgOptions {
    pub default_registry: String,
    pub scoped_registries: BTreeMap<String, String>,
    pub access: Option<Access>,
    pub tag: String,
    pub otp: Option<String>,
    pub provenance: Option<bool>,
    pub dry_run: bool,
    pub stage: bool,
    pub http: OidcHttpOptions,
}

/// The shared network handles the publish request needs.
pub struct PublishNetwork<'a> {
    pub client: &'a ThrottledClient,
    pub auth_headers: &'a AuthHeaders,
}

/// Publish one packed package and return its [`PublishSummary`].
///
/// `Sys` carries the OIDC capabilities used while resolving credentials; the
/// OTP / web-authentication flow always runs against
/// [`pacquet_network_web_auth::Host`]. Ports TS `publishPackedPkg`.
pub async fn publish_packed_pkg<Sys, Reporter>(
    pkg: &PackedPkg<'_>,
    opts: &PublishPackedPkgOptions,
    network: &PublishNetwork<'_>,
) -> Result<PublishSummary, PublishPackedPkgError>
where
    Sys: EnvVar + CiInfo + Clock + OidcFetch,
    Reporter: self::Reporter,
{
    let input = CreatePublishOptionsInput {
        default_registry: &opts.default_registry,
        scoped_registries: &opts.scoped_registries,
        access: opts.access,
        tag: &opts.tag,
        otp: opts.otp.as_deref(),
        provenance: opts.provenance,
        http: &opts.http,
    };
    let resolved =
        create_publish_options::<Sys, Reporter>(pkg.published_manifest, &input, true).await?;

    let name = manifest_string(pkg.published_manifest, "name");
    let version = manifest_string(pkg.published_manifest, "version");
    let registry = resolved.registry.clone();
    let is_stage = opts.stage;

    global_info::<Reporter>(&format!("📦 {name}@{version} → {}", registry.as_str()));

    let mut summary = create_publish_summary(
        &PackedPkgInfo {
            published_manifest: pkg.published_manifest,
            tarball_path: pkg.tarball_path,
            contents: pkg.contents,
            unpacked_size: pkg.unpacked_size,
        },
        pkg.tarball_data,
    );

    if opts.dry_run {
        let verb = if is_stage { "staging" } else { "publishing" };
        global_warn::<Reporter>(&format!("Skip {verb} {name}@{version} (dry run)"));
        return Ok(summary);
    }

    // Generating a signed provenance attestation requires sigstore, which
    // pacquet does not yet bundle. Refuse rather than silently publishing
    // without the attestation pnpm would attach.
    if resolved.provenance == Some(true) {
        return Err(PublishPackedPkgError::ProvenanceUnsupported);
    }

    let document = build_publish_document(
        pkg.published_manifest,
        pkg.tarball_data,
        &registry,
        resolved.access,
        &resolved.default_tag,
        is_stage,
    )?;
    let document_bytes = serde_json::to_vec(&document).expect("serialize publish document");

    let put_url = join_registry(&registry, &escaped_package_name(&name))?;
    let authorization = resolved
        .auth_token_override
        .as_ref()
        .map(|token| format!("Bearer {token}"))
        .or_else(|| network.auth_headers.for_url_with_package(registry.as_str(), Some(&name)));
    let npm_command = if is_stage { "stage" } else { "publish" };

    let response = publish_with_otp_handling::<Reporter>(
        network.client,
        &put_url,
        authorization.as_deref(),
        npm_command,
        &document_bytes,
        resolved.otp.as_deref(),
        is_stage,
        web_auth_fetch_options(&opts.http),
    )
    .await?;

    if response.ok {
        if is_stage {
            summary.stage_id = response.stage_id;
        }
        let verb = if is_stage { "Staged" } else { "Published" };
        global_info::<Reporter>(&format!("✅ {verb} package {name}@{version}"));
        return Ok(summary);
    }

    Err(PublishPackedPkgError::FailedToPublish(FailedToPublishError::new(
        &name,
        &version,
        response.status,
        response.status_text,
        response.body,
    )))
}

/// One completed publish response. Mirrors TS `OtpPublishResponse`.
struct PublishResponse {
    ok: bool,
    status: u16,
    status_text: String,
    body: String,
    stage_id: Option<String>,
}

/// An HTTP-level publish failure handed to [`with_otp_handling`]. Only the
/// [`Otp`](Self::Otp) arm is a challenge it acts on; a transport failure
/// propagates.
#[derive(Debug, derive_more::Display, derive_more::Error, Diagnostic)]
pub enum PublishHttpError {
    #[display("the registry requested a one-time password")]
    Otp {
        #[error(not(source))]
        challenge: OtpChallenge,
    },

    #[display("the publish request failed: {reason}")]
    Transport {
        #[error(not(source))]
        reason: String,
    },
}

impl OtpError for PublishHttpError {
    fn as_otp_challenge(&self) -> Option<OtpChallenge> {
        match self {
            PublishHttpError::Otp { challenge } => Some(challenge.clone()),
            PublishHttpError::Transport { .. } => None,
        }
    }
}

/// Send the publish PUT, retrying once under OTP through the web-auth flow.
/// The operation returns `Ok` for every completed HTTP response (the caller
/// inspects `ok`) and `Err` only for an OTP challenge or a transport failure.
#[allow(
    clippy::too_many_arguments,
    reason = "a single registry request legitimately needs the URL, auth, command, body, OTP, stage flag and retry options"
)]
async fn publish_with_otp_handling<Reporter: self::Reporter>(
    client: &ThrottledClient,
    put_url: &str,
    authorization: Option<&str>,
    npm_command: &str,
    document_bytes: &[u8],
    otp: Option<&str>,
    is_stage: bool,
    fetch_options: WebAuthFetchOptions,
) -> Result<PublishResponse, WithOtpError<PublishHttpError>> {
    with_otp_handling::<WebAuthHost, Reporter, PublishResponse, PublishHttpError, _>(
        fetch_options,
        async move |challenge_otp: Option<&str>| {
            // The web-auth-provided OTP (a fresh challenge) takes precedence
            // over any statically configured one, mirroring `{ ...opts, otp }`.
            // Convert to an owned value before the first await so the borrowed
            // challenge argument is not held across it (which would make the
            // returned future's `Send` bound not general enough for the CLI).
            let effective_otp = challenge_otp.map(str::to_owned).or_else(|| otp.map(str::to_owned));
            put_publish(
                client,
                put_url,
                authorization,
                npm_command,
                document_bytes,
                effective_otp.as_deref(),
                is_stage,
            )
            .await
        },
    )
    .await
}

/// Perform a single publish PUT and classify the response.
#[allow(
    clippy::too_many_arguments,
    reason = "a single registry request legitimately needs the URL, auth, command, body, OTP and stage flag"
)]
async fn put_publish(
    client: &ThrottledClient,
    put_url: &str,
    authorization: Option<&str>,
    npm_command: &str,
    document_bytes: &[u8],
    otp: Option<&str>,
    is_stage: bool,
) -> Result<PublishResponse, PublishHttpError> {
    let guard = client.acquire_for_url(put_url).await;
    let mut request = guard
        .put(put_url)
        .header("content-type", "application/json")
        .header("npm-auth-type", "web")
        .header("npm-command", npm_command)
        .body(document_bytes.to_vec());
    if let Some(authorization) = authorization {
        request = request.header("authorization", authorization);
    }
    if let Some(otp) = otp {
        request = request.header("npm-otp", otp);
    }

    let response = request
        .send()
        .await
        .map_err(|error| PublishHttpError::Transport { reason: error.to_string() })?;
    let status = response.status();
    let status_text = status.canonical_reason().unwrap_or_default().to_owned();
    let www_authenticate = response
        .headers()
        .get(reqwest::header::WWW_AUTHENTICATE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let body = response.text().await.unwrap_or_default();

    // The registry signals an OTP / web-auth challenge with a 401 whose
    // `WWW-Authenticate` header advertises `otp` (and, for web auth, a body
    // carrying `authUrl` / `doneUrl`).
    if status.as_u16() == 401
        && www_authenticate.as_deref().is_some_and(|value| value.to_lowercase().contains("otp"))
    {
        return Err(PublishHttpError::Otp { challenge: parse_otp_challenge(&body) });
    }

    let stage_id = is_stage.then(|| stage_id_from_body(&body)).flatten();
    Ok(PublishResponse {
        ok: status.is_success(),
        status: status.as_u16(),
        status_text,
        body,
        stage_id,
    })
}

/// Read `authUrl` / `doneUrl` out of a challenge body for the web-auth flow.
fn parse_otp_challenge(body: &str) -> OtpChallenge {
    let parsed = serde_json::from_str::<Value>(body).ok();
    let read =
        |field: &str| parsed.as_ref().and_then(|json| json.get(field)?.as_str().map(str::to_owned));
    OtpChallenge {
        body: Some(OtpErrorBody { auth_url: read("authUrl"), done_url: read("doneUrl") }),
    }
}

fn stage_id_from_body(body: &str) -> Option<String> {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|json| json.get("stageId")?.as_str().map(str::to_owned))
}

fn web_auth_fetch_options(http: &OidcHttpOptions) -> WebAuthFetchOptions {
    WebAuthFetchOptions {
        timeout: http.fetch_timeout,
        retry: Some(WebAuthRetryOptions {
            factor: http.fetch_retry_factor,
            max_timeout: http.fetch_retry_maxtimeout,
            min_timeout: http.fetch_retry_mintimeout,
            randomize: None,
            retries: http.fetch_retries,
        }),
    }
}

/// Build the npm publish document — the JSON body `libnpmpublish` would send
/// as the whole `PUT /:pkg` request. Ports `buildMetadata` (and the matching
/// `createPublishDocument` used by batch publish).
fn build_publish_document(
    manifest: &Value,
    tarball_data: &[u8],
    registry: &NormalizedRegistryUrl,
    access: Option<Access>,
    tag: &str,
    _is_stage: bool,
) -> Result<Value, PublishPackedPkgError> {
    if manifest.get("private").and_then(Value::as_bool) == Some(true) {
        return Err(PublishPackedPkgError::Private);
    }
    let name = manifest_string(manifest, "name");
    let version = clean_version(&manifest_string(manifest, "version"))?;

    if !name.starts_with('@') && access == Some(Access::Restricted) {
        return Err(PublishPackedPkgError::UnscopedRestricted { name });
    }

    let integrity = IntegrityOpts::new().algorithm(Algorithm::Sha512).chain(tarball_data).result();
    let shasum = IntegrityOpts::new().algorithm(Algorithm::Sha1).chain(tarball_data).result();
    let tarball_name = format!("{name}-{version}.tgz");
    let tarball_uri = format!("{name}/-/{tarball_name}");
    let tarball_url = join_registry(registry, &tarball_uri)?.replacen("https://", "http://", 1);

    let mut dist = Map::new();
    dist.insert("integrity".to_owned(), Value::String(integrity.to_string()));
    dist.insert("shasum".to_owned(), Value::String(shasum.to_hex().1));
    dist.insert("tarball".to_owned(), Value::String(tarball_url));

    let mut version_manifest = manifest.as_object().cloned().unwrap_or_default();
    version_manifest.insert("_id".to_owned(), Value::String(format!("{name}@{version}")));
    version_manifest.insert("version".to_owned(), Value::String(version.clone()));
    version_manifest.insert("dist".to_owned(), Value::Object(dist));

    let mut versions = Map::new();
    versions.insert(version.clone(), Value::Object(version_manifest));

    let mut dist_tags = Map::new();
    dist_tags.insert(tag.to_owned(), Value::String(version));

    let attachment = serde_json::json!({
        "content_type": "application/octet-stream",
        "data": base64_standard(tarball_data),
        "length": tarball_data.len(),
    });
    let mut attachments = Map::new();
    attachments.insert(tarball_name, attachment);

    let mut root = Map::new();
    root.insert("_id".to_owned(), Value::String(name.clone()));
    root.insert("name".to_owned(), Value::String(name));
    if let Some(description) = manifest.get("description").filter(|value| value.is_string()) {
        root.insert("description".to_owned(), description.clone());
    }
    root.insert("dist-tags".to_owned(), Value::Object(dist_tags));
    root.insert("versions".to_owned(), Value::Object(versions));
    root.insert(
        "access".to_owned(),
        access.map_or(Value::Null, |access| Value::String(access.to_string())),
    );
    root.insert("_attachments".to_owned(), Value::Object(attachments));
    Ok(Value::Object(root))
}

/// Resolve `path` against the registry the way `new URL(path, registry)` does.
fn join_registry(
    registry: &NormalizedRegistryUrl,
    path: &str,
) -> Result<String, PublishPackedPkgError> {
    url::Url::parse(registry.as_str())
        .and_then(|base| base.join(path))
        .map(|url| url.to_string())
        .map_err(PublishPackedPkgError::InvalidUrl)
}

/// Clean a version string, mirroring `semver.clean`.
fn clean_version(version: &str) -> Result<String, PublishPackedPkgError> {
    let trimmed = version.trim().trim_start_matches(['=', 'v']);
    trimmed
        .parse::<node_semver::Version>()
        .map(|parsed| parsed.to_string())
        .map_err(|_| PublishPackedPkgError::BadSemver { version: version.to_owned() })
}

fn base64_standard(data: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(data)
}

fn manifest_string(manifest: &Value, key: &str) -> String {
    manifest.get(key).and_then(Value::as_str).unwrap_or_default().to_owned()
}

/// Failure surface of [`publish_packed_pkg`].
#[derive(Debug, derive_more::Display, derive_more::Error, Diagnostic)]
pub enum PublishPackedPkgError {
    #[display("{_0}")]
    #[diagnostic(transparent)]
    CreateOptions(CreatePublishOptionsError),

    #[display("This package has been marked as private")]
    #[diagnostic(
        code(ERR_PNPM_PRIVATE_PACKAGE),
        help("Remove the 'private' field from the package.json to publish it.")
    )]
    Private,

    #[display("Can't restrict access to the unscoped package {name}")]
    #[diagnostic(code(ERR_PNPM_UNSCOPED_RESTRICTED))]
    UnscopedRestricted { name: String },

    #[display("Invalid semver: {version}")]
    #[diagnostic(code(ERR_PNPM_BAD_SEMVER))]
    BadSemver { version: String },

    #[display("Provenance generation is not yet supported by pacquet")]
    #[diagnostic(
        code(ERR_PNPM_PROVENANCE_UNSUPPORTED),
        help(
            "Publish without provenance, or use the TypeScript pnpm CLI for provenance attestations."
        )
    )]
    ProvenanceUnsupported,

    #[display("invalid registry URL: {_0}")]
    InvalidUrl(url::ParseError),

    #[display("{_0}")]
    #[diagnostic(transparent)]
    Otp(WithOtpError<PublishHttpError>),

    #[display("{_0}")]
    #[diagnostic(transparent)]
    FailedToPublish(FailedToPublishError),
}

impl From<CreatePublishOptionsError> for PublishPackedPkgError {
    fn from(error: CreatePublishOptionsError) -> Self {
        PublishPackedPkgError::CreateOptions(error)
    }
}

impl From<WithOtpError<PublishHttpError>> for PublishPackedPkgError {
    fn from(error: WithOtpError<PublishHttpError>) -> Self {
        PublishPackedPkgError::Otp(error)
    }
}

#[cfg(test)]
mod tests;
