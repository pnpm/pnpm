//! pnpr resolver: server-side dependency resolution exposed as an
//! additive, opt-in protocol alongside pnpr's npm-compatible API. The
//! handshake + endpoint are served under one base URL (the `pnprServer`).
//!
//! Two routes, built on pacquet's resolver:
//!
//! * `GET /-/pnpr` — capability handshake; advertises the supported
//!   protocol versions so a client can negotiate or fail fast.
//! * `POST /v1/resolve` — resolve a project **against the registries
//!   the client sends** (so the server uses the same source of truth as
//!   the client), verify the client's input lockfile under the client's
//!   policy, and **stream** the result back as NDJSON: one `package`
//!   frame per resolved tarball as the tree walk yields it, then a
//!   terminal `done` frame carrying the full lockfile (or an `error` /
//!   `violations` frame). The client fetches each tarball the moment its
//!   frame arrives, so download overlaps the server's resolution
//!   ([pnpm/pnpm#12234](https://github.com/pnpm/pnpm/issues/12234)),
//!   then fetches the rest in parallel like a normal install
//!   ([pnpm/pnpm#12230](https://github.com/pnpm/pnpm/issues/12230)).
//!
//! pnpr is a stateless resolver: it stores no tarballs and serves no file
//! content. The client fetches every tarball directly from the registry
//! with its own credentials, so the registry enforces access on the
//! bytes; pnpr only shapes the lockfile.
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
//! as the caller.

mod protocol;
mod resolve;
mod verdict_cache;

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};

use crate::config::Config as RegistryConfig;

use axum::{
    body::{Body, Bytes},
    http::{StatusCode, header},
    response::Response,
};
use indexmap::IndexMap;
use pacquet_config::Config as PacquetConfig;
use pacquet_lockfile::{Lockfile, LockfileResolution};
use pacquet_lockfile_verification::{collect_resolution_policy_violations, hash_lockfile};
use pacquet_network::{AuthHeaders, ThrottledClient};
use pacquet_package_manager::{
    ResolvedPackageHint, build_resolution_verifiers, tarball_url_and_integrity,
};
use pacquet_resolving_npm_resolver::{
    InMemoryPackageMetaCache, ObservedDistStats, PackageMetaCache, observed_dist_stats_sink,
};
use pacquet_resolving_resolver_base::ResolutionVerifier;
use pacquet_store_dir::StoreDir;
use sha2::{Digest, Sha256};

use self::{protocol::ResolveRequest, verdict_cache::VerdictCache};

/// Per-server engine backing the pnpr install endpoint: it holds the
/// store, cache, and HTTP client used to resolve a client's project. The
/// store and cache dirs are fixed for the server's lifetime; the
/// *registries* come from each client request (the server resolves
/// against the client's registries, not its own), so the `&'static Config`
/// the install path requires is interned per distinct client registry
/// configuration rather than leaked once or per request.
///
/// Held lazily in a [`OnceLock`] on the server's state so servers that
/// never receive such a request pay nothing, and so each server in
/// a multi-server test process keeps its own store.
pub(crate) struct Resolver {
    store_dir: StoreDir,
    cache_dir: PathBuf,
    client: Arc<ThrottledClient>,
    /// Held behind an [`Arc`] so the detached streaming-resolve task can
    /// own a clone and record its result after the response body has
    /// already started flowing to the client.
    resolution_cache: Arc<Mutex<HashMap<String, CachedResolution>>>,
    resolution_cache_ttl: Duration,
    /// One leaked `Config` per distinct client registry configuration,
    /// keyed by its canonical JSON. Bounds the leak to the number of
    /// distinct client setups the server sees (typically one).
    configs: Mutex<HashMap<String, &'static PacquetConfig>>,
    /// SQLite-backed whole-lockfile verification verdict cache. `None`
    /// only if the database couldn't be opened — verification then runs
    /// every time (uncached) rather than failing the server.
    verdict_cache: Option<VerdictCache>,
}

struct CachedResolution {
    lockfile: Lockfile,
    inserted: Instant,
}

