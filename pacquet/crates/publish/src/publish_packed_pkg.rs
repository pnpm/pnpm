//! The request half of `pnpm publish`: assemble the publish document, send the
//! registry `PUT`, drive any OTP challenge through
//! [`pacquet_network_web_auth`], and turn the registry's response into a
//! [`PublishSummary`].

use std::collections::BTreeMap;

use pacquet_diagnostics::miette::{self, Diagnostic};
use pacquet_network::{AuthHeaders, ThrottledClient};
use pacquet_network_web_auth::{
    Clock as WebAuthClock, EnterKeyListener, Host as WebAuthHost, OpenUrl, OtpChallenge, OtpError,
    OtpErrorBody, PromptOtp, Sleep, StdinIsTty, StdoutIsTty, WebAuthFetch, WebAuthFetchOptions,
    WebAuthRetryOptions, WithOtpError, with_otp_handling,
};
use pacquet_reporter::Reporter;
use serde_json::{Map, Value};

use crate::{
    capabilities::{Clock, EnvVar, OidcFetch},
    failed_to_publish_error::FailedToPublishError,
    global_log::{global_info, global_warn},
    oidc::{OidcHttpOptions, escaped_package_name},
    provenance_gen::{ProvenanceGenError, SignProvenance, generate_provenance},
    publish_options::{
        Access, CreatePublishOptionsError, CreatePublishOptionsInput, create_publish_options,
    },
    publish_summary::{PackedPkgInfo, PublishSummary, create_publish_summary},
    registry_config_keys::NormalizedRegistryUrl,
};

/// The packed package handed to [`publish_packed_pkg`]: the manifest that was
/// packed, the tarball's bytes, and the file listing / unpacked size used for
/// the summary.
pub struct PackedPkg<'a> {
    pub published_manifest: &'a Value,
    pub tarball_data: &'a [u8],
    pub tarball_path: &'a str,
    pub contents: &'a [String],
    pub unpacked_size: u64,
}

/// The configuration [`publish_packed_pkg`] reads. Credential and TLS
/// resolution is handled by pacquet's shared [`AuthHeaders`] /
/// [`ThrottledClient`] rather than per-field options.
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
/// [`pacquet_network_web_auth::Host`].
pub async fn publish_packed_pkg<Sys, Reporter>(
    pkg: &PackedPkg<'_>,
    opts: &PublishPackedPkgOptions,
    network: &PublishNetwork<'_>,
) -> Result<PublishSummary, PublishPackedPkgError>
where
    Sys: EnvVar + Clock + OidcFetch + SignProvenance,
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

    // `summary` already hashed the tarball; reuse those digests for the
    // document's `dist` rather than hashing the bytes a second time.
    let mut document = build_publish_document(
        pkg.published_manifest,
        pkg.tarball_data,
        &registry,
        resolved.access,
        &resolved.default_tag,
        &DistHashes { integrity: &summary.integrity, shasum: &summary.shasum },
    )?;

    // Provenance is requested either explicitly (`--provenance`) or by OIDC
    // auto-detection for a public repo; `resolved.provenance` carries the merged
    // result. Sign an SLSA attestation with sigstore and splice it into the
    // document's `_attachments`.
    if resolved.provenance == Some(true) {
        let attachment =
            generate_provenance::<Sys, Reporter>(&name, &version, pkg.tarball_data, &opts.http)
                .await
                .map_err(PublishPackedPkgError::Provenance)?;
        document["_attachments"][attachment.bundle_name.as_str()] = serde_json::json!({
            "content_type": attachment.content_type,
            "data": attachment.data,
            "length": attachment.data.len(),
        });
    }
    let body =
        bytes::Bytes::from(serde_json::to_vec(&document).expect("serialize publish document"));

    let put_url = join_registry(&registry, &escaped_package_name(&name))?;
    let authorization = resolved
        .auth_token_override
        .as_ref()
        .map(|token| format!("Bearer {token}"))
        .or_else(|| network.auth_headers.for_url_with_package(registry.as_str(), Some(&name)));
    let npm_command = if is_stage { "stage" } else { "publish" };

    let response = publish_with_otp_handling::<WebAuthHost, Reporter>(
        network.client,
        &put_url,
        authorization.as_deref(),
        npm_command,
        body,
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

/// One completed publish response.
#[derive(Debug)]
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
///
/// `Sys` is the web-auth [host](pacquet_network_web_auth::Host): production
/// passes the real one, tests pass a fake so the poll / clock / prompt are
/// scripted while the PUT still goes through a mocked registry.
#[expect(
    clippy::too_many_arguments,
    reason = "a single registry request legitimately needs the URL, auth, command, body, OTP, stage flag and retry options"
)]
async fn publish_with_otp_handling<Sys, Reporter>(
    client: &ThrottledClient,
    put_url: &str,
    authorization: Option<&str>,
    npm_command: &str,
    body: bytes::Bytes,
    otp: Option<&str>,
    is_stage: bool,
    fetch_options: WebAuthFetchOptions,
) -> Result<PublishResponse, WithOtpError<PublishHttpError>>
where
    Sys: WebAuthClock
        + Sleep
        + WebAuthFetch
        + StdinIsTty
        + StdoutIsTty
        + EnterKeyListener
        + OpenUrl
        + PromptOtp,
    Reporter: self::Reporter,
{
    with_otp_handling::<Sys, Reporter, PublishResponse, PublishHttpError, _, _>(
        fetch_options,
        // A plain `FnMut` returning an `async move` block (not an `AsyncFnMut`),
        // so the produced future is a concrete type with an ordinary `Send`
        // obligation — see `with_otp_handling`'s `Operation` bound.
        move |challenge_otp: Option<String>| {
            // The web-auth-provided OTP (a fresh challenge) takes precedence
            // over any statically configured one.
            let effective_otp = challenge_otp.or_else(|| otp.map(str::to_owned));
            // `Bytes::clone` is a cheap refcount bump, so the megabytes-large
            // body is not re-copied when the OTP retry re-invokes this closure.
            let body = body.clone();
            async move {
                put_publish(
                    client,
                    put_url,
                    authorization,
                    npm_command,
                    body,
                    effective_otp.as_deref(),
                    is_stage,
                )
                .await
            }
        },
    )
    .await
}

