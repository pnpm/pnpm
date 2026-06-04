//! pnpr install accelerator: server-side dependency resolution plus file-level
//! store deduplication, exposed as an additive, opt-in protocol
//! alongside pnpr's npm-compatible API. The handshake + endpoints are
//! served under one base URL (the `pnprServer`).
//!
//! Two routes, built on pacquet's resolver and content-addressable
//! store:
//!
//! * `GET /-/pnpr` — capability handshake; advertises the supported
//!   protocol versions so a client can negotiate or fail fast.
//! * `POST /v1/install` — resolve a project **against the registries
//!   the client sends** (so the server uses the same source of truth as
//!   the client), then return, in a single gzipped binary response, the
//!   lockfile, stats, pre-packed store-index entries, and the contents of
//!   the files the client is missing (a length-prefixed JSON header
//!   followed by the binary file frames). One round trip
//!   ([pnpm/pnpm#12165](https://github.com/pnpm/pnpm/issues/12165)).
//!
//! Files are bound to access ([`authorize_served_packages`]): a
//! content-addressed digest is never a bearer capability. Anonymous
//! content is checked against pnpr's own `packages:` policy; content
//! fetched with the caller's forwarded credentials is gated per user
//! against the owning registry.
//!
//! The client's `registry`, `namedRegistries`, `overrides`, and the
//! verification policy (`minimumReleaseAge`, `trustPolicy`, ...) drive
//! resolution and verification. When the client sends its on-disk
//! lockfile, the server verifies it under the client's policy before
//! resolving, then reuses it as the resolution seed (frozen → as-is;
//! non-frozen → reuse-and-update). A multi-project workspace is resolved
//! by reconstructing the workspace on disk (root manifest +
//! `pnpm-workspace.yaml` + member manifests) and letting pacquet's
//! install path discover and resolve every importer. The client also
//! forwards its per-registry credentials, so private dependencies resolve
//! and fetch as the caller. Responses are buffered rather than truly
//! streamed.

mod diff;
mod grant_table;
mod protocol;
mod public_packages;
mod resolve;
mod verdict_cache;

#[cfg(test)]
mod tests;

use std::{
    collections::{HashMap, HashSet},
    io::Write as _,
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};

use crate::{
    config::Config as RegistryConfig,
    policy::{Identity, PackagePolicies},
};

use axum::{
    body::{Body, Bytes},
    http::{StatusCode, header},
    response::Response,
};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use flate2::{Compression, write::GzEncoder};
use indexmap::IndexMap;
use pacquet_config::Config as PacquetConfig;
use pacquet_lockfile::Lockfile;
use pacquet_lockfile_verification::{collect_resolution_policy_violations, hash_lockfile};
use pacquet_network::{AuthHeaders, ThrottledClient};
use pacquet_package_manager::build_resolution_verifiers;
use pacquet_resolving_npm_resolver::{InMemoryPackageMetaCache, PackageMetaCache, to_registry_url};
use pacquet_resolving_resolver_base::ResolutionVerifier;
use pacquet_store_dir::{StoreDir, StoreIndex};

use self::{
    grant_table::GrantTable, protocol::InstallRequest, public_packages::PublicPackages,
    verdict_cache::VerdictCache,
};

/// Per-server engine backing the pnpr install endpoints: it holds the
/// store, cache, and HTTP client used to resolve a client's project and
/// serve the files its store is missing. The store and cache dirs are
/// fixed for the server's lifetime; the *registries* come from each
/// client request (the server resolves against the client's registries,
/// not its own), so the `&'static Config` the install path requires is
/// interned per distinct client registry configuration rather than
/// leaked once or per request.
///
/// Held lazily in a [`OnceLock`] on the server's state so servers that
/// never receive such a request pay nothing, and so each server in
/// a multi-server test process keeps its own store.
pub(crate) struct InstallAccelerator {
    store_dir: StoreDir,
    cache_dir: PathBuf,
    client: Arc<ThrottledClient>,
    /// One leaked `Config` per distinct client registry configuration,
    /// keyed by its canonical JSON. Bounds the leak to the number of
    /// distinct client setups the server sees (typically one).
    configs: Mutex<HashMap<String, &'static PacquetConfig>>,
    /// SQLite-backed whole-lockfile verification verdict cache. `None`
    /// only if the database couldn't be opened — verification then runs
    /// every time (uncached) rather than failing the server.
    verdict_cache: Option<VerdictCache>,
    /// Per-`(user, name@version)` access grants for externally-resolved
    /// private content. `None` if the DB couldn't be opened (every such
    /// package then re-verifies uncached). See [`GrantTable`].
    grant_table: Option<GrantTable>,
    /// Global set of anonymously-readable package names, so a public
    /// package isn't gated per user. `None` if the DB couldn't be opened.
    /// See [`PublicPackages`].
    public_packages: Option<PublicPackages>,
    /// How long a grant (or public classification) stays valid. `None`
    /// (the default) is permanent, leaving revocation to
    /// clear-on-discovery; a TTL lets it bite already-seen versions.
    grant_ttl: Option<Duration>,
}

