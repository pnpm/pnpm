pub(crate) use client_ip::{ClientIp, resolve_client_ip};
pub use lifecycle::{recover_publish_journal, serve, serve_listener};
pub(crate) use metrics::{ServerMetrics, serve_metrics};
pub(crate) use request_access::compute_upstream_cache_namespace;
pub(crate) use request_id::{RequestId, generate_request_id, is_valid_request_id};
pub(crate) use tar_variants::NAMESPACE as TAR_VARIANT_CACHE_NAMESPACE;
pub(crate) use tcp_listener::PeerAddr;

pub(crate) use self::striped_locks::StripedLocks;

mod lifecycle;
use lifecycle::load_active_osv_index;

mod tcp_listener;
use tcp_listener::NodelayTcpListener;

mod pnpr_protocol;
use pnpr_protocol::{
    pnpr_protocols_disabled, serve_artifact_blob, serve_artifact_blob_size, serve_pnpr_handshake,
    serve_publish_artifact, serve_resolve, serve_resolve_artifacts, serve_upload_artifact_blob,
    serve_verify_lockfile,
};

mod pipeline_runs;
use pipeline_runs::{
    serve_get_pipeline_run, serve_list_pipeline_runs, serve_pipeline_ui, serve_publish_pipeline_run,
};

mod organizations;
use organizations::{get_org_teams, get_team_members, serve_org_packages};

mod package_search;
use package_search::{
    DiscoverySource, SearchPage, discovery_sources, hosted_search_names, serve_search,
};

mod osv_filtering;
use osv_filtering::{
    ensure_osv_allowed, filter_osv_vulnerable_dist_tags, filter_osv_vulnerable_versions,
    is_osv_vulnerable_packument_version, resolve_version_or_tag,
};

mod tar_registry;
use tar_registry::{
    DistView, describe_tar, describe_tars, dist_view, is_tar_registry, restore_tgz_integrities,
    split_package_identifier, tar_integrities_of, tgz_integrity_key,
};

mod tar_responses;
use tar_responses::{
    CachedTgz, MAX_CONCURRENT_TAR_DECODES, TarSource, TgzTrust, tar_decode_limit, tar_integrity_of,
    tar_integrity_within, tar_resource_integrity, tar_response,
};

mod version_tarballs;
use version_tarballs::{VersionTgzCache, serve_package_tarball};

mod tar_variants;
use tar_variants::{TarCopy, TarVariantQueue, open_variant};

mod package_responses;
use package_responses::{
    DistBlock, PackumentDists, TarballDist, expected_tarball_dist, packument_bytes_response,
    packument_response, tarball_integrity_error, tarball_response, tarball_stream_error,
    tarball_stream_error_for_package,
};

mod package_reads;
use package_reads::{
    HostedGate, RegistrySource, default_registry_target, hosted_gate, hosted_read_namespace,
    resolve_ecosystem_source, resolve_registry_source, resolves_to_private_source, serve_packument,
    serve_registry_tarball, serve_tarball, serve_version_manifest,
};

mod revision_refs;
use revision_refs::{
    RevisionScan, RevisionSource, hosted_revision_refs, hosted_revision_sources,
    serve_private_revision_refs, serve_revision_refs,
};

mod revision_tarballs;
use revision_tarballs::{
    HostedOriginal, declared_tarball_integrity, hosted_original, open_hosted_revision_tarball,
    serve_revision_tarball,
};

mod upstream_tarballs;
use upstream_tarballs::{
    BackgroundTarScans, TarballOrigin, UpstreamTarball, cached_upstream_tarball,
    serve_bound_upstream_tarball, serve_tarball_via_upstream, tarball_cache_name,
};

mod upstream_integrity;
mod upstream_packuments;
use upstream_packuments::{
    load_packument_for_read, load_upstream_packument, read_source_packument,
    recover_stale_upstream_packument, serve_packument_via_upstream,
};

