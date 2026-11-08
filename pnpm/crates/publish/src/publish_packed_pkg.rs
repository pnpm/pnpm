//! The request half of `pnpm publish`: assemble the publish document, send the
//! registry `PUT`, drive any OTP challenge through
//! [`pnpm_network_web_auth`], and turn the registry's response into a
//! [`PublishSummary`].

pub(crate) use document::{DistHashes, build_publish_document, clean_version};
pub use request::PublishHttpError;
pub(crate) use request::{PublishResponse, publish_with_otp_handling, web_auth_fetch_options};
pub use uploads::{UploadedArchive, upload_variant_archives};
pub use variants::{PackedVariant, StagedReference, VariantArchive};
pub(crate) use variants::{
    StagedLengths, check_registry as check_variants_registry, reference_staged_archives,
    staged_archives as variant_staged_archives, staged_lengths,
};

use std::collections::{BTreeMap, HashSet};

use pnpm_diagnostics::miette::{self, Diagnostic};
use pnpm_network::{AuthHeaders, ThrottledClient, escaped_package_name};
use pnpm_network_web_auth::{
    Clock as WebAuthClock, EnterKeyListener, Host as WebAuthHost, OpenUrl, OtpChallenge, OtpError,
    OtpErrorBody, PromptOtp, Sleep, StdinIsTty, StdoutIsTty, WebAuthFetch, WebAuthFetchOptions,
    WebAuthRetryOptions, WithOtpError, with_otp_handling,
};
use pnpm_package_name::is_valid_old_npm_package_name;
use pnpm_reporter::Reporter;
use serde_json::{Map, Value};

use crate::{
    PublishFailure,
    capabilities::{Clock, EnvVar, OidcFetch},
    failed_to_publish_error::FailedToPublishError,
    global_log::{global_info, global_warn},
    oidc::OidcHttpOptions,
    provenance_gen::{ProvenanceGenError, SignProvenance},
    publish_options::{
        Access, CreatePublishOptionsError, CreatePublishOptionsInput, create_publish_options,
    },
    publish_summary::{PackedPkgInfo, PublishSummary, create_publish_summary},
    publish_wait::{PublishWaitError, wait_for_published_packages},
    registry_config_keys::NormalizedRegistryUrl,
    release_notes::ReleaseNotesTarget,
    staged_archives::{StagingError, stage_archives},
};

/// The packed package handed to [`publish_packed_pkg`]: the manifest that was
/// packed, the tarball's bytes, and the file listing / unpacked size used for
/// the summary.
pub struct PackedPkg<'a> {
    /// The manifest of the version: its `any` archive's `package.json`, or
    /// the base manifest of a version without one.
    pub published_manifest: &'a Value,
    /// The `any` archive's bytes; empty when another machine staged it
    /// (`any_staged`) or the version, published with variants only, has
    /// none.
    pub tarball_data: &'a [u8],
    pub tarball_path: &'a str,
    pub contents: &'a [String],
    pub unpacked_size: u64,
    /// The version note, sent only to a registry that stores notes.
    pub notes: Option<&'a str>,
    /// The platform variants published with the version, to a registry
    /// declared with `serverType: pnpr`; empty for a package without them.
    pub variants: &'a [PackedVariant],
    /// The `any` archive, staged before this publish by another machine;
    /// `tarball_data` is then empty.
    pub any_staged: Option<&'a StagedReference>,
}

impl PackedPkg<'_> {
    /// Whether the version is published as several archives, which pnpm
    /// stages and references: it has platform variants, or an `any` archive
    /// another job staged.
    pub(crate) fn publishes_staged_archives(&self) -> bool {
        !self.variants.is_empty() || self.any_staged.is_some()
    }

    /// Whether the version has an `any` archive: its bytes, or one another
    /// machine staged.
    pub(crate) fn has_any_archive(&self) -> bool {
        !self.tarball_data.is_empty() || self.any_staged.is_some()
    }

    pub(crate) fn summary(&self) -> PublishSummary {
        let mut summary = create_publish_summary(
            &PackedPkgInfo {
                published_manifest: self.published_manifest,
                tarball_path: self.tarball_path,
                contents: self.contents,
                unpacked_size: self.unpacked_size,
            },
            self.tarball_data,
        );
        // This publish has no bytes of an archive another machine staged.
        if let Some(staged) = self.any_staged {
            summary.integrity.clone_from(&staged.integrity);
            summary.shasum = String::new();
        }
        summary
    }
}