impl InstallAccelerator {
    pub(crate) fn get_or_init<'a>(
        cell: &'a OnceLock<InstallAccelerator>,
        config: &RegistryConfig,
    ) -> &'a InstallAccelerator {
        cell.get_or_init(|| InstallAccelerator::build(config))
    }

    fn build(config: &RegistryConfig) -> InstallAccelerator {
        let store_dir = config.storage.join("pnpr-store");
        let cache_dir = config.storage.join("pnpr-cache");
        // Best-effort: a real failure here (e.g. a permission problem)
        // resurfaces with a precise error on the first store/cache write
        // during resolution, so there's nothing actionable to report yet.
        let _ = std::fs::create_dir_all(&store_dir);
        let _ = std::fs::create_dir_all(&cache_dir);
        let verdict_cache = VerdictCache::open(&cache_dir.join("lockfile-verdicts.sqlite")).ok();
        let grant_table = GrantTable::open(&cache_dir.join("install-grants.sqlite")).ok();
        let public_packages = PublicPackages::open(&cache_dir.join("public-packages.sqlite")).ok();
        InstallAccelerator {
            store_dir: StoreDir::new(store_dir),
            cache_dir,
            client: Arc::new(ThrottledClient::new_for_installs()),
            configs: Mutex::new(HashMap::new()),
            verdict_cache,
            grant_table,
            public_packages,
            grant_ttl: config.install_accelerator_grant_ttl,
        }
    }

    /// Resolve (or build + intern) the `&'static Config` for a request's
    /// registry configuration. Pacquet's install path resolves against
    /// `config.registry` / `named_registries` / `overrides`, so a request
    /// from a client with a different registry setup gets its own Config.
    fn config_for(&self, request: &InstallRequest) -> &'static PacquetConfig {
        let registry =
            request.registry.clone().unwrap_or_else(|| "https://registry.npmjs.org/".to_string());
        let registry = if registry.ends_with('/') { registry } else { format!("{registry}/") };
        let overrides: Option<IndexMap<String, String>> =
            request.overrides.as_ref().and_then(|value| serde_json::from_value(value.clone()).ok());

        let key = serde_json::json!({
            "registry": registry,
            "namedRegistries": request.named_registries,
            "overrides": overrides,
            "minimumReleaseAge": request.minimum_release_age,
            "minimumReleaseAgeExclude": request.minimum_release_age_exclude,
            "minimumReleaseAgeIgnoreMissingTime": request.minimum_release_age_ignore_missing_time,
            "trustPolicy": request.trust_policy,
            "trustPolicyExclude": request.trust_policy_exclude,
            "trustPolicyIgnoreAfter": request.trust_policy_ignore_after,
        })
        .to_string();

        let mut configs = self.configs.lock().expect("config cache poisoned");
        if let Some(config) = configs.get(&key) {
            return config;
        }

        let mut config = PacquetConfig::new();
        config.store_dir = self.store_dir.clone();
        config.cache_dir = self.cache_dir.clone();
        config.registry = registry;
        config.named_registries = request.named_registries.clone();
        config.overrides = overrides;
        config.modules_dir = PathBuf::from("node_modules");
        config.lockfile = true;
        config.verify_store_integrity = true;
        // The client's verification policy drives both the input-lockfile
        // verifier and the resolver's pick-time `minimumReleaseAge` /
        // `trustPolicy` checks, so newly-resolved entries are held to the
        // same policy as the reused ones.
        config.minimum_release_age = request.minimum_release_age;
        config.minimum_release_age_exclude = request.minimum_release_age_exclude.clone();
        if let Some(ignore_missing_time) = request.minimum_release_age_ignore_missing_time {
            config.minimum_release_age_ignore_missing_time = ignore_missing_time;
        }
        config.trust_policy = request.trust_policy;
        config.trust_policy_exclude = request.trust_policy_exclude.clone();
        config.trust_policy_ignore_after = request.trust_policy_ignore_after;
        let config: &'static PacquetConfig = config.leak();
        configs.insert(key, config);
        config
    }
}

