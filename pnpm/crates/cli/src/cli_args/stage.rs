//! `pacquet stage` — stage packages for publishing, deferring
//! proof-of-presence (2FA) approval to a later point in time.
//!
//! `stage publish` runs the regular publish pipeline (packing, lifecycle
//! scripts, OIDC/OTP) against the registry's staging endpoint; the remaining
//! subcommands — `list`, `view`, `approve`, `reject`, `download` — talk to
//! the registry's `-/stage` API directly.

mod summarize_tarball;

use std::{collections::HashMap, path::Path, time::Duration};

use clap::Args;
use derive_more::{Display, Error};
use miette::{Context, Diagnostic, IntoDiagnostic};
use pacquet_config::Config;
use pacquet_network::{
    RetryOpts, ThrottledClient, read_limited_body, redact_url_credentials, send_with_retry,
};
use pacquet_network_web_auth::{
    Host as WebAuthHost, OtpChallenge, OtpError, OtpErrorBody, WebAuthFetchOptions,
    WebAuthRetryOptions, WithOtpError, with_otp_handling,
};
use pacquet_publish::{Host, PublishSummary, resolve_otp_from_env};
use pacquet_reporter::{GlobalLog, LogEvent, LogLevel, Reporter};
use pacquet_resolving_npm_resolver::pick_registry_for_package;
use pacquet_resolving_parse_wanted_dependency::parse_wanted_dependency;
use serde::Deserialize;
use serde_json::Value;

use super::{
    dist_tag::body_display_string,
    publish::{PublishArgs, PublishFlags},
};
use crate::cli_args::registry_client::build_registry_client;
use summarize_tarball::{create_tarball_filename, summarize_tarball};

/// The staged-list page size; matches pnpm's paginated `-/stage` reads.
const PER_PAGE: usize = 100;
/// Fail-safe bound on the staged-list pagination loop, so a registry that
/// keeps answering full pages with an inflated `total` cannot drive it
/// forever.
const STAGE_LIST_MAX_PAGES: usize = 1000;
const STAGE_BODY_LIMIT: usize = 1024 * 1024;
const STAGE_ERROR_BODY_LIMIT: usize = 64 * 1024;
/// Cap on a staged tarball download; a registry response is
/// attacker-controlled input and must not exhaust memory.
const STAGE_TARBALL_BODY_LIMIT: usize = 512 * 1024 * 1024;
const STAGE_SUBCOMMANDS: &str = "publish, list, view, approve, reject, download";

#[derive(Debug, Args)]
pub struct StageArgs {
    /// Stage subcommand and arguments.
    pub params: Vec<String>,

    /// The base URL of the npm registry.
    #[clap(long)]
    pub registry: Option<String>,

    #[clap(flatten)]
    pub flags: PublishFlags,
}

#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum StageError {
    #[display("Stage subcommand is required")]
    #[diagnostic(code(ERR_PNPM_STAGE_SUBCOMMAND_REQUIRED), help("Use one of: {STAGE_SUBCOMMANDS}"))]
    SubcommandRequired,

    #[display(r#"Unknown stage subcommand "{subcommand}""#)]
    #[diagnostic(code(ERR_PNPM_STAGE_UNKNOWN_SUBCOMMAND), help("Use one of: {STAGE_SUBCOMMANDS}"))]
    UnknownSubcommand {
        #[error(not(source))]
        subcommand: String,
    },

    #[display(r#"Missing required <stage-id> for "pnpm stage {subcommand}""#)]
    #[diagnostic(code(ERR_PNPM_STAGE_ID_REQUIRED))]
    StageIdRequired {
        #[error(not(source))]
        subcommand: &'static str,
    },

    #[display("stage-id must be a valid UUID")]
    #[diagnostic(code(ERR_PNPM_INVALID_STAGE_ID))]
    InvalidStageId,

    #[display("Invalid package spec: {spec}")]
    #[diagnostic(code(ERR_PNPM_INVALID_PACKAGE_SPEC))]
    InvalidPackageSpec {
        #[error(not(source))]
        spec: String,
    },

    #[display("Version specifiers are not supported for listing staged packages")]
    #[diagnostic(code(ERR_PNPM_STAGE_VERSION_SPECIFIER_UNSUPPORTED))]
    VersionSpecifierUnsupported,

    #[display("Failed to {operation}: {reason}")]
    #[diagnostic(code(ERR_PNPM_STAGE_REGISTRY_ERROR))]
    RequestFailed {
        #[error(not(source))]
        operation: String,
        #[error(not(source))]
        reason: String,
    },

    #[display("Could not read package.json from tarball")]
    #[diagnostic(code(ERR_PNPM_STAGE_TARBALL_MANIFEST_NOT_FOUND))]
    TarballManifestNotFound,

    #[display(r#"Invalid package name "{name}"."#)]
    #[diagnostic(code(ERR_PNPM_INVALID_PACKAGE_NAME))]
    InvalidPackageName {
        #[error(not(source))]
        name: String,
    },

    #[display(r#"Invalid package version "{version}"."#)]
    #[diagnostic(code(ERR_PNPM_INVALID_PACKAGE_VERSION))]
    InvalidPackageVersion {
        #[error(not(source))]
        version: String,
    },

    #[display(r#"Invalid tarball filename "{filename}"."#)]
    #[diagnostic(code(ERR_PNPM_INVALID_TARBALL_FILENAME))]
    InvalidTarballFilename {
        #[error(not(source))]
        filename: String,
    },
}