impl Resolver {
    pub(crate) fn get_or_init<'a>(
        cell: &'a OnceLock<Resolver>,
        config: &RegistryConfig,
    ) -> &'a Resolver {
        cell.get_or_init(|| Resolver::build(config))
    }

    fn build(config: &RegistryConfig) -> Resolver {
        let store_dir = config.cache_storage.join("pnpr-store");
        let cache_dir = config.cache_storage.join("pnpr-cache");
        // Best-effort: a real failure here (e.g. a permission problem)
        // resurfaces with a precise error on the first store/cache write
        // during resolution, so there's nothing actionable to report yet.
        let _ = std::fs::create_dir_all(&store_dir);
        let _ = std::fs::create_dir_all(&cache_dir);
        let verdict_cache = VerdictCache::open(&cache_dir.join("lockfile-verdicts.sqlite")).ok();
        Resolver {
            store_dir: StoreDir::new(store_dir),
            cache_dir,
            client: Arc::new(ThrottledClient::new_for_installs()),
            resolution_cache: Arc::new(Mutex::new(HashMap::new())),
            resolution_cache_ttl: config.packument_ttl,
            configs: Mutex::new(HashMap::new()),
            verdict_cache,
        }
    }

    /// Resolve (or build + intern) the `&'static Config` for a request's
    /// registry configuration. Pacquet's install path resolves against
    /// `config.registry` / `named_registries` / `overrides`, so a request
    /// from a client with a different registry setup gets its own Config.
    fn config_for(&self, request: &ResolveRequest) -> &'static PacquetConfig {
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
        config.cache_dir.clone_from(&self.cache_dir);
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
        config.minimum_release_age_exclude.clone_from(&request.minimum_release_age_exclude);
        if let Some(ignore_missing_time) = request.minimum_release_age_ignore_missing_time {
            config.minimum_release_age_ignore_missing_time = ignore_missing_time;
        }
        config.trust_policy = request.trust_policy;
        config.trust_policy_exclude.clone_from(&request.trust_policy_exclude);
        config.trust_policy_ignore_after = request.trust_policy_ignore_after;
        let config: &'static PacquetConfig = config.leak();
        configs.insert(key, config);
        config
    }
}

/// Handle `POST /v1/resolve`: verify the client's input lockfile under
/// the client's policy, resolve against the client's registries, and
/// stream the result back as NDJSON.
///
/// The response is `application/x-ndjson`: one `package` frame per
/// resolved tarball as the server's tree walk yields it (so the client
/// fetches tarballs while the server is still resolving —
/// [pnpm/pnpm#12234](https://github.com/pnpm/pnpm/issues/12234)),
/// followed by exactly one terminal frame: `done` carrying the full
/// lockfile + stats, `error` if resolution aborts mid-stream, or
/// `violations` if the input lockfile failed the client's policy. The
/// short-circuit paths (frozen reuse, cache hit) emit only the terminal
/// `done` frame. No tarball leaves the server — the client fetches them
/// itself.
pub(crate) async fn handle_resolve(runtime: &Resolver, body: Bytes) -> Response {
    let request: ResolveRequest = match serde_json::from_slice(&body) {
        Ok(request) => request,
        Err(err) => return json_error(StatusCode::BAD_REQUEST, &err.to_string()),
    };

    // Resolve against the client's registries, not the server's own.
    let config = runtime.config_for(&request);

    // The caller's forwarded upstream credentials, threaded through
    // resolve/verify but kept out of the interned `config` so it never
    // leaks a `&'static Config` per user.
    let request_auth = Arc::new(AuthHeaders::from_map(
        request.auth_headers.iter().map(|(uri, value)| (uri.clone(), value.clone())).collect(),
    ));

    // Verify the *input* lockfile under the client's policy before any
    // package is streamed ([pnpm/pnpm#12139](https://github.com/pnpm/pnpm/issues/12139)).
    // The client skips its own `verifyLockfileResolutions` whenever a
    // pnpr server is configured, so this is the only place the
    // committed/reused entries get checked. A true first install sends
    // no lockfile — nothing to verify. `trustLockfile` is the client's
    // opt-out (mirrors the local path's `--trust-lockfile`). Freshly-
    // resolved entries are held to the same policy by the resolver's
    // pick-time gate (the policy is wired into `config`).
    let mut verified_dist_stats = None;
    if !request.trust_lockfile
        && let Some(input_lockfile) = request.lockfile.as_ref()
    {
        match verify_input_lockfile(runtime, config, &request_auth, input_lockfile).await {
            Ok(stats) => verified_dist_stats = stats,
            Err(VerifyFailure::Internal(response)) => return response,
            Err(VerifyFailure::Violations(violations)) => {
                return ndjson_single_frame(&violations_frame(&violations));
            }
        }
    }

    // Short-circuit paths that produce the whole lockfile without an
    // incremental tree walk. A verified frozen lockfile still announces
    // its tarballs as `package` frames when the verification fan-out
    // just fetched their metadata — the sizes let the client start the
    // largest downloads first. On a verdict-cache hit no metadata was
    // fetched, so there's nothing to add and the response is the bare
    // `done` frame.
    if let Some(lockfile) = resolve::fresh_frozen_input_lockfile(config, &request) {
        let mut frames = verified_dist_stats
            .map(|sizes| frozen_package_frames(config, &lockfile, &sizes))
            .unwrap_or_default();
        frames.push(done_frame(&lockfile));
        return ndjson_frames(&frames);
    }
    let resolution_cache_key = if request.auth_headers.is_empty() && request.lockfile.is_none() {
        resolution_cache_key(config, &request)
    } else {
        None
    };
    if let Some(key) = resolution_cache_key.as_ref()
        && let Some(lockfile) =
            cached_resolution(&runtime.resolution_cache, runtime.resolution_cache_ttl, key)
    {
        return ndjson_single_frame(&done_frame(&lockfile));
    }

    // Streaming resolve. Run it in a detached task that pushes one
    // `package` frame per resolved tarball into the channel via the
    // observer, then a terminal `done` / `error` frame. The response
    // body drains the channel as frames arrive.
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
    let observer: Arc<dyn pacquet_package_manager::ResolutionObserver> =
        Arc::new(StreamObserver { tx: tx.clone() });
    let client = Arc::clone(&runtime.client);
    let cache = Arc::clone(&runtime.resolution_cache);
    let cache_ttl = runtime.resolution_cache_ttl;
    tokio::spawn(async move {
        match resolve::resolve(config, &client, &request, &request_auth, Some(observer)).await {
            Ok(lockfile) => {
                if let Some(key) = resolution_cache_key {
                    store_resolution(&cache, cache_ttl, key, &lockfile);
                }
                let _ = tx.send(done_frame(&lockfile));
            }
            Err(err) => {
                let _ = tx.send(error_frame(&err.to_string()));
            }
        }
    });
    ndjson_stream_response(rx)
}