/// Handle `POST /v1/install`. `identity` is the resolved caller; the
/// store's possession of a package's bytes is not a capability to read
/// them, so every served package is authorized first — see
/// [`authorize_served_packages`].
pub(crate) async fn handle_install(
    runtime: &InstallAccelerator,
    policies: &PackagePolicies,
    identity: Identity,
    body: Bytes,
) -> Response {
    let request: InstallRequest = match serde_json::from_slice(&body) {
        Ok(request) => request,
        Err(err) => return json_error(StatusCode::BAD_REQUEST, &err.to_string()),
    };

    // Resolve against the client's registries, not the server's own.
    let config = runtime.config_for(&request);

    // The caller's forwarded upstream credentials, threaded through
    // resolve/verify/fetch but kept out of the interned `config` so it
    // never leaks a `&'static Config` per user.
    let request_auth = Arc::new(AuthHeaders::from_map(
        request.auth_headers.iter().map(|(uri, value)| (uri.clone(), value.clone())).collect(),
    ));

    // Verify the *input* lockfile under the client's policy before
    // resolving ([pnpm/pnpm#12139](https://github.com/pnpm/pnpm/issues/12139)).
    // The client skips its own `verifyLockfileResolutions` whenever a
    // pnpr server is configured, so this is the only place the
    // committed/reused entries get checked. A true first install sends
    // no lockfile — nothing to verify. `trustLockfile` is the client's
    // opt-out (mirrors the local path's `--trust-lockfile`). Freshly-
    // resolved entries are held to the same policy by the resolver's
    // pick-time gate (the policy is wired into `config`).
    if !request.trust_lockfile
        && let Some(input_lockfile) = request.lockfile.as_ref()
        && let Err(failure) =
            verify_input_lockfile(runtime, config, &request_auth, input_lockfile).await
    {
        return match failure {
            VerifyFailure::Internal(response) => response,
            VerifyFailure::Violations(violations) => violation_response(&violations),
        };
    }

    let lockfile = match resolve::resolve(config, &runtime.client, &request, &request_auth).await {
        Ok(lockfile) => lockfile,
        Err(err) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
    };

    let packages = resolve::collect_packages(&lockfile, &config.registry);

    // `pkg_id`s fetched from upstream this request: the registry accepted
    // the caller's token for each, so the gate treats them as proven.
    let mut freshly_fetched: HashSet<String> = HashSet::new();

    // `--lockfile-only`: pnpm resolves and writes the lockfile but
    // fetches nothing and links nothing. Skip the tarball fetch + the
    // file-level diff and return just the lockfile; the client writes it
    // and stops, so the response carries no `D`/`I` lines.
    // See [pnpm/pnpm#12146](https://github.com/pnpm/pnpm/issues/12146).
    let result = if request.lockfile_only {
        diff::DiffResult {
            missing_files: Vec::new(),
            package_index: Vec::new(),
            stats: diff::Stats { total_packages: packages.len() as u64, ..diff::Stats::default() },
        }
    } else {
        match resolve::fetch_uncached(config, &runtime.client, &request_auth, &packages).await {
            Ok(fetched) => freshly_fetched = fetched,
            Err(err) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
        }

        let store = match StoreIndex::open_readonly_in(&config.store_dir) {
            Ok(store) => store,
            Err(err) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
        };

        let diff_packages: Vec<diff::ResolvedPackage> = packages
            .iter()
            .map(|pkg| diff::ResolvedPackage {
                integrity: pkg.integrity.clone(),
                pkg_id: pkg.pkg_id.clone(),
            })
            .collect();

        match diff::compute_diff(&store, &diff_packages, &request.store_integrities) {
            Ok(result) => result,
            Err(err) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
        }
    };

    if let Some(denied) = authorize_served_packages(
        runtime,
        policies,
        &identity,
        &request,
        &request_auth,
        &freshly_fetched,
        &result.package_index,
    )
    .await
    {
        return denied;
    }

    let stats_json = stats_json(&result.stats);
    inline_response(runtime, &lockfile, &stats_json, &result)
}