/// The configuration [`publish_packed_pkg`] reads. Credential and TLS
/// resolution is handled by pacquet's shared [`AuthHeaders`] /
/// [`ThrottledClient`] rather than per-field options.
pub struct PublishPackedPkgOptions {
    pub dry_run: bool,
    pub stage: bool,
    pub wait_timeout: std::time::Duration,
    pub registry: PublishRegistryOptions,
    pub release_notes: ReleaseNotesTarget,
    /// The normalized URLs of the registries declared `serverType: pnpr`. A
    /// batch bound for one uploads its archives before the commit, which
    /// then references them by integrity.
    pub pnpr_registries: HashSet<String>,
}

impl PublishPackedPkgOptions {
    pub fn validate(&self) -> Result<(), PublishPackedPkgError> {
        if self.stage && !self.wait_timeout.is_zero() {
            return Err(PublishPackedPkgError::WaitWithStage);
        }
        Ok(())
    }
}

pub struct PublishRegistryOptions {
    pub default: String,
    pub scoped: BTreeMap<String, String>,
    pub access: Option<Access>,
    pub tag: String,
    pub otp: Option<String>,
    pub provenance: Option<bool>,
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
/// [`pnpm_network_web_auth::Host`].
pub async fn publish_packed_pkg<Sys, Reporter>(
    pkg: &PackedPkg<'_>,
    opts: &PublishPackedPkgOptions,
    network: &PublishNetwork<'_>,
) -> Result<PublishSummary, PublishFailure<PublishPackedPkgError>>
where
    Sys: EnvVar + Clock + OidcFetch + SignProvenance,
    Reporter: self::Reporter,
{
    opts.validate()?;
    let input = opts.registry.create_options_input();
    let resolved = create_publish_options::<Sys, Reporter>(pkg.published_manifest, &input, true)
        .await
        .map_err(PublishPackedPkgError::from)?;

    let name = manifest_string(pkg.published_manifest, "name");
    let version = manifest_string(pkg.published_manifest, "version");
    let registry = resolved.registry.clone();
    let is_stage = opts.stage;

    global_info::<Reporter>(&format!("📦 {name}@{version} → {}", registry_for_display(&registry)));

    let mut summary = pkg.summary();
    variants::check_registry(pkg, &opts.pnpr_registries, &registry)?;

    if opts.dry_run {
        global_warn::<Reporter>(&format!(
            "Skip {verb} {name}@{version} (dry run)",
            verb = if is_stage { "staging" } else { "publishing" },
        ));
        return Ok(summary);
    }

    let uploaded = upload_package::<Sys, Reporter>(pkg, opts, network, &resolved, &summary).await;
    let published = PublishedPkg { name: &name, version: &version, is_stage };
    let context = (opts, network, &resolved);
    release_retry::settle_publish::<Reporter>(pkg, context, uploaded, (&mut summary, &published))
        .await?;
    wait_for_published_packages::<Reporter>(
        &[(&name, &version)],
        &registry,
        network,
        opts.wait_timeout,
    )
    .await
    .map_err(|error| PublishFailure {
        published: vec![summary.clone()],
        error: PublishPackedPkgError::Wait(error),
    })?;
    Ok(summary)
}

async fn upload_package<Sys, Reporter>(
    pkg: &PackedPkg<'_>,
    opts: &PublishPackedPkgOptions,
    network: &PublishNetwork<'_>,
    resolved: &crate::publish_options::ResolvedPublishOptions,
    summary: &PublishSummary,
) -> Result<PublishResponse, PublishPackedPkgError>
where
    Sys: EnvVar + Clock + OidcFetch + SignProvenance,
    Reporter: self::Reporter,
{
    let name = &summary.name;
    let version = &summary.version;
    let registry = &resolved.registry;
    let is_stage = opts.stage;
    let authorization = publish_authorization(resolved, network, registry, name);
    let mut lengths = variants::StagedLengths::new();
    if pkg.publishes_staged_archives() {
        lengths = variants::staged_lengths(pkg, network.client, registry, authorization.as_deref())
            .await?;
        let archives = variants::staged_archives(pkg, &summary.integrity);
        stage_archives(network.client, registry, authorization.as_deref(), &archives)
            .await
            .map_err(PublishPackedPkgError::Staging)?;
    }
    let body =
        publish_body::<Sys, Reporter>(pkg, opts, resolved, (summary, &lengths), name, version)
            .await?;

    let put_url = publish_endpoint(registry, name, is_stage)?;

    publish_with_otp_handling::<WebAuthHost, Reporter>(
        network.client,
        &put_url,
        authorization.as_deref(),
        if is_stage { "stage" } else { "publish" },
        body,
        resolved.otp.as_deref(),
        is_stage,
        web_auth_fetch_options(&opts.registry.http),
    )
    .await
    .map_err(PublishPackedPkgError::from)
}

/// Reuse the summary's digests and attach provenance when the resolved
/// publish options request it, including OIDC auto-detection.
async fn publish_body<Sys, Reporter>(
    pkg: &PackedPkg<'_>,
    opts: &PublishPackedPkgOptions,
    resolved: &crate::publish_options::ResolvedPublishOptions,
    (summary, lengths): (&PublishSummary, &variants::StagedLengths),
    name: &str,
    version: &str,
) -> Result<bytes::Bytes, PublishPackedPkgError>
where
    Sys: EnvVar + Clock + OidcFetch + SignProvenance,
    Reporter: self::Reporter,
{
    let mut document = build_publish_document(
        pkg.published_manifest,
        pkg.tarball_data,
        &resolved.registry,
        resolved.access,
        &resolved.default_tag,
        &DistHashes { integrity: &summary.integrity, shasum: &summary.shasum },
    )?;
    opts.release_notes.attach_version_notes(&mut document, pkg.notes, &resolved.registry);
    let has_variants = pkg.publishes_staged_archives();
    if has_variants {
        variants::reference_staged_archives(&mut document, pkg, (version, lengths));
    }

    if resolved.provenance == Some(true) {
        attach_provenance::<Sys, Reporter>(
            &mut document,
            (name, version),
            pkg,
            &opts.registry.http,
        )
        .await?;
    }
    let body =
        bytes::Bytes::from(serde_json::to_vec(&document).expect("serialize publish document"));

    Ok(body)
}

impl PublishRegistryOptions {
    fn create_options_input(&self) -> CreatePublishOptionsInput<'_> {
        CreatePublishOptionsInput {
            default_registry: &self.default,
            scoped_registries: &self.scoped,
            access: self.access,
            tag: &self.tag,
            otp: self.otp.as_deref(),
            provenance: self.provenance,
            http: &self.http,
        }
    }
}