mod request_access;
use request_access::{
    authorized_revision_upstream, authorized_upstream, require_admin, require_artifact_caller,
    require_caller, require_pipeline_caller, require_resolver_caller, revision_registry_is_private,
    revision_source_registry, single_authorization_header, upstream_cache_namespace,
};

mod restrictions;
mod tokens;
mod user_accounts;

mod attestations;
mod authentication;
mod batch;
mod cargo;
mod client_ip;
mod compiler_cache;
mod org_access;
use org_access::CallerOrgs;
mod documents;
mod ecosystem;
mod health;
use health::shutdown_signal;
mod managed_state;
mod metrics;
mod oci;
mod oidc;
mod oidc_groups;
mod package_mutation;
mod publishing;
mod pypi;
mod rate_limit;
use rate_limit::LoginRateLimiter;
mod registry_directory;
mod request_id;
mod routing;
mod rule_admin;
mod rule_overrides;
mod scim;
mod security_audit;
mod singleflight;
use singleflight::Singleflight;
mod staged;
mod striped_locks;
mod team_access;
mod team_mutations;
mod team_rosters;
mod user_admin;
mod web_login;
mod web_ui;

#[cfg(test)]
mod tests;

use self::{
    authentication::{Action, AuthedCaller, authenticate, authorize, authorize_package},
    documents::RegistryDocuments,
    ecosystem::{addressed_registry, caller_scoped, registry_endpoint},
    package_mutation::{
        delete_package, delete_tarball, get_dist_tags, remove_dist_tag, set_dist_tag,
        update_packument,
    },
    publishing::{
        PublishTarget, check_publish_tombstone, commit_publishes, publish_package,
        resolve_publish_target_for, serve_batch_publish, stage_publish, validate_publish_doc,
    },
    routing::router_with_auth_and_osv,
};

use axum::{
    Router,
    body::Body,
    extract::{
        FromRequestParts, Path, RawPathParams, Request, State, connect_info::Connected,
        rejection::RawPathParamsRejection,
    },
    http::{HeaderMap, StatusCode, header, request::Parts},
    middleware::Next,
    response::{IntoResponse, Response},
    serve::IncomingStream,
};
use chrono::Utc;
use indexmap::IndexMap;
use pnpm_crypto_hash::{integrity_addressed_tarball_integrity, integrity_addressed_tarball_path};
use pnpr_auth::{AuthState, UpsertOutcome, identify};
use pnpr_config::{Config, HostedConfig};
use pnpr_error::RegistryError;
use pnpr_package_name::CanonicalPackageName;
use pnpr_policy::Identity;
use pnpr_registry::{ConcreteKind, Ecosystem, Registry, Resolved};
use pnpr_storage::{
    Storage,
    publish::{iso_from_unix_millis, now_iso},
    streaming,
};

use pnpr_upstream::{
    CacheValidators, FetchOutcome, PackumentFetch, Upstream, abbreviate_packument,
    extract_upstream_version_manifest, extract_version_manifest, rewrite_tarball_urls,
    rewrite_upstream_tarball_urls, tarball_basename,
};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use ssri::Integrity;
use std::{collections::HashSet, net::SocketAddr, sync::Arc, time::Duration};

/// MIME the npm registry uses for the abbreviated install-v1 form.
/// Matches what pacquet (and pnpm/npm/yarn) send in `Accept` when
/// resolving for an install — see pacquet's
/// `resolving-npm-resolver::ACCEPT_ABBREVIATED_DOC`. Returning the
/// full document instead bloats the wire by 2–10× on packuments with
/// long version histories.
const ABBREVIATED_CONTENT_TYPE: &str = "application/vnd.npm.install-v1+json";

/// Cap tarballs at 100 MiB while pnpr has to spool them to disk for SRI
/// verification. This bounds per-request temporary disk usage for
/// chunked or malicious upstream bodies.
const MAX_TARBALL_BYTES: u64 = 100 * 1024 * 1024;