fn stats_json(stats: &diff::Stats) -> serde_json::Value {
    serde_json::json!({
        "totalPackages": stats.total_packages,
        "alreadyInStore": stats.already_in_store,
        "packagesToFetch": stats.packages_to_fetch,
        "filesInNewPackages": stats.files_in_new_packages,
        "filesAlreadyInCafs": stats.files_already_in_cafs,
        "filesToDownload": stats.files_to_download,
        "downloadBytes": stats.download_bytes,
    })
}

/// Authorize every served package before its files leave the store (a
/// shared content digest is never a read capability), dispatched by
/// whether a forwarded credential was used to fetch it: such packages are
/// gated per user against the owning registry
/// ([`authorize_upstream_package`]); the rest by pnpr's local `packages:`
/// policy ([`deny_local_policy`]). Returns the first denial, or `None`.
async fn authorize_served_packages(
    runtime: &InstallAccelerator,
    policies: &PackagePolicies,
    identity: &Identity,
    request: &InstallRequest,
    request_auth: &AuthHeaders,
    freshly_fetched: &HashSet<String>,
    served: &[diff::PackageIndexEntry],
) -> Option<Response> {
    // The default registry pnpr resolved against (what `collect_packages`
    // / `fetch_uncached` built every tarball URL from). Per-scope external
    // registries are a future refinement.
    let registry = request.registry.as_deref().unwrap_or("https://registry.npmjs.org/");

    let mut local_pkg_ids: Vec<&str> = Vec::new();
    for entry in served {
        let Some(name) = package_name(&entry.pkg_id) else { continue };
        let pkg_url = to_registry_url(registry, name);
        if request_auth.for_url(&pkg_url).is_none() {
            local_pkg_ids.push(entry.pkg_id.as_str());
            continue;
        }
        if let Some(denied) = authorize_upstream_package(
            runtime,
            identity,
            request_auth,
            freshly_fetched,
            registry,
            name,
            &entry.pkg_id,
        )
        .await
        {
            return Some(denied);
        }
    }

    deny_local_policy(policies, identity, local_pkg_ids.into_iter())
}

/// Deny when the caller may not read a package gated by pnpr's own
/// `packages:` policy. 401 for anonymous, 403 for an authenticated caller
/// outside the allowed set; `None` when every name is readable.
fn deny_local_policy<'a>(
    policies: &PackagePolicies,
    identity: &Identity,
    pkg_ids: impl Iterator<Item = &'a str>,
) -> Option<Response> {
    let mut checked: HashSet<&str> = HashSet::new();
    for pkg_id in pkg_ids {
        let Some(name) = package_name(pkg_id) else { continue };
        if !checked.insert(name) {
            continue;
        }
        if !policies.for_package(name).access.allows(identity) {
            let status = match identity {
                Identity::Anonymous => StatusCode::UNAUTHORIZED,
                Identity::User { .. } => StatusCode::FORBIDDEN,
            };
            return Some(json_error(status, &format!("not authorized to access {name:?}")));
        }
    }
    None
}

