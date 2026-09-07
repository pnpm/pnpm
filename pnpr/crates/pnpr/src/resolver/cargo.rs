//! Cargo resolution for the install accelerator.
//!
//! `POST /-/pnpr/v0/resolve` with `"ecosystem": "cargo"` resolves a Cargo
//! workspace the way the npm ecosystem is resolved one module over: the
//! client sends what only it has (its `cargo metadata` output and the
//! sparse index it resolves against), and the server does the part that
//! costs round trips — walking the index, wave by wave, until every
//! candidate crate's metadata is in hand — then runs pacquet's Cargo
//! resolver and returns the rendered `Cargo.lock`.
//!
//! Index files are cached under the server's cache directory, namespaced
//! by registry and by the route scope the caller's identity and the crate
//! resolve to, so one caller's private index never satisfies another
//! caller's fetch. A crate whose index is not cached is fetched once
//! across concurrent resolves.
//!
//! There are no per-package frames: Cargo resolution is a single pubgrub
//! solve rather than an incrementally-yielding tree walk, so nothing is
//! known before everything is. The response carries the terminal `done`
//! frame alone, and the client fetches `.crate` archives from the
//! lockfile as it does after a local resolve.
//!
//! The verification policies the npm surface applies — `minimumReleaseAge`,
//! `trustPolicy`, the OSV advisory index — are npm advisory data and do not
//! reach this path; a Cargo resolve is held to the same rules a local one
//! is.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime},
};

use axum::{http::StatusCode, response::Response};
use futures_util::{StreamExt, TryStreamExt, stream};
use pnpm_network::{AuthHeaders, MetadataCacheScope, RetryOpts, ThrottledClient};

use pnpr_policy::Identity;
use pnpr_route::{Footprint, RouteContext, RouteHook, url_has_inline_credentials};

use crate::server::StripedLocks;

use super::{
    Resolver, json_error,
    package_route::PackageRoute,
    protocol::CargoResolveRequest,
    report_message,
    request_validation::forbidden_off_allowlist,
    wire::{cargo_done_frame, error_frame, ndjson_single_frame},
};

/// How many index files one resolve may fetch. A crate graph reaches a few
/// thousand index files at the very top of the scale; well past that the
/// request is either pathological or hostile, and each entry is a fetch the
/// server pays for.
const MAX_INDEX_FILES: usize = 20_000;

/// How many fetch waves one resolve may run. Each wave resolves one more
/// level of the dependency graph, so a real workspace finishes in tens of
/// waves; the cap only stops a loop that stops making progress.
const MAX_INDEX_WAVES: usize = 256;

/// Cap on a single index file. The largest crates.io entries are a couple
/// of megabytes, so this bounds a hostile registry's reply without
/// truncating a real one.
const MAX_INDEX_FILE_BYTES: usize = 16 * 1024 * 1024;

/// Cap on the index bytes one resolve holds. [`MAX_INDEX_FILES`] and
/// [`MAX_INDEX_FILE_BYTES`] bound the count and each entry, whose product
/// is far more memory than a server has: the whole index of a real
/// workspace is a few hundred megabytes at the very top of the scale, and
/// past this budget a registry is feeding the resolver rather than
/// answering it.
///
/// An entry is charged when it lands, and no fetch starts once the budget
/// is spent, so a wave already in flight overshoots by at most
/// [`INDEX_FETCH_CONCURRENCY`] × [`MAX_INDEX_FILE_BYTES`]. Charging a
/// reservation up front instead would bound the peak exactly but refuse
/// real workspaces, whose entries are three orders of magnitude smaller
/// than the per-entry cap.
const MAX_INDEX_TOTAL_BYTES: usize = 256 * 1024 * 1024;

/// Index files fetched in parallel within one wave.
const INDEX_FETCH_CONCURRENCY: usize = 16;