/// Cap publish bodies at 100 MiB. The default axum body limit is
/// 2 MiB, far too small for a real package — npm itself caps publish
/// at 100 MiB and verdaccio inherits that limit. We apply it via
/// [`axum::extract::DefaultBodyLimit::max`] on the router rather than on each
/// route, so future write endpoints inherit the same ceiling.
const MAX_PUBLISH_BODY_BYTES: usize = MAX_TARBALL_BYTES as usize;

/// Cap adduser/login bodies far below the publish ceiling. The body is a
/// small couchdb-user JSON document, and login is the one body-accepting
/// endpoint reachable anonymously on every tier — letting it inherit the
/// 100 MiB publish limit would hand unauthenticated callers a cheap
/// buffer-and-parse amplifier.
const MAX_LOGIN_BODY_BYTES: usize = 64 * 1024;

/// Cap on a team mutation body, which names one team or one user.
const MAX_TEAM_BODY_BYTES: usize = 64 * 1024;

/// The `PoC` accepts blobs inline on artifact publication. Keep the buffered
/// request at the same ceiling as an npm package publish.
const MAX_ARTIFACT_PUBLISH_BODY_BYTES: usize = MAX_PUBLISH_BODY_BYTES;
const MAX_ARTIFACT_RESOLVE_BODY_BYTES: usize = 16 * 1024 * 1024;
const MAX_ARTIFACT_BLOB_BODY_BYTES: usize = 8 * 1024;
/// A pipeline run record is a summary plus a bounded event stream; well
/// under this in practice, with the ceiling guarding against a hostile
/// client rather than a large workspace.
const MAX_PIPELINE_RUN_BODY_BYTES: usize = 4 * 1024 * 1024;
mod state;
use state::AppInner;