/// Authorize one upstream-as-authority package: the owning registry, not
/// pnpr, decides. Known-public, freshly fetched, or already granted →
/// allow (recording a grant where applicable); otherwise probe the
/// registry anonymously (a `2xx` records it public globally) then
/// re-verify with the caller's token (`2xx` grants, `401`/`403` clears the
/// caller's grants and denies). Grants key on an identified user; the
/// global public set benefits anonymous callers too. See the body's
/// branches and the module tests for each path.
async fn authorize_upstream_package(
    runtime: &InstallAccelerator,
    identity: &Identity,
    request_auth: &AuthHeaders,
    freshly_fetched: &HashSet<String>,
    registry: &str,
    name: &str,
    pkg_id: &str,
) -> Option<Response> {
    // Public content needs no per-user gating, so it never reaches the
    // grant table or an upstream round trip once classified.
    if let Some(public) = runtime.public_packages.as_ref()
        && public.is_public(name, runtime.grant_ttl)
    {
        return None;
    }

    let user = match identity {
        Identity::User { username } => Some(username.as_str()),
        Identity::Anonymous => None,
    };
    let grants = || user.zip(runtime.grant_table.as_ref());

    // The cold fetch this request already proved access: the upstream
    // accepted the caller's forwarded token.
    if freshly_fetched.contains(pkg_id) {
        if let Some((user, table)) = grants() {
            table.record(user, pkg_id);
        }
        return None;
    }

    if let Some((user, table)) = grants()
        && table.is_granted(user, pkg_id, runtime.grant_ttl)
    {
        return None;
    }

    // Classify before gating per user: a package the registry serves
    // anonymously is public — record it globally so no one probes it
    // again. Only a token-gated package takes the per-user path below.
    if let UpstreamAccess::Authorized =
        probe_upstream_access(&runtime.client, None, registry, name).await
    {
        if let Some(public) = runtime.public_packages.as_ref() {
            public.record(name);
        }
        return None;
    }

    match probe_upstream_access(&runtime.client, Some(request_auth), registry, name).await {
        UpstreamAccess::Authorized => {
            if let Some((user, table)) = grants() {
                table.record(user, pkg_id);
            }
            None
        }
        UpstreamAccess::Denied => {
            if let Some((user, table)) = grants() {
                table.clear_package(user, name);
            }
            Some(json_error(StatusCode::FORBIDDEN, &format!("not authorized to access {name:?}")))
        }
        UpstreamAccess::Unknown => Some(json_error(
            StatusCode::BAD_GATEWAY,
            &format!("could not verify access to {name:?}"),
        )),
    }
}

/// Outcome of an upstream access probe.
enum UpstreamAccess {
    /// The upstream served the package's packument for the probe.
    Authorized,
    /// The upstream returned `401`/`403`.
    Denied,
    /// The upstream was unreachable or returned some other status; access
    /// can't be decided.
    Unknown,
}

/// Probe whether `name` is readable from `registry` by fetching its
/// (abbreviated) packument. `auth` set attaches the caller's credential
/// (a re-verify); `auth` `None` is anonymous (a public/private check).
async fn probe_upstream_access(
    client: &ThrottledClient,
    auth: Option<&AuthHeaders>,
    registry: &str,
    name: &str,
) -> UpstreamAccess {
    let url = to_registry_url(registry, name);
    let guard = client.acquire_for_url(&url).await;
    let mut request = guard.get(&url).header("accept", "application/vnd.npm.install-v1+json");
    if let Some(value) = auth.and_then(|auth| auth.for_url(&url)) {
        request = request.header("authorization", value);
    }
    match request.send().await {
        Ok(response) => {
            let status = response.status().as_u16();
            if (200..300).contains(&status) {
                UpstreamAccess::Authorized
            } else if status == 401 || status == 403 {
                UpstreamAccess::Denied
            } else {
                UpstreamAccess::Unknown
            }
        }
        Err(_) => UpstreamAccess::Unknown,
    }
}

/// The package name from a `name@version` package id, tolerating a
/// leading scope `@` (`@scope/foo@1.0.0` → `@scope/foo`).
fn package_name(pkg_id: &str) -> Option<&str> {
    let at = pkg_id.rfind('@')?;
    (at > 0).then_some(&pkg_id[..at])
}

/// gzip level for the install response body. Level 6 (the gzip default)
/// shrinks the payload ~16% over level 1 — the win that matters once the
/// server is across a latency link, where fewer bytes means fewer TCP
/// slow-start round trips — while level 9 adds under a percent for several
/// times the CPU.
const FILES_GZIP_LEVEL: u32 = 6;

/// Content type of the install response: a length-prefixed JSON header
/// followed by the [`build_files_payload`] binary frames, gzip-compressed.
const INLINE_CONTENT_TYPE: &str = "application/x-pnpr-install-inline";

/// Build the single-response body: the lockfile, stats, and store-index
/// entries in a length-prefixed JSON header, followed by the contents of
/// the files the client is missing as binary frames — so the client
/// materializes everything from one round trip.
fn inline_response(
    runtime: &InstallAccelerator,
    lockfile: &Lockfile,
    stats_json: &serde_json::Value,
    result: &diff::DiffResult,
) -> Response {
    let index_entries: Vec<serde_json::Value> = result
        .package_index
        .iter()
        .map(|entry| {
            serde_json::json!({
                "key": format!("{}\t{}", entry.integrity, entry.pkg_id),
                "b64": BASE64.encode(&entry.raw),
            })
        })
        .collect();
    let header = serde_json::json!({
        "lockfile": serde_json::to_value(lockfile).unwrap_or(serde_json::Value::Null),
        "stats": stats_json,
        "indexEntries": index_entries,
    });

    let files = result.missing_files.iter().map(|file| (file.digest.as_str(), file.executable));
    let files_payload = match build_files_payload(&runtime.store_dir, files) {
        Ok(payload) => payload,
        Err((status, message)) => return json_error(status, &message),
    };

    finish_inline_response(&header, &files_payload)
}