/// Handle a `"ecosystem": "cargo"` resolve request: fetch the index files
/// the workspace needs, resolve, and answer with the `Cargo.lock` the
/// client writes verbatim.
pub(super) async fn handle_resolve(
    runtime: &Resolver,
    identity: Identity,
    body: &[u8],
) -> Response {
    let request: CargoResolveRequest = match serde_json::from_slice(body) {
        Ok(request) => request,
        Err(err) => return json_error(StatusCode::BAD_REQUEST, &err.to_string()),
    };
    let registry = request
        .registry
        .as_deref()
        .unwrap_or(pnpm_cargo_resolver::CRATES_IO_SPARSE_INDEX)
        .trim_end_matches('/')
        .to_string();
    if url_has_inline_credentials(&registry) {
        return json_error(
            StatusCode::BAD_REQUEST,
            "inline URL credentials (user:pass@host) are not allowed; \
             configure an upstream credential alias instead",
        );
    }
    if !runtime.route_context.allows_registry(&registry) {
        return forbidden_off_allowlist(&registry);
    }

    let index = IndexFetcher {
        client: Arc::clone(&runtime.client),
        route: Arc::clone(&runtime.route_context),
        identity,
        // Auth comes from this server's route policy for the caller, never
        // from the request — the same rule the npm surface follows, so a
        // caller cannot borrow the server's reach by describing a registry
        // it has no credential for.
        footprint: Arc::new(Mutex::new(Footprint::default())),
        secret: Arc::clone(&runtime.resolution_cache_secret),
        locks: Arc::clone(&runtime.cargo_index_locks),
        cache_dir: runtime.cargo_index_cache_dir(&registry),
        ttl: runtime.cargo_index_ttl,
        bytes_held: AtomicUsize::new(0),
        registry,
    };

    let metadata = request.metadata;
    let source = pnpm_cargo_resolver::registry_source(&index.registry);
    let index_files = match index.fetch_for(&metadata).await {
        Ok(index_files) => index_files,
        Err(err) => return ndjson_single_frame(&error_frame(&err)),
    };
    // pubgrub's solve is CPU-bound and can run for a while on a large
    // workspace, so it stays off the async runtime's worker threads.
    let lockfile = tokio::task::spawn_blocking(move || {
        pnpm_cargo_resolver::resolve_lockfile(&metadata, &index_files, &source)
    })
    .await;
    match lockfile {
        Ok(Ok(lockfile)) => ndjson_single_frame(&cargo_done_frame(&lockfile)),
        Ok(Err(err)) => ndjson_single_frame(&error_frame(&report_message(&err))),
        Err(err) => ndjson_single_frame(&error_frame(&err.to_string())),
    }
}

/// The failure a resolve earns once the index bytes it holds pass
/// [`MAX_INDEX_TOTAL_BYTES`], naming the crate the budget ran out on.
fn over_index_budget(held: usize, name: &str) -> Option<String> {
    (held > MAX_INDEX_TOTAL_BYTES).then(|| index_budget_exhausted(name))
}

/// Whether a resolve holding `held` bytes has room for another entry. An
/// entry that lands exactly on [`MAX_INDEX_TOTAL_BYTES`] is kept, but it
/// leaves no room for the next one.
fn index_budget_has_room(held: usize) -> bool {
    held < MAX_INDEX_TOTAL_BYTES
}

fn index_budget_exhausted(name: &str) -> String {
    format!(
        "resolving this workspace needs more than {MAX_INDEX_TOTAL_BYTES} bytes of \
         sparse-index metadata (reached at {name})",
    )
}