const MAX_RESOLUTION_CACHE_ENTRIES: usize = 1024;

fn cached_resolution(
    cache: &Mutex<HashMap<String, CachedResolution>>,
    ttl: Duration,
    key: &str,
) -> Option<Lockfile> {
    if ttl.is_zero() {
        return None;
    }
    let mut cache = cache.lock().expect("resolution cache poisoned");
    match cache.get(key) {
        Some(cached) if cached.inserted.elapsed() <= ttl => Some(cached.lockfile.clone()),
        Some(_) => {
            cache.remove(key);
            None
        }
        None => None,
    }
}

fn store_resolution(
    cache: &Mutex<HashMap<String, CachedResolution>>,
    ttl: Duration,
    key: String,
    lockfile: &Lockfile,
) {
    if ttl.is_zero() {
        return;
    }
    let mut cache = cache.lock().expect("resolution cache poisoned");
    if cache.len() >= MAX_RESOLUTION_CACHE_ENTRIES {
        cache.retain(|_, cached| cached.inserted.elapsed() <= ttl);
    }
    if cache.len() >= MAX_RESOLUTION_CACHE_ENTRIES
        && let Some(oldest) =
            cache.iter().min_by_key(|(_, cached)| cached.inserted).map(|(key, _)| key.clone())
    {
        cache.remove(&oldest);
    }
    cache.insert(key, CachedResolution { lockfile: lockfile.clone(), inserted: Instant::now() });
}

fn resolution_cache_key(config: &PacquetConfig, request: &ResolveRequest) -> Option<String> {
    let projects: Vec<serde_json::Value> = request
        .projects_normalized()
        .into_iter()
        .map(|project| {
            serde_json::json!({
                "dir": project.dir,
                "dependencies": project.dependencies,
                "devDependencies": project.dev_dependencies,
                "optionalDependencies": project.optional_dependencies,
            })
        })
        .collect();
    let input = serde_json::json!({
        "registry": &config.registry,
        "namedRegistries": &request.named_registries,
        "overrides": &request.overrides,
        "projects": projects,
        "lockfile": &request.lockfile,
        "frozenLockfile": request.frozen_lockfile,
        "preferFrozenLockfile": request.prefer_frozen_lockfile,
        "ignoreManifestCheck": request.ignore_manifest_check,
        "trustLockfile": request.trust_lockfile,
        "minimumReleaseAge": request.minimum_release_age,
        "minimumReleaseAgeExclude": &request.minimum_release_age_exclude,
        "minimumReleaseAgeIgnoreMissingTime": request.minimum_release_age_ignore_missing_time,
        "trustPolicy": request.trust_policy,
        "trustPolicyExclude": &request.trust_policy_exclude,
        "trustPolicyIgnoreAfter": request.trust_policy_ignore_after,
    });
    let bytes = serde_json::to_vec(&input).ok()?;
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    Some(format!("{:x}", hasher.finalize()))
}