/// Perform a single publish PUT and classify the response.
async fn put_publish(
    client: &ThrottledClient,
    put_url: &str,
    authorization: Option<&str>,
    npm_command: &str,
    body: bytes::Bytes,
    otp: Option<&str>,
    is_stage: bool,
) -> Result<PublishResponse, PublishHttpError> {
    let guard = client.acquire_for_url(put_url).await;
    let mut request = guard
        .put(put_url)
        .header("content-type", "application/json")
        .header("npm-auth-type", "web")
        .header("npm-command", npm_command)
        .body(body);
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

    // The registry signals an OTP / web-auth challenge with a 401 that either
    // advertises the `otp` token in `WWW-Authenticate` or carries a
    // `one-time pass` body (npm-registry-fetch's two detection paths). For web
    // auth the body also carries `authUrl` / `doneUrl`.
    if status.as_u16() == 401 && is_otp_challenge(www_authenticate.as_deref(), &body) {
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

/// Whether a 401 response is an OTP / two-factor challenge: the
/// `WWW-Authenticate` header lists `otp` as a comma-separated token, or the
/// body mentions `one-time pass`.
fn is_otp_challenge(www_authenticate: Option<&str>, body: &str) -> bool {
    let header_lists_otp = www_authenticate.is_some_and(|value| {
        value.split(',').any(|token| token.trim().eq_ignore_ascii_case("otp"))
    });
    header_lists_otp || body.to_lowercase().contains("one-time pass")
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

/// The tarball digests written into the document's `dist`, already computed by
/// [`create_publish_summary`] so the tarball is not hashed twice.
struct DistHashes<'a> {
    /// SRI SHA-512 (`sha512-...`).
    integrity: &'a str,
    /// Lowercase hex SHA-1.
    shasum: &'a str,
}

/// Build the npm publish document — the JSON body sent as the whole
/// `PUT /:pkg` request.
fn build_publish_document(
    manifest: &Value,
    tarball_data: &[u8],
    registry: &NormalizedRegistryUrl,
    access: Option<Access>,
    tag: &str,
    dist_hashes: &DistHashes<'_>,
) -> Result<Value, PublishPackedPkgError> {
    if manifest.get("private").and_then(Value::as_bool) == Some(true) {
        return Err(PublishPackedPkgError::Private);
    }
    let name = manifest_string(manifest, "name");
    let version = clean_version(&manifest_string(manifest, "version"))?;

    if !name.starts_with('@') && access == Some(Access::Restricted) {
        return Err(PublishPackedPkgError::UnscopedRestricted { name });
    }

    let tarball_name = format!("{name}-{version}.tgz");
    let tarball_uri = format!("{name}/-/{tarball_name}");
    let tarball_url = join_registry(registry, &tarball_uri)?.replacen("https://", "http://", 1);

    let mut dist = Map::new();
    dist.insert("integrity".to_owned(), Value::String(dist_hashes.integrity.to_owned()));
    dist.insert("shasum".to_owned(), Value::String(dist_hashes.shasum.to_owned()));
    dist.insert("tarball".to_owned(), Value::String(tarball_url));

    let mut version_manifest = manifest.as_object().cloned().unwrap_or_default();
    version_manifest.insert("_id".to_owned(), Value::String(format!("{name}@{version}")));
    version_manifest.insert("version".to_owned(), Value::String(version.clone()));
    version_manifest.insert("dist".to_owned(), Value::Object(dist));

    let mut versions = Map::new();
    versions.insert(version.clone(), Value::Object(version_manifest));

    // A manifest-level `tag` wins over the default.
    let tag = manifest.get("tag").and_then(Value::as_str).unwrap_or(tag);
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

/// Clean a version string to `major.minor.patch` plus any prerelease,
/// dropping build metadata.
fn clean_version(version: &str) -> Result<String, PublishPackedPkgError> {
    let trimmed = version.trim().trim_start_matches(['=', 'v']);
    let mut parsed = trimmed
        .parse::<node_semver::Version>()
        .map_err(|_| PublishPackedPkgError::BadSemver { version: version.to_owned() })?;
    // The published version is `major.minor.patch` plus any prerelease but
    // never build metadata. node_semver's `Display` appends `+build`, so drop
    // it to keep the published version identical to what pnpm registers (e.g.
    // `1.2.3+build` -> `1.2.3`).
    parsed.build.clear();
    Ok(parsed.to_string())
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

    #[display("{_0}")]
    #[diagnostic(transparent)]
    Provenance(ProvenanceGenError),

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