#[derive(Clone)]
pub(super) struct AppState {
    pub(super) inner: Arc<AppInner>,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct HostedOriginalRef {
    package: String,
    version: String,
}

/// Build the axum [`Router`] with in-memory auth state. Convenient
/// for tests and for callers that don't want disk-backed users —
/// [`serve`] is the production entry point and goes through
/// [`router_with_auth`] with an [`AuthState::load`]-ed bundle so a
/// corrupted htpasswd file surfaces as a startup error.
///
/// The 2- and 3-segment routes do dispatch inside the handler rather
/// than registering overlapping parametric routes — matchit can't
/// disambiguate `/{scope}/{name}` from `/{name}/{version}` at the
/// router level, so we take both via one handler that branches on
/// the `@` prefix and the literal-`-` segment.
pub fn router(config: Config) -> Router {
    let max_users = config.identity.auth.htpasswd.max_users;
    router_with_auth(config, AuthState::in_memory_with_max_users(max_users))
}

/// Fallible counterpart to [`router`]: surfaces an invalid config, an
/// unloadable OSV database (when `osv.enabled`), and hosted-store settings
/// that don't build a client as errors instead of panicking, for embedders
/// that build the router directly rather than via [`serve`].
pub fn try_router(config: Config) -> pnpr_error::Result<Router> {
    let max_users = config.identity.auth.htpasswd.max_users;
    try_router_with_auth(config, AuthState::in_memory_with_max_users(max_users))
}

/// Like [`router`] but with a caller-supplied [`AuthState`].
///
/// Panics if the config is invalid, an enabled OSV database can't load, or
/// the hosted object store's settings don't build a client. Call
/// [`try_router_with_auth`] to handle these as recoverable errors.
pub fn router_with_auth(config: Config, auth: AuthState) -> Router {
    try_router_with_auth(config, auth)
        .expect("pnpr config must be valid, and any enabled OSV database and hosted-store client must build, before building the router")
}

/// Fallible counterpart to [`router_with_auth`].
pub fn try_router_with_auth(mut config: Config, auth: AuthState) -> pnpr_error::Result<Router> {
    // Enforce the "at least one surface enabled" invariant for embedders
    // that build and serve the router themselves rather than going through
    // `serve`/`serve_listener`.
    config.features.ensure_a_feature_is_enabled()?;
    config.routing.ensure_valid_registry_graph(config.features.registry.enabled)?;
    let osv_index = load_active_osv_index(&config)?;
    router_with_auth_and_osv(config, auth, osv_index)
}

/// The request URI as recorded in the access log. npm's logout protocol
/// (`DELETE .../-/user/token/{token}`, path-less or under a `/~<prefix>/`)
/// puts the raw bearer token in the URL path, and a reusable credential
/// must never reach a log line, so everything after that marker is
/// redacted. Web login polling (`/-/v1/login/poll/{token}`) is also
/// redacted, and OIDC callback and web login start queries are omitted.
/// Other URIs are logged verbatim; a false positive (a registry path that
/// merely embeds the marker) is redacted too, which only costs log detail
/// on a request no route serves.
fn loggable_uri(uri: &axum::http::Uri) -> String {
    let path = uri.path();
    if path.starts_with("/-/oidc/") || path.ends_with("/-/v1/login/start") {
        return path.to_string();
    }
    const TOKEN_MARKER: &str = "/-/user/token/";
    const POLL_MARKER: &str = "/-/v1/login/poll/";
    if let Some(index) = path.find(TOKEN_MARKER) {
        return format!("{}<redacted>", &path[..index + TOKEN_MARKER.len()]);
    }
    if let Some(index) = path.find(POLL_MARKER) {
        return format!("{}<redacted>", &path[..index + POLL_MARKER.len()]);
    }
    uri.to_string()
}

// --------------------------------------------------------------------
// Account routes — adduser/login, whoami, profile, token list and
// revocation, logout. Mounted on every tier (see the router construction
// in `router_with_auth_and_osv`), each with a `/~<name>/`-addressed twin.
// --------------------------------------------------------------------

/// The registry a `~<name>` path segment addresses, or `None` for a segment
/// that is not one. A bare `~` names no registry, so it reads as "not a
/// registry prefix" rather than as an empty name — every caller then treats
/// it the way it treats `foo`.
#[allow(dead_code, reason = "helper used across server tiers and in tests")]
pub(super) fn tilde_registry(segment: &str) -> Option<&str> {
    segment
        .strip_prefix('~')
        .filter(|registry| !registry.is_empty())
}

/// The registry a request addressed through a leading `/~<name>/`, or `None`
/// when it arrived on the path-less base.
///
/// Every route that answers under a registry prefix is registered twice — once
/// bare, once under `/~{registry}` — pointing at the same handler. This
/// extractor is what tells the two apart, so a handler states once that it is
/// prefix-aware instead of needing a near-identical twin.
///
/// A bare `~` names no registry, so it rejects with 404 rather than reading as
/// the path-less base.
pub(super) struct TargetRegistry(pub(super) Option<String>);

impl<RouterState: Send + Sync> FromRequestParts<RouterState> for TargetRegistry {
    type Rejection = Response;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &RouterState,
    ) -> Result<Self, Self::Rejection> {
        // `RawPathParams` reports only what the matched route captured, so an
        // absent `prefix` means this is the bare registration rather than a
        // prefixed request that happened to omit the segment.
        let params = RawPathParams::from_request_parts(parts, &()).await
            .map_err(|err| {
                match err {
                    // The client sent a segment that percent-decodes to invalid
                    // UTF-8. It cannot be a registry name, so answer it the same
                    // 404 every other malformed prefix gets rather than a 500 — a
                    // bad URL is not a server fault, and rendering it as one would
                    // also let a client fill the error log.
                    RawPathParamsRejection::InvalidUtf8InPathParam(_) => RegistryError::NotFound,
                    // The matched route registered no path parameters at all, which
                    // means the route table and this extractor disagree. Fail closed
                    // rather than serve the request as if it named no registry.
                    rejection => RegistryError::Internal {
                        reason: format!("path params unavailable: {rejection}"),
                    },
                }
                .into_response()
            })?;
        let Some((_, registry)) = params
            .iter()
            .find(|(name, _)| *name == "registry")
        else {
            return Ok(Self(None));
        };
        if !pnpr_package_name::is_safe_path_segment(registry) {
            return Err(RegistryError::NotFound.into_response());
        }
        Ok(Self(Some(registry.to_string())))
    }
}