/// A failed `-/stage` registry response
/// (`ERR_PNPM_STAGE_REGISTRY_ERROR`), with the same message shape as the
/// TypeScript CLI's stage registry error.
#[derive(Debug, Display, Error, Diagnostic)]
#[display("{message}")]
#[diagnostic(code(ERR_PNPM_STAGE_REGISTRY_ERROR))]
pub struct StageRegistryError {
    #[error(not(source))]
    message: String,
}

impl StageRegistryError {
    fn new(action: &str, status: u16, status_text: &str, body: &str) -> Self {
        let status_display = if status_text.is_empty() {
            status.to_string()
        } else {
            format!("{status} {status_text}")
        };
        let trimmed = body.trim();
        let message = if trimmed.is_empty() {
            format!("Failed to {action} (status {status_display})")
        } else {
            format!("Failed to {action} (status {status_display}): {trimmed}")
        };
        StageRegistryError { message }
    }
}

/// One page of the registry's `-/stage` listing.
#[derive(Debug, Deserialize)]
struct StageListResponse {
    items: Vec<Value>,
    total: usize,
}

impl StageArgs {
    /// Dispatch to the requested stage subcommand and return the output to
    /// print, if any.
    pub async fn run<Reporter: self::Reporter>(
        self,
        dir: &Path,
        config: &Config,
        recursive: bool,
    ) -> miette::Result<Option<String>> {
        match self.params.first().map(String::as_str) {
            Some("publish") => self.stage_publish::<Reporter>(dir, config, recursive).await,
            Some("list") => self.stage_list(config).await,
            Some("view") => self.stage_view(config).await,
            Some("approve") => self.stage_approve::<Reporter>(config).await,
            Some("reject") => self.stage_reject::<Reporter>(config).await,
            Some("download") => self.stage_download(dir, config).await,
            None => Err(StageError::SubcommandRequired.into()),
            Some(other) => {
                Err(StageError::UnknownSubcommand { subcommand: other.to_owned() }.into())
            }
        }
    }

    /// `stage publish` — the regular publish pipeline against the staging
    /// endpoint, rendered as `+ <pkg> (staged with id <id>)` lines or a
    /// name-keyed JSON object.
    async fn stage_publish<Reporter: self::Reporter>(
        self,
        dir: &Path,
        config: &Config,
        recursive: bool,
    ) -> miette::Result<Option<String>> {
        let StageArgs { params, flags, .. } = self;
        let json = flags.json;
        let dry_run = flags.dry_run;
        let publish = PublishArgs { package: params.get(1).cloned(), flags };
        let published =
            publish.publish_packages::<Reporter>(dir, config, recursive, /* stage */ true).await?;
        let summaries = published.summaries();
        if json {
            let keyed = key_by_package_name(summaries);
            return Ok(Some(json_pretty(&Value::Object(keyed))?));
        }
        if summaries.is_empty() {
            return Ok(None);
        }
        let lines: Vec<String> = summaries
            .iter()
            .map(|summary| render_stage_publish_summary(summary, dry_run))
            .collect();
        Ok(Some(lines.join("\n")))
    }