/// NDJSON content type for the `/v1/resolve` response. One JSON object
/// per line; the client parses frames as they arrive. Excluded from the
/// server's gzip [`CompressionLayer`](crate::server) so frames flush to
/// the client incrementally rather than being buffered by the encoder.
const NDJSON_CONTENT_TYPE: &str = "application/x-ndjson";

/// [`ResolutionObserver`](pacquet_package_manager::ResolutionObserver)
/// that turns each resolved tarball into a `package` NDJSON frame and
/// pushes it down the response channel. `on_resolved` is best-effort: a
/// closed channel (client hung up) or a serialization failure drops the
/// frame silently — the resolve still runs to completion server-side.
struct StreamObserver {
    tx: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
}

impl pacquet_package_manager::ResolutionObserver for StreamObserver {
    fn on_resolved(&self, hint: pacquet_package_manager::ResolvedPackageHint<'_>) {
        if let Ok(line) = ndjson_line(&package_frame(&hint)) {
            let _ = self.tx.send(line);
        }
    }
}

/// One `package` NDJSON frame. `unpackedSize` is omitted (not null)
/// when the registry never published a `dist.unpackedSize`, so older
/// clients parse the frame unchanged.
fn package_frame(hint: &ResolvedPackageHint<'_>) -> serde_json::Value {
    let mut frame = serde_json::json!({
        "type": "package",
        "id": hint.id,
        "name": hint.name,
        "version": hint.version,
        "integrity": hint.integrity,
        "tarball": hint.tarball_url,
    });
    if let Some(size) = hint.unpacked_size {
        frame["unpackedSize"] = serde_json::Value::from(size);
    }
    if let Some(count) = hint.file_count {
        frame["fileCount"] = serde_json::Value::from(count);
    }
    frame
}

/// `package` frames for every tarball-fetchable entry of a verified
/// frozen lockfile, deduplicated by tarball URL. Mirrors what the
/// streaming resolve's [`StreamObserver`] would have announced had the
/// tree walk run: the client prefetches each tarball on arrival, with
/// `unpackedSize` (from the verification fan-out's metadata, when the
/// registry published one) prioritizing the largest downloads.
///
/// Tarball URLs are derived with the same
/// [`tarball_url_and_integrity`] the client's frozen materialization
/// uses, so the announced URLs match the client's mem-cache keys
/// byte-for-byte. Non-tarball resolutions (git, directory, binary,
/// variations) are skipped — the client fetches those through their
/// own protocol paths.
fn frozen_package_frames(
    config: &PacquetConfig,
    lockfile: &Lockfile,
    dist_stats: &ObservedDistStats,
) -> Vec<Vec<u8>> {
    let Some(packages) = lockfile.packages.as_ref() else {
        return Vec::new();
    };
    let mut seen_urls = std::collections::HashSet::new();
    let mut frames = Vec::new();
    for (package_key, snapshot) in packages {
        if !matches!(
            snapshot.resolution,
            LockfileResolution::Registry(_) | LockfileResolution::Tarball(_),
        ) {
            continue;
        }
        let Ok((tarball_url, integrity)) =
            tarball_url_and_integrity(&snapshot.resolution, package_key, config)
        else {
            continue;
        };
        if !seen_urls.insert(tarball_url.to_string()) {
            continue;
        }
        let name = package_key.name.to_string();
        let version = package_key.suffix.version().to_string();
        let id = format!("{name}@{version}");
        let integrity = integrity.to_string();
        let stats = dist_stats.get(&(name.clone(), version.clone())).map(|entry| *entry.value());
        let frame = package_frame(&ResolvedPackageHint {
            id: &id,
            name: &name,
            version: &version,
            integrity: &integrity,
            tarball_url: &tarball_url,
            unpacked_size: stats.and_then(|stats| stats.unpacked_size),
            file_count: stats.and_then(|stats| stats.file_count),
        });
        if let Ok(line) = ndjson_line(&frame) {
            frames.push(line);
        }
    }
    frames
}

/// Terminal `done` frame: the full resolved lockfile + stats. The client
/// writes the lockfile and fetches every tarball itself.
fn done_frame(lockfile: &Lockfile) -> Vec<u8> {
    let total_packages = lockfile.packages.as_ref().map_or(0, std::collections::HashMap::len);
    let frame = serde_json::json!({
        "type": "done",
        "lockfile": serde_json::to_value(lockfile).unwrap_or(serde_json::Value::Null),
        "stats": { "totalPackages": total_packages },
    });
    ndjson_line(&frame).unwrap_or_else(|_| {
        br#"{"type":"error","message":"failed to serialize lockfile"}"#.to_vec()
    })
}