/// Reads a sparse index for one resolve: cache first, then the registry.
struct IndexFetcher {
    client: Arc<ThrottledClient>,
    route: Arc<RouteContext>,
    identity: Identity,
    /// The private routes this resolve's fetches touched, recorded by the
    /// route hook as it selects each credential.
    footprint: Arc<Mutex<Footprint>>,
    /// HMAC secret keying a private route's cache namespace.
    secret: Arc<[u8]>,
    /// Serializes the fetch of one crate's index file, per cache
    /// namespace, across concurrent resolves — so a cold graph is fetched
    /// once rather than once per caller.
    locks: Arc<StripedLocks>,
    /// Where this registry's index files are cached, already namespaced by
    /// registry origin. The route scope adds the last segment; see
    /// [`Self::cache_path`].
    cache_dir: PathBuf,
    ttl: Duration,
    /// Index bytes this resolve is holding, against
    /// [`MAX_INDEX_TOTAL_BYTES`].
    bytes_held: AtomicUsize,
    /// The sparse index base URL, without its trailing slash.
    registry: String,
}

impl IndexFetcher {
    /// Every index file `metadata`'s dependency graph reaches, fetched in
    /// waves: each wave asks the resolver which names are still missing,
    /// fetches those, and repeats until nothing is missing.
    async fn fetch_for(&self, metadata: &str) -> Result<BTreeMap<String, String>, String> {
        let mut index_files = BTreeMap::new();
        let source = pnpm_cargo_resolver::registry_source(&self.registry);
        for _ in 0..MAX_INDEX_WAVES {
            let missing = pnpm_cargo_resolver::missing_index_names(metadata, &index_files, &source)
                .map_err(|err| report_message(&err))?;
            if missing.is_empty() {
                return Ok(index_files);
            }
            if index_files.len() + missing.len() > MAX_INDEX_FILES {
                return Err(format!(
                    "resolving this workspace needs more than {MAX_INDEX_FILES} sparse-index files",
                ));
            }
            let fetched = stream::iter(missing)
                .map(|name| async move {
                    let contents = self.index_file(&name).await?;
                    Ok::<_, String>((name, contents))
                })
                .buffer_unordered(INDEX_FETCH_CONCURRENCY)
                .try_collect::<Vec<_>>()
                .await?;
            index_files.extend(fetched);
        }
        Err(format!("Cargo sparse-index discovery did not settle within {MAX_INDEX_WAVES} waves"))
    }

    /// One crate's index file, from the cache when it is still fresh.
    ///
    /// A miss is taken under the stripe of the entry's cache path: whoever
    /// holds it fetches and caches the entry while the rest wait and read
    /// what it stored.
    async fn index_file(&self, name: &str) -> Result<String, String> {
        let canonical_name =
            pnpr_package_name::CanonicalPackageName::parse(name, pnpr_registry::Ecosystem::Cargo)
                .map_err(|err| err.to_string())?;
        let relative_path = pnpr_cargo::sparse_index_path(name);
        let url = format!("{}/{relative_path}", self.registry);
        // The route policy classifies a fetch by the crate it is for, as the
        // Cargo registry surface does, so an upstream's per-crate rules
        // decide the credential and the cache namespace here too. Both
        // surfaces match rules against the lowercased crate name.
        let canonical_name = canonical_name.as_str().to_string();
        let auth = self.auth_for(&canonical_name);
        let cache_path = self.cache_path(&auth, &url, &relative_path);
        if let Some(cached) = self.cached(&cache_path).await {
            return self.hold(name, cached);
        }
        // Keyed by the cache path, not the URL: it carries the route scope
        // that decides which callers can read each other's entry, so two
        // callers on different private scopes fetch in parallel rather than
        // queueing for a result neither could reuse.
        let _fetching = self.locks.lock(&cache_path.to_string_lossy()).await;
        if let Some(cached) = self.cached(&cache_path).await {
            return self.hold(name, cached);
        }
        // Nothing more is fetched once the budget is spent, so the entries
        // still in flight bound how far past it the resolve can reach.
        if !index_budget_has_room(self.bytes_held.load(Ordering::Relaxed)) {
            return Err(index_budget_exhausted(name));
        }
        // The route policy decides what this deployment may reach at all;
        // a registry a caller merely names is refused here rather than
        // fetched (SSRF boundary).
        if !auth.allows_fetch(&url) {
            return Err(format!(
                "{url:?} is not allowed by this pnpr server; the operator must declare its \
                 registry as a public route or an upstream",
            ));
        }
        let response = self
            .client
            .get_limited_bytes_with_secure_auth_and_retry(
                &url,
                &auth,
                None,
                RetryOpts::default(),
                MAX_INDEX_FILE_BYTES,
            )
            .await
            .map_err(|err| format!("fetch sparse index entry for {name}: {err}"))?;
        if response.body_truncated {
            return Err(format!(
                "sparse index entry for {name} exceeds {MAX_INDEX_FILE_BYTES} bytes",
            ));
        }
        if !response.status.is_success() {
            return Err(format!(
                "fetch sparse index entry for {name} returned HTTP {}",
                response.status,
            ));
        }
        let contents = String::from_utf8(response.body)
            .map_err(|err| format!("decode sparse index entry for {name}: {err}"))?;
        // Charged before it is cached, so an entry that spends the last of
        // the budget is not left behind for the next resolve to read.
        let contents = self.hold(name, contents)?;
        Self::store(cache_path, contents.clone()).await;
        Ok(contents)
    }