    /// `stage list [<package-spec>]` — every staged version, paginated.
    async fn stage_list(&self, config: &Config) -> miette::Result<Option<String>> {
        let package_filter = parse_package_filter(self.params.get(1))?;
        let context = self.stage_context(config, package_filter.as_deref())?;
        let mut items: Vec<Value> = Vec::new();
        let mut page: usize = 0;
        loop {
            let mut url = stage_endpoint_url(&context.registry, "-/stage")?;
            url.query_pairs_mut()
                .append_pair("page", &page.to_string())
                .append_pair("perPage", &PER_PAGE.to_string());
            if let Some(package) = &package_filter {
                url.query_pairs_mut().append_pair("package", package);
            }
            let response: StageListResponse =
                stage_json_request(&context, url.as_str(), "list staged packages").await?;
            let page_len = response.items.len();
            items.extend(response.items);
            if items.len() >= response.total || page_len < PER_PAGE {
                break;
            }
            page += 1;
            if page >= STAGE_LIST_MAX_PAGES {
                break;
            }
        }

        if self.flags.json {
            return Ok(Some(json_pretty(&Value::Array(items))?));
        }
        if items.is_empty() {
            return Ok(Some(match package_filter {
                Some(package) => format!(r#"No staged versions of package name "{package}"."#),
                None => "No staged packages found.".to_owned(),
            }));
        }
        let rendered: Vec<String> = items.iter().map(render_stage_item).collect();
        Ok(Some(rendered.join("\n\n")))
    }

    /// `stage view <stage-id>` — one staged version's metadata.
    async fn stage_view(&self, config: &Config) -> miette::Result<Option<String>> {
        let stage_id = require_stage_id(&self.params, "view")?;
        let context = self.stage_context(config, None)?;
        let url = stage_endpoint_url(&context.registry, &format!("-/stage/{stage_id}"))?;
        let item: Value =
            stage_json_request(&context, url.as_str(), &format!("view staged package {stage_id}"))
                .await?;
        if self.flags.json {
            return Ok(Some(json_pretty(&item)?));
        }
        Ok(Some(render_stage_item(&item)))
    }

    /// `stage approve <stage-id>` — publish the staged version, satisfying an
    /// OTP / web-auth challenge if the registry raises one.
    async fn stage_approve<Reporter: self::Reporter>(
        &self,
        config: &Config,
    ) -> miette::Result<Option<String>> {
        let stage_id = require_stage_id(&self.params, "approve")?;
        let context = self.stage_context(config, None)?;
        let url = stage_endpoint_url(&context.registry, &format!("-/stage/{stage_id}/approve"))?;
        stage_request_with_otp::<Reporter>(
            &context,
            reqwest::Method::POST,
            url.as_str(),
            &format!("approve staged package {stage_id}"),
        )
        .await?;
        Ok(Some(format!("Staged package {stage_id} approved and published successfully.")))
    }

    /// `stage reject <stage-id>` — permanently delete the staged version.
    async fn stage_reject<Reporter: self::Reporter>(
        &self,
        config: &Config,
    ) -> miette::Result<Option<String>> {
        let stage_id = require_stage_id(&self.params, "reject")?;
        let context = self.stage_context(config, None)?;
        global_warn::<Reporter>(
            "Rejecting will permanently delete this staged publish record and tarball from the \
             registry.",
        );
        let url = stage_endpoint_url(&context.registry, &format!("-/stage/{stage_id}"))?;
        stage_request_with_otp::<Reporter>(
            &context,
            reqwest::Method::DELETE,
            url.as_str(),
            &format!("reject staged package {stage_id}"),
        )
        .await?;
        Ok(Some(format!("Staged package {stage_id} has been rejected.")))
    }

    /// `stage download <stage-id>` — fetch the staged tarball into `dir` and
    /// print its summary.
    async fn stage_download(&self, dir: &Path, config: &Config) -> miette::Result<Option<String>> {
        let stage_id = require_stage_id(&self.params, "download")?;
        let context = self.stage_context(config, None)?;
        let url = stage_endpoint_url(&context.registry, &format!("-/stage/{stage_id}/tarball"))?;
        let action = format!("download staged package {stage_id}");
        let (_guard, response) = stage_send(&context, reqwest::Method::GET, url.as_str(), None)
            .await
            .map_err(|source| request_failed(&action, source))?;
        if !response.status().is_success() {
            return Err(registry_error_from_response(response, &action).await.into());
        }
        let tarball_data = read_limited_body(response, STAGE_TARBALL_BODY_LIMIT)
            .await
            .map_err(|source| request_failed(&action, source))?;
        if tarball_data.truncated {
            return Err(StageError::RequestFailed {
                operation: action,
                reason: format!("registry response exceeded {STAGE_TARBALL_BODY_LIMIT} bytes"),
            }
            .into());
        }
        let tarball_data = tarball_data.bytes;

        let mut summary = summarize_tarball(&tarball_data)?;
        let filename = create_tarball_filename(&summary.name, &summary.version, Some(stage_id))?;
        summary.filename.clone_from(&filename);
        let output_path = dir.join(&filename);
        // `create_tarball_filename` already rejects separators; this guards
        // the write against any bare-basename assumption it might not cover.
        if output_path.file_name().map(|name| name.to_string_lossy().into_owned())
            != Some(filename.clone())
            || output_path.parent() != Some(dir)
        {
            return Err(StageError::InvalidTarballFilename { filename }.into());
        }
        std::fs::write(&output_path, &tarball_data)
            .into_diagnostic()
            .wrap_err_with(|| format!("write {}", output_path.display()))?;

        if self.flags.json {
            let mut keyed = serde_json::Map::new();
            keyed.insert(
                summary.name.clone(),
                serde_json::to_value(&summary).expect("a publish summary serializes"),
            );
            return Ok(Some(json_pretty(&Value::Object(keyed))?));
        }
        Ok(Some(format!("{}\n{filename}", render_tarball_summary(&summary))))
    }

    /// Shared per-subcommand request context: the resolved registry, its auth
    /// header (package-scoped when a package filter is given), the network
    /// client, and the configured OTP.
    fn stage_context(
        &self,
        config: &Config,
        package_name: Option<&str>,
    ) -> miette::Result<StageContext> {
        let mut registries: HashMap<String, String> =
            config.resolved_registries().into_iter().collect();
        if let Some(registry) = &self.registry {
            registries.insert("default".to_owned(), registry.clone());
        }
        let registry = match package_name {
            Some(package) => pick_registry_for_package(&registries, package, None),
            None => registries.get("default").cloned().unwrap_or_default(),
        };
        let registry = if registry.ends_with('/') { registry } else { format!("{registry}/") };
        let auth_header = config.auth_headers.for_url_with_package(&registry, package_name);
        Ok(StageContext {
            registry,
            auth_header,
            http_client: build_registry_client(config)?,
            retry_opts: RetryOpts {
                retries: config.fetch_retries,
                factor: config.fetch_retry_factor,
                min_timeout: Duration::from_millis(config.fetch_retry_mintimeout),
                max_timeout: Duration::from_millis(config.fetch_retry_maxtimeout),
            },
            otp: resolve_otp_from_env::<Host>(self.flags.otp.clone()),
            web_auth_fetch_options: WebAuthFetchOptions {
                timeout: Some(config.fetch_timeout),
                retry: Some(WebAuthRetryOptions {
                    factor: Some(f64::from(config.fetch_retry_factor)),
                    max_timeout: Some(config.fetch_retry_maxtimeout),
                    min_timeout: Some(config.fetch_retry_mintimeout),
                    randomize: None,
                    retries: Some(config.fetch_retries),
                }),
            },
        })
    }
}

struct StageContext {
    registry: String,
    auth_header: Option<String>,
    http_client: ThrottledClient,
    retry_opts: RetryOpts,
    otp: Option<String>,
    web_auth_fetch_options: WebAuthFetchOptions,
}

/// An HTTP-level failure of a stage mutation, handed to
/// [`with_otp_handling`]. Only the [`Otp`](Self::Otp) arm is a challenge it
/// acts on; the rest propagate.
#[derive(Debug, Display, Error, Diagnostic)]
enum StageHttpError {
    #[display("the registry requested a one-time password")]
    Otp {
        #[error(not(source))]
        challenge: OtpChallenge,
    },