/// Frame a JSON `header` and an already-built [`build_files_payload`]
/// byte buffer into one length-prefixed, gzip-compressed body.
fn finish_inline_response(header: &serde_json::Value, files_payload: &[u8]) -> Response {
    let header_bytes = serde_json::to_vec(header).unwrap_or_else(|_| b"{}".to_vec());
    let Ok(header_len) = u32::try_from(header_bytes.len()) else {
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, "install header too large");
    };
    let mut body = Vec::with_capacity(4 + header_bytes.len() + files_payload.len());
    body.extend_from_slice(&header_len.to_be_bytes());
    body.extend_from_slice(&header_bytes);
    body.extend_from_slice(files_payload);

    let mut encoder = GzEncoder::new(Vec::new(), Compression::new(FILES_GZIP_LEVEL));
    if encoder.write_all(&body).is_err() {
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, "gzip failed");
    }
    let gzipped = match encoder.finish() {
        Ok(gzipped) => gzipped,
        Err(_) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "gzip failed"),
    };

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, INLINE_CONTENT_TYPE)
        .header(header::CONTENT_ENCODING, "gzip")
        .body(Body::from(gzipped))
        .expect("binary response is always valid")
}

/// Why [`verify_input_lockfile`] failed: either the lockfile violated
/// the client's policy (carry the rendered violations so the caller can
/// shape them for the client's protocol) or the verifiers couldn't be
/// built at all (a ready-made error response).
enum VerifyFailure {
    Violations(Vec<serde_json::Value>),
    Internal(Response),
}

/// Verify the client's input lockfile under the client's policy. On a
/// clean pass returns `Ok(())`; on a policy violation returns the
/// rendered violations so the caller can deliver them in whichever
/// protocol the client asked for (NDJSON `E` line or inline header). A
/// build-verifiers failure (e.g. an invalid exclude pattern) returns a
/// ready-made 500.
async fn verify_input_lockfile(
    runtime: &InstallAccelerator,
    config: &'static PacquetConfig,
    auth_headers: &Arc<AuthHeaders>,
    lockfile: &Lockfile,
) -> Result<(), VerifyFailure> {
    // A fresh per-request packument cache shared with the verifier; the
    // on-disk metadata mirror under `<cache_dir>/v11/metadata-full` is
    // warm across requests and is the real verification cache.
    let meta_cache = Arc::new(InMemoryPackageMetaCache::default());
    let verifiers = build_resolution_verifiers(
        config,
        Arc::clone(&runtime.client),
        Some(meta_cache as Arc<dyn PackageMetaCache>),
        Some(Arc::clone(auth_headers)),
    )
    .map_err(|err| {
        VerifyFailure::Internal(json_error(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()))
    })?;

    // Whole-lockfile verdict cache: an O(1) hit when this exact lockfile
    // already passed under a policy we still trust skips the whole fan-out
    // (the dominant win for a shared pnpr — CI re-runs, a fleet building
    // the same repo).
    let hash = hash_lockfile(lockfile);
    if let Some(cache) = runtime.verdict_cache.as_ref()
        && cache.is_verified(&hash, |policy| {
            verifiers.iter().all(|verifier| verifier.can_trust_past_check(policy))
        })
    {
        return Ok(());
    }

    let violations = collect_resolution_policy_violations(lockfile, &verifiers, None).await;
    if violations.is_empty() {
        if let Some(cache) = runtime.verdict_cache.as_ref() {
            cache.record(&hash, &merge_policies(&verifiers));
        }
        return Ok(());
    }

    let rendered: Vec<serde_json::Value> = violations
        .iter()
        .map(|violation| {
            serde_json::json!({
                "name": violation.name.to_string(),
                "version": violation.version,
                "code": violation.code,
                "reason": violation.reason,
            })
        })
        .collect();
    Err(VerifyFailure::Violations(rendered))
}