/// What a finished publish reports on.
#[derive(Clone, Copy)]
struct PublishedPkg<'a> {
    name: &'a str,
    version: &'a str,
    is_stage: bool,
}

/// Record what the registry answered: a staged publish keeps its stage id, and
/// anything but success is an error naming the package.
fn finish_publish<Reporter: self::Reporter>(
    response: PublishResponse,
    summary: &mut PublishSummary,
    published: &PublishedPkg<'_>,
) -> Result<(), PublishPackedPkgError> {
    let PublishedPkg { name, version, is_stage } = *published;
    if !response.ok {
        return Err(PublishPackedPkgError::FailedToPublish(FailedToPublishError::new(
            name,
            version,
            is_stage,
            response.status,
            response.status_text,
            response.body,
        )));
    }
    if is_stage {
        summary.stage_id = response.stage_id;
    }
    let verb = if is_stage { "Staged" } else { "Published" };
    global_info::<Reporter>(&format!("✅ {verb} package {name}@{version}"));
    Ok(())
}

/// The `Authorization` header the publish carries: an explicit token override,
/// else whatever the caller's auth config offers for this registry and package.
fn publish_authorization(
    resolved: &crate::ResolvedPublishOptions,
    network: &PublishNetwork<'_>,
    registry: &NormalizedRegistryUrl,
    name: &str,
) -> Option<String> {
    resolved.auth_token_override
        .as_ref()
        .map(|token| format!("Bearer {token}"))
        .or_else(|| network.auth_headers.for_url_with_package(registry.as_str(), Some(name)))
}

/// Where the package document is sent. A staged publish goes to the registry's
/// staging endpoint (a `POST` in [`put_publish`](crate::publish_packed_pkg::request::put_publish)); a regular publish PUTs the
/// document directly.
fn publish_endpoint(
    registry: &NormalizedRegistryUrl,
    name: &str,
    is_stage: bool,
) -> Result<String, PublishPackedPkgError> {
    let escaped = escaped_package_name(name);
    let path = if is_stage { format!("-/stage/package/{escaped}") } else { escaped };
    join_registry(registry, &path)
}