    #[display("{_0}")]
    #[diagnostic(transparent)]
    Registry(#[error(not(source))] StageRegistryError),

    #[display("{_0}")]
    #[diagnostic(transparent)]
    Request(#[error(not(source))] Box<StageError>),
}

impl OtpError for StageHttpError {
    fn as_otp_challenge(&self) -> Option<OtpChallenge> {
        match self {
            StageHttpError::Otp { challenge } => Some(challenge.clone()),
            StageHttpError::Registry(_) | StageHttpError::Request(_) => None,
        }
    }
}

/// Send one stage mutation (approve / reject) with OTP / web-auth handling:
/// the first attempt carries any configured `--otp`; a 401 OTP challenge
/// drives the interactive flow and retries with the obtained password.
async fn stage_request_with_otp<Reporter: self::Reporter>(
    context: &StageContext,
    method: reqwest::Method,
    url: &str,
    action: &str,
) -> miette::Result<()> {
    with_otp_handling::<WebAuthHost, Reporter, (), StageHttpError, _, _>(
        context.web_auth_fetch_options.clone(),
        // A plain `FnMut` returning an `async move` block (not an
        // `AsyncFnMut`) so the produced future carries an ordinary `Send`
        // obligation — see `with_otp_handling`'s `Operation` bound.
        move |challenge_otp: Option<String>| {
            // The web-auth-provided OTP (a fresh challenge) takes precedence
            // over any statically configured one.
            let effective_otp = challenge_otp.or_else(|| context.otp.clone());
            let method = method.clone();
            async move {
                stage_mutation(context, method, url, action, effective_otp.as_deref()).await
            }
        },
    )
    .await
    .map_err(|error| match error {
        // Unwrap the operation's own failure so the user sees the registry
        // error once, not re-narrated through the OTP wrapper.
        WithOtpError::Operation(StageHttpError::Registry(registry_error)) => {
            miette::Report::new(registry_error)
        }
        WithOtpError::Operation(StageHttpError::Request(request_error)) => {
            miette::Report::new(*request_error)
        }
        other => miette::Report::new(other),
    })
}

/// Perform a single stage mutation request and classify the response.
async fn stage_mutation(
    context: &StageContext,
    method: reqwest::Method,
    url: &str,
    action: &str,
    otp: Option<&str>,
) -> Result<(), StageHttpError> {
    let (_guard, response) = stage_send(context, method, url, otp).await.map_err(|source| {
        StageHttpError::Request(Box::new(request_failed_error(action, source)))
    })?;
    let status = response.status();
    if status.is_success() {
        return Ok(());
    }
    let status_text = status.canonical_reason().unwrap_or_default().to_owned();
    let www_authenticate = response
        .headers()
        .get(reqwest::header::WWW_AUTHENTICATE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let body = read_limited_body(response, STAGE_ERROR_BODY_LIMIT).await.map_err(|source| {
        StageHttpError::Request(Box::new(request_failed_error(action, source)))
    })?;
    if status.as_u16() == 401
        && let Some(challenge) = parse_stage_otp_challenge(www_authenticate.as_deref(), &body.bytes)
    {
        return Err(StageHttpError::Otp { challenge });
    }
    Err(StageHttpError::Registry(StageRegistryError::new(
        action,
        status.as_u16(),
        &status_text,
        &body_display_string(&body),
    )))
}

/// GET a `-/stage` endpoint and parse its JSON body.
async fn stage_json_request<Body: serde::de::DeserializeOwned>(
    context: &StageContext,
    url: &str,
    action: &str,
) -> miette::Result<Body> {
    let (_guard, response) = stage_send(context, reqwest::Method::GET, url, None)
        .await
        .map_err(|source| request_failed(action, source))?;
    if !response.status().is_success() {
        return Err(registry_error_from_response(response, action).await.into());
    }
    let body = read_limited_body(response, STAGE_BODY_LIMIT)
        .await
        .map_err(|source| request_failed(action, source))?;
    if body.truncated {
        return Err(StageError::RequestFailed {
            operation: action.to_owned(),
            reason: format!("registry response exceeded {STAGE_BODY_LIMIT} bytes"),
        }
        .into());
    }
    serde_json::from_slice(&body.bytes).map_err(|source| request_failed(action, source))
}

/// Send one request to a `-/stage` endpoint with the stage headers
/// (`npm-auth-type: web`, `npm-command: stage`, auth, optional OTP),
/// retrying transient failures.
async fn stage_send<'client>(
    context: &'client StageContext,
    method: reqwest::Method,
    url: &str,
    otp: Option<&str>,
) -> Result<(pacquet_network::ThrottledClientGuard<'client>, reqwest::Response), reqwest::Error> {
    send_with_retry(&context.http_client, url, context.retry_opts, |client| {
        let mut builder = client
            .request(method.clone(), url)
            .header("npm-auth-type", "web")
            .header("npm-command", "stage");
        if let Some(auth_header) = &context.auth_header {
            builder = builder.header("authorization", auth_header);
        }
        if let Some(otp) = otp {
            builder = builder.header("npm-otp", otp);
        }
        builder
    })
    .await
}

/// Map a failed (non-2xx) stage response to a [`StageRegistryError`].
async fn registry_error_from_response(
    response: reqwest::Response,
    action: &str,
) -> StageRegistryError {
    let status = response.status();
    let status_text = status.canonical_reason().unwrap_or_default().to_owned();
    let body = match read_limited_body(response, STAGE_ERROR_BODY_LIMIT).await {
        Ok(body) => body_display_string(&body),
        Err(_) => String::new(),
    };
    StageRegistryError::new(action, status.as_u16(), &status_text, &body)
}

/// Identify a 401 stage response as an OTP / web-auth challenge: a JSON body
/// carrying `authUrl` + `doneUrl` (the browser-based flow), or a
/// `www-authenticate` header mentioning `otp` (classic TOTP).
fn parse_stage_otp_challenge(www_authenticate: Option<&str>, body: &[u8]) -> Option<OtpChallenge> {
    let parsed: Option<Value> = serde_json::from_slice(body).ok();
    let read =
        |field: &str| parsed.as_ref().and_then(|json| json.get(field)?.as_str().map(str::to_owned));
    let auth_url = read("authUrl");
    let done_url = read("doneUrl");
    let has_web_auth_urls = auth_url.is_some() && done_url.is_some();
    let header_mentions_otp =
        www_authenticate.is_some_and(|value| value.to_lowercase().contains("otp"));
    if !has_web_auth_urls && !header_mentions_otp {
        return None;
    }
    Some(OtpChallenge { body: Some(OtpErrorBody { auth_url, done_url }) })
}

fn request_failed(action: &str, source: impl std::fmt::Display) -> miette::Report {
    request_failed_error(action, source).into()
}

fn request_failed_error(action: &str, source: impl std::fmt::Display) -> StageError {
    StageError::RequestFailed {
        operation: action.to_owned(),
        reason: redact_url_credentials(&source.to_string()),
    }
}

/// Resolve a `-/stage` path against the registry base URL.
fn stage_endpoint_url(registry: &str, path: &str) -> miette::Result<reqwest::Url> {
    reqwest::Url::parse(registry)
        .and_then(|url| url.join(path))
        .map_err(|source| request_failed("build the registry staging URL", source))
}

/// The `<stage-id>` argument of `view` / `approve` / `reject` / `download`,
/// validated as a UUID.
fn require_stage_id<'params>(
    params: &'params [String],
    subcommand: &'static str,
) -> Result<&'params str, StageError> {
    let stage_id = params.get(1).map(String::as_str).unwrap_or_default();
    if stage_id.is_empty() {
        return Err(StageError::StageIdRequired { subcommand });
    }
    if !is_uuid(stage_id) {
        return Err(StageError::InvalidStageId);
    }
    Ok(stage_id)
}

/// Whether `value` is a hyphenated UUID (`8-4-4-4-12` hex digits).
fn is_uuid(value: &str) -> bool {
    value.len() == 36
        && value.char_indices().all(|(index, char)| match index {
            8 | 13 | 18 | 23 => char == '-',
            _ => char.is_ascii_hexdigit(),
        })
}

/// The `list` package filter: a bare package name; a version specifier other
/// than `*` is rejected.
fn parse_package_filter(raw_spec: Option<&String>) -> Result<Option<String>, StageError> {
    let Some(raw_spec) = raw_spec.filter(|spec| !spec.is_empty()) else {
        return Ok(None);
    };
    let parsed = parse_wanted_dependency(raw_spec);
    let Some(name) = parsed.alias else {
        return Err(StageError::InvalidPackageSpec { spec: raw_spec.clone() });
    };
    match parsed.bare_specifier.as_deref() {
        None | Some("" | "*") => Ok(Some(name)),
        Some(_) => Err(StageError::VersionSpecifierUnsupported),
    }
}

/// The `--json` map `stage publish` and `stage download` print: summaries
/// keyed by package name.
fn key_by_package_name(summaries: &[PublishSummary]) -> serde_json::Map<String, Value> {
    let mut keyed = serde_json::Map::new();
    for summary in summaries {
        let key = if summary.name.is_empty() { summary.id.clone() } else { summary.name.clone() };
        if key.is_empty() {
            continue;
        }
        keyed.insert(key, serde_json::to_value(summary).expect("a publish summary serializes"));
    }
    keyed
}

/// One `+ <pkg> (staged...)` line of the non-JSON `stage publish` output.
fn render_stage_publish_summary(summary: &PublishSummary, dry_run: bool) -> String {
    if dry_run {
        return format!("+ {} (would stage)", summary.id);
    }
    match &summary.stage_id {
        Some(stage_id) => format!("+ {} (staged with id {stage_id})", summary.id),
        None => format!("+ {} (staged)", summary.id),
    }
}

/// Render one staged item as `key: value` lines: the known fields in a fixed
/// order, then any extra fields the registry returned, `null`s skipped.
fn render_stage_item(item: &Value) -> String {
    let Some(object) = item.as_object() else {
        return render_value(item);
    };
    let mut lines: Vec<String> = Vec::new();
    let mut push = |key: &str, value: Option<&Value>| {
        if let Some(value) = value.filter(|value| !value.is_null()) {
            lines.push(format!("{key}: {}", render_value(value)));
        }
    };
    push("id", object.get("id"));
    push("package name", object.get("packageName"));
    push("version", object.get("version"));
    push("tag", object.get("tag"));
    push("date staged", object.get("createdAt"));
    let staged_by = match object
        .get("actorType")
        .and_then(Value::as_str)
        .filter(|actor_type| !actor_type.is_empty())
    {
        Some(actor_type) => {
            let actor = object
                .get("actor")
                .filter(|value| !value.is_null())
                .map(render_value)
                .unwrap_or_default();
            Some(Value::String(format!("{actor} ({actor_type})")))
        }
        None => object.get("actor").cloned(),
    };
    push("staged by", staged_by.as_ref());
    push("shasum", object.get("shasum"));
    const KNOWN_KEYS: [&str; 8] =
        ["id", "packageName", "version", "tag", "createdAt", "actor", "actorType", "shasum"];
    for (key, value) in object {
        if !KNOWN_KEYS.contains(&key.as_str()) {
            push(key, Some(value));
        }
    }
    lines.join("\n")
}

/// A value on a `key: value` line: strings raw, scalars via their JSON text,
/// objects and arrays as compact JSON.
fn render_value(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Bool(boolean) => boolean.to_string(),
        Value::Number(number) => number.to_string(),
        other => serde_json::to_string(other).expect("a JSON value serializes"),
    }
}