/// Await `fut`, emitting its duration as a `pnpr::serve_timing` debug event
/// (`phase`, `package`, `elapsed_us`).
///
/// Enabling that target — `RUST_LOG=pnpr::serve_timing=debug`, or a pnpr `log:`
/// level of `debug`/`trace` — turns the upstream serve paths into a per-request
/// profile of where time goes: the upstream packument/tarball fetch vs the
/// on-disk cache read. Near zero-cost when the target is disabled: the
/// only always-on work is one `Instant::now()`; the field values (including
/// `elapsed`) are computed only when the event is enabled.
async fn timed<Fut: Future>(phase: &'static str, package: &str, fut: Fut) -> Fut::Output {
    let start = std::time::Instant::now();
    let out = fut.await;
    tracing::debug!(
        target: "pnpr::serve_timing",
        phase,
        package,
        elapsed_us = start.elapsed().as_micros() as u64,
    );
    out
}

fn json_response(status: StatusCode, body: &Value) -> Response {
    let bytes = serde_json::to_vec(body).expect("static-shape JSON serializes");
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(bytes))
        .expect("static-shape response always builds")
}

/// Mark a response as caller-scoped and uncacheable. Authenticated endpoints
/// can return per-user data keyed on the `Authorization` header, so a shared
/// HTTP cache that ignored `Vary` could hand one caller's data to another.
/// Apply this to every response branch so intermediaries cannot cache errors
/// either.
fn private_no_cache(mut response: Response) -> Response {
    use axum::http::HeaderValue;
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
    if let Some(existing) = headers.get(header::VARY) {
        if let Ok(existing_str) = existing.to_str()
            && !existing_str.contains("Authorization")
        {
            let combined = format!("{existing_str}, Authorization");
            if let Ok(val) = HeaderValue::from_str(&combined) {
                headers.insert(header::VARY, val);
            }
        }
    } else {
        headers.insert(header::VARY, HeaderValue::from_static("Authorization"));
    }
    response
}

/// The hosted storage view a publish writes to: a hosted namespace, or
/// the flat (path-less) store when `org` is `None`.
fn hosted_storage(state: &AppState, org: Option<&str>) -> Storage {
    match org {
        Some(org) => state.inner.storage.for_hosted(org),
        None => state.inner.storage.clone(),
    }
}

// --------------------------------------------------------------------
// Helpers.
// --------------------------------------------------------------------

/// Resolve the hosted storage namespace a non-publish write (dist-tag,
/// unpublish, packument update) targets, or the [`Response`] to return. A
/// write routes like a publish: through the addressed `/~<name>/` (or,
/// path-less, the default-target registry) to a hosted org, rejecting a name
/// routed to an upstream and 404ing when the path-less base has no default
/// target or the registry's access list denies the caller.
fn resolve_write_target(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    name: &CanonicalPackageName,
) -> Result<WriteTarget, RegistryError> {
    resolve_write_target_for(state, identity, registry, Ecosystem::Npm, name)
}

/// [`resolve_write_target`] for one ecosystem's surface.
fn resolve_write_target_for(
    state: &AppState,
    identity: &Identity,
    registry: Option<&str>,
    ecosystem: Ecosystem,
    name: &CanonicalPackageName,
) -> Result<WriteTarget, RegistryError> {
    match resolve_publish_target_for(state, identity, registry, ecosystem, name.as_str()) {
        PublishTarget::Hosted { source, org } => Ok(WriteTarget { source, org }),
        PublishTarget::Reject(reason) => Err(RegistryError::BadRequest { reason }),
        PublishTarget::Denied(response) => Err(response),
        PublishTarget::NotFound => Err(RegistryError::NotFound),
    }
}

/// The hosted registry a write resolved to: its name (for the
/// `publish`/`unpublish` rule lookup) and its storage namespace.
struct WriteTarget {
    source: String,
    org: String,
}

fn not_found() -> Response {
    RegistryError::NotFound.into_response()
}