/// Sign an SLSA attestation with sigstore and splice it into the document's
/// `_attachments`.
///
/// Provenance is requested either explicitly (`--provenance`) or by OIDC
/// auto-detection for a public repo; the caller has already merged the two.
/// Resolve `path` against the registry the way `new URL(path, registry)` does.
pub(crate) fn join_registry(
    registry: &NormalizedRegistryUrl,
    path: &str,
) -> Result<String, PublishPackedPkgError> {
    registry.join(path).map_err(PublishPackedPkgError::InvalidUrl)
}

/// Render the registry URL for logging (see
/// [`NormalizedRegistryUrl::for_display`]); the unsanitized URL is still used
/// for the request and auth-header lookup.
pub(crate) fn registry_for_display(registry: &NormalizedRegistryUrl) -> String {
    registry.for_display()
}

/// Failure surface of [`publish_packed_pkg`].
#[derive(Debug, derive_more::Display, derive_more::Error, Diagnostic)]
pub enum PublishPackedPkgError {
    #[display("--publish-wait-timeout cannot be used with stage publish")]
    #[diagnostic(
        code(ERR_PNPM_PUBLISH_WAIT_WITH_STAGE),
        help(
            "Staged versions are not available for installation. Set --publish-wait-timeout=0 to stage a package."
        )
    )]
    WaitWithStage,

    #[diagnostic(transparent)]
    Wait(PublishWaitError),

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

    #[display("Invalid package name \"{name}\".")]
    #[diagnostic(code(ERR_PNPM_INVALID_PACKAGE_NAME))]
    InvalidPackageName { name: String },

    #[display("Invalid semver: {version}")]
    #[diagnostic(code(ERR_PNPM_BAD_SEMVER))]
    BadSemver { version: String },

    #[diagnostic(transparent)]
    Provenance(ProvenanceGenError),

    #[diagnostic(transparent)]
    PublishedRelease(release_retry::PublishedReleaseError),

    #[display("invalid registry URL: {_0}")]
    InvalidUrl(url::ParseError),

    #[diagnostic(transparent)]
    Otp(WithOtpError<PublishHttpError>),

    #[diagnostic(transparent)]
    FailedToPublish(FailedToPublishError),

    #[display(
        "Platform variants can only be published to a registry declared with serverType: pnpr, and {registry} is not"
    )]
    #[diagnostic(
        code(ERR_PNPM_VARIANTS_NEED_PNPR),
        help(
            "Declare the registry with serverType: pnpr in pnpm-workspace.yaml. To publish only the archive that targets any, pass --no-variants."
        )
    )]
    VariantsNeedPnpr { registry: String },

    #[display("{name} has no package URL to name it in a provenance statement: {error}")]
    #[diagnostic(code(ERR_PNPM_PROVENANCE_PURL))]
    Purl {
        name: String,
        #[error(source)]
        error: pnpm_release_record::PurlError,
    },

    #[display("The archive {integrity} has no SHA-512 for a provenance statement to name it by")]
    #[diagnostic(code(ERR_PNPM_PROVENANCE_NO_SHA512))]
    NoSha512 { integrity: String },

    #[diagnostic(transparent)]
    Staging(StagingError),

    #[display("--upload-variant cannot sign provenance")]
    #[diagnostic(
        code(ERR_PNPM_UPLOAD_VARIANT_PROVENANCE),
        help(
            "Sign provenance in the job that publishes the version with --variant-integrity: its statement names every archive."
        )
    )]
    UploadWithProvenance,

    #[display("The archive for {label}, {integrity}, is not staged on {registry} for this account")]
    #[diagnostic(
        code(ERR_PNPM_VARIANT_NOT_STAGED),
        help(
            "Run the job that uploads it with pnpm publish --upload-variant again. Staged archives belong to the account that uploaded them and expire a day after their last upload, so the upload jobs must use this job's credentials."
        )
    )]
    NotStaged { label: String, integrity: String, registry: String },
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

mod document;
use document::manifest_string;
mod provenance;
mod release_retry;
mod uploads;
mod variants;

use provenance::attach_provenance;

mod request;