/// The non-JSON `stage download` report: the tarball's contents and details,
/// in pnpm's `renderTarballSummary` shape.
fn render_tarball_summary(summary: &PublishSummary) -> String {
    let files: Vec<&str> = summary.files.iter().map(|file| file.path.as_str()).collect();
    format!(
        "package: {name}@{version}\nTarball Contents\n{contents}\nTarball Details\nname: \
         {name}\nversion: {version}\nfilename: {filename}\npackage size: {size}\nunpacked size: \
         {unpacked_size}\nshasum: {shasum}\nintegrity: {integrity}\ntotal files: {entry_count}",
        name = summary.name,
        version = summary.version,
        contents = files.join("\n"),
        filename = summary.filename,
        size = summary.size,
        unpacked_size = summary.unpacked_size,
        shasum = summary.shasum,
        integrity = summary.integrity,
        entry_count = summary.entry_count,
    )
}

fn global_warn<Reporter: self::Reporter>(message: &str) {
    Reporter::emit(&LogEvent::Global(GlobalLog {
        level: LogLevel::Warn,
        message: message.to_owned(),
    }));
}

/// `JSON.stringify(value, null, 2)` — the two-space-indented JSON the
/// `--json` outputs print.
fn json_pretty(value: &Value) -> miette::Result<String> {
    serde_json::to_string_pretty(value).into_diagnostic()
}

#[cfg(test)]
mod tests;