    /// Account `contents` against this resolve's index-byte budget, which
    /// bounds what one request can make the server hold in memory and write
    /// to its cache.
    fn hold(&self, name: &str, contents: String) -> Result<String, String> {
        let held = self.bytes_held.fetch_add(contents.len(), Ordering::Relaxed) + contents.len();
        match over_index_budget(held, name) {
            Some(err) => Err(err),
            None => Ok(contents),
        }
    }

    /// The request auth for a fetch about `canonical_name`: this server's
    /// route policy for the caller, with the crate bound in so the
    /// package-blind fetch helpers still classify by it.
    fn auth_for(&self, canonical_name: &str) -> AuthHeaders {
        let hook = RouteHook::new(
            Arc::clone(&self.route),
            self.identity.clone(),
            Arc::clone(&self.footprint),
            Arc::clone(&self.secret),
        );
        AuthHeaders::default()
            .with_route_hook(Arc::new(PackageRoute::new(hook, canonical_name.to_string())))
    }

    /// Where `url`'s index file is cached. The route scope keys the
    /// namespace, so a private index cached under one caller's credential is
    /// never read back for a caller who does not reproduce that scope.
    fn cache_path(&self, auth: &AuthHeaders, url: &str, relative_path: &str) -> PathBuf {
        let scope = match auth.metadata_scope(url, None) {
            MetadataCacheScope::Public => "public".to_string(),
            MetadataCacheScope::Private { descriptor_id } => descriptor_id,
        };
        self.cache_dir.join(scope).join(relative_path)
    }

    /// The cached index file when it is younger than the TTL. Every failure
    /// (absent, unreadable, stale) is a miss: the registry is the source of
    /// truth and refetching is always correct.
    async fn cached(&self, path: &Path) -> Option<String> {
        let metadata = tokio::fs::metadata(path).await.ok()?;
        let age = SystemTime::now().duration_since(metadata.modified().ok()?).ok()?;
        if age >= self.ttl {
            return None;
        }
        tokio::fs::read_to_string(path).await.ok()
    }

    /// Cache an index file, best effort: a cache that cannot be written
    /// costs a refetch on the next resolve, which is not worth failing over.
    /// The write is atomic (and therefore blocking), so it runs off the
    /// runtime's worker threads.
    async fn store(path: PathBuf, contents: String) {
        let _ = tokio::task::spawn_blocking(move || {
            let parent = path.parent()?;
            std::fs::create_dir_all(parent).ok()?;
            pnpm_fs::write_atomic(&path, contents.as_bytes()).ok()
        })
        .await;
    }
}

#[cfg(test)]
mod tests;