/// Render input-lockfile policy violations into the inline response
/// header (`{ "violations": [...] }`, no files following) so the client
/// rebuilds the identical `VerifyError` and aborts the same way the local
/// gate would.
fn violation_response(violations: &[serde_json::Value]) -> Response {
    let header = serde_json::json!({ "violations": violations });
    // No files follow a verification failure: just the end-of-stream
    // marker so the client's frame parser terminates cleanly.
    let files_payload = empty_files_payload();
    finish_inline_response(&header, &files_payload)
}

/// Merge every active verifier's policy snapshot into one bag, the key
/// the verdict cache stores alongside the lockfile hash. Later verifiers
/// overwrite earlier ones on a shared key — mirrors the local cache's
/// `merge_policies` so a verdict recorded here is comparable to one the
/// client's own cache would write.
fn merge_policies(
    verifiers: &[Arc<dyn ResolutionVerifier>],
) -> serde_json::Map<String, serde_json::Value> {
    let mut merged = serde_json::Map::new();
    for verifier in verifiers {
        for (key, value) in verifier.policy() {
            merged.insert(key.clone(), value.clone());
        }
    }
    merged
}

/// The binary file frames the install response embeds: a 2-byte `{}` JSON
/// header (length-prefixed) followed by one
/// `[64-byte digest][u32 size][1-byte exec][content]` frame per file,
/// terminated by 64 zero bytes. Reads each file's content from the store
/// by digest; an `Err` is a ready-made error response.
fn build_files_payload<'a>(
    store_dir: &StoreDir,
    files: impl Iterator<Item = (&'a str, bool)>,
) -> Result<Vec<u8>, (StatusCode, String)> {
    let mut payload = empty_files_payload_prefix();
    for (digest, executable) in files {
        let mode = if executable { 0o755 } else { 0o644 };
        let Some(path) = store_dir.cas_file_path_by_mode(digest, mode) else {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                "could not resolve file path".to_string(),
            ));
        };
        let content = match std::fs::read(&path) {
            Ok(content) => content,
            Err(err) => {
                return Err((StatusCode::INTERNAL_SERVER_ERROR, format!("{digest}: {err}")));
            }
        };
        let Some(digest_bytes) = hex_to_bytes(digest) else {
            return Err((StatusCode::BAD_REQUEST, "invalid digest".to_string()));
        };
        // The wire framing encodes the size as a u32; a >4 GiB file would
        // truncate. npm files never approach this, but fail cleanly rather
        // than corrupt the stream.
        let Ok(content_len) = u32::try_from(content.len()) else {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("{digest}: file too large for the protocol"),
            ));
        };
        payload.extend_from_slice(&digest_bytes);
        payload.extend_from_slice(&content_len.to_be_bytes());
        payload.push(u8::from(executable));
        payload.extend_from_slice(&content);
    }
    payload.extend_from_slice(&[0u8; 64]);
    Ok(payload)
}

/// The leading 2-byte `{}` JSON header every files payload starts with.
fn empty_files_payload_prefix() -> Vec<u8> {
    let mut prefix = Vec::new();
    prefix.extend_from_slice(&2u32.to_be_bytes());
    prefix.extend_from_slice(b"{}");
    prefix
}

/// A files payload carrying no files — the header prefix plus the
/// end-of-stream marker. Used when an `inlineFiles` response has only
/// metadata (a `--lockfile-only` resolve or a verification failure).
fn empty_files_payload() -> Vec<u8> {
    let mut payload = empty_files_payload_prefix();
    payload.extend_from_slice(&[0u8; 64]);
    payload
}

fn json_error(status: StatusCode, message: &str) -> Response {
    let body = serde_json::json!({ "error": message }).to_string();
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body))
        .expect("static json error response is always valid")
}

/// Decode a 64-byte (128 hex char) digest into raw bytes. Returns
/// `None` on a malformed length or non-hex byte.
fn hex_to_bytes(hex: &str) -> Option<[u8; 64]> {
    if hex.len() != 128 {
        return None;
    }
    let bytes = hex.as_bytes();
    let mut out = [0u8; 64];
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = (hex_val(bytes[2 * i])? << 4) | hex_val(bytes[2 * i + 1])?;
    }
    Some(out)
}

fn hex_val(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}