/// Terminal `error` frame for a resolution that aborted mid-stream,
/// after one or more `package` frames may already have been sent (so the
/// HTTP status is locked at 200 — the failure has to ride in the body).
fn error_frame(message: &str) -> Vec<u8> {
    let frame = serde_json::json!({ "type": "error", "message": message });
    ndjson_line(&frame)
        .unwrap_or_else(|_| br#"{"type":"error","message":"resolution failed"}"#.to_vec())
}

/// Terminal `violations` frame: the input lockfile failed the client's
/// policy. Each entry mirrors the local runner's rendered violation so
/// the client rebuilds the identical `VerifyError` and aborts the same
/// way the local gate would.
fn violations_frame(violations: &[serde_json::Value]) -> Vec<u8> {
    let frame = serde_json::json!({ "type": "violations", "violations": violations });
    ndjson_line(&frame)
        .unwrap_or_else(|_| br#"{"type":"error","message":"verification failed"}"#.to_vec())
}

/// Serialize one frame to a newline-terminated NDJSON line.
fn ndjson_line(value: &serde_json::Value) -> Result<Vec<u8>, serde_json::Error> {
    let mut bytes = serde_json::to_vec(value)?;
    bytes.push(b'\n');
    Ok(bytes)
}

/// A 200 NDJSON response carrying a single, already-serialized terminal
/// frame (the short-circuit and violation paths, which never stream
/// `package` frames).
fn ndjson_single_frame(frame: &[u8]) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, NDJSON_CONTENT_TYPE)
        .body(Body::from(frame.to_vec()))
        .expect("binary response is always valid")
}

/// A 200 NDJSON response carrying several already-serialized frames in
/// one fixed body. Used by the frozen fast path, where every frame is
/// known up front — no channel to stream from.
fn ndjson_frames(frames: &[Vec<u8>]) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, NDJSON_CONTENT_TYPE)
        .body(Body::from(frames.concat()))
        .expect("binary response is always valid")
}

/// A 200 NDJSON response whose body drains the frame channel as the
/// detached resolve task produces frames. Closing the channel (the task
/// dropped its sender) ends the body.
fn ndjson_stream_response(rx: tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>) -> Response {
    let stream = futures_util::stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|line| (Ok::<_, std::io::Error>(axum::body::Bytes::from(line)), rx))
    });
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, NDJSON_CONTENT_TYPE)
        .body(Body::from_stream(stream))
        .expect("streaming response is always valid")
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
/// clean pass returns the [`ObservedDistStats`] the verifier
/// collected — `None` when the whole-lockfile verdict cache satisfied
/// the check without a fan-out (no metadata was fetched, so no sizes
/// exist). On a policy violation returns the rendered violations so
/// the caller can deliver them to the client. A build-verifiers
/// failure (e.g. an invalid exclude pattern) returns a ready-made 500.
async fn verify_input_lockfile(
    runtime: &Resolver,
    config: &'static PacquetConfig,
    auth_headers: &Arc<AuthHeaders>,
    lockfile: &Lockfile,
) -> Result<Option<ObservedDistStats>, VerifyFailure> {
    // A fresh per-request packument cache shared with the verifier; the
    // on-disk metadata mirror under `<cache_dir>/v11/metadata-full` is
    // warm across requests and is the real verification cache.
    let meta_cache = Arc::new(InMemoryPackageMetaCache::default());
    let dist_stats = observed_dist_stats_sink();
    let verifiers = build_resolution_verifiers(
        config,
        Arc::clone(&runtime.client),
        Some(meta_cache as Arc<dyn PackageMetaCache>),
        Some(Arc::clone(auth_headers)),
        Some(Arc::clone(&dist_stats)),
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
        return Ok(None);
    }

    let violations = collect_resolution_policy_violations(lockfile, &verifiers, None).await;
    if violations.is_empty() {
        if let Some(cache) = runtime.verdict_cache.as_ref() {
            cache.record(&hash, &merge_policies(&verifiers));
        }
        return Ok(Some(dist_stats));
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

fn json_error(status: StatusCode, message: &str) -> Response {
    let body = serde_json::json!({ "error": message }).to_string();
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body))
        .expect("static json error response is always valid")
}

#[cfg(test)]
mod tests;
