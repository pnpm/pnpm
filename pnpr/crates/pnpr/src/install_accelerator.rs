//! pnpr install accelerator: server-side dependency resolution exposed as
//! an additive, opt-in protocol alongside pnpr's npm-compatible API. The
//! handshake + endpoint are served under one base URL (the `pnprServer`).
//!
//! Two routes, built on pacquet's resolver:
//!
//! * `GET /-/pnpr` — capability handshake; advertises the supported
//!   protocol versions so a client can negotiate or fail fast.
//! * `POST /v1/install` — resolve a project **against the registries
//!   the client sends** (so the server uses the same source of truth as
//!   the client), verify the client's input lockfile under the client's
//!   policy, and return the resolved lockfile as a gzipped JSON body.
//!   The client then fetches tarballs in parallel from the registries
//!   like a normal install
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
    io::Write as _,
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock},
};

use crate::config::Config as RegistryConfig;

use axum::{
    body::{Body, Bytes},
    http::{StatusCode, header},
    response::Response,
};
use flate2::{Compression, write::GzEncoder};
use indexmap::IndexMap;
use pacquet_config::Config as PacquetConfig;
use pacquet_lockfile::Lockfile;
use pacquet_lockfile_verification::{collect_resolution_policy_violations, hash_lockfile};
use pacquet_network::{AuthHeaders, ThrottledClient};
use pacquet_package_manager::build_resolution_verifiers;
use pacquet_resolving_npm_resolver::{InMemoryPackageMetaCache, PackageMetaCache};
use pacquet_resolving_resolver_base::ResolutionVerifier;
use pacquet_store_dir::StoreDir;

use self::{protocol::InstallRequest, verdict_cache::VerdictCache};

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
}

impl InstallAccelerator {
    pub(crate) fn get_or_init<'a>(
        cell: &'a OnceLock<InstallAccelerator>,
        config: &RegistryConfig,
    ) -> &'a InstallAccelerator {
        cell.get_or_init(|| InstallAccelerator::build(config))
    }

    fn build(config: &RegistryConfig) -> InstallAccelerator {
        let store_dir = config.cache_storage.join("pnpr-store");
        let cache_dir = config.cache_storage.join("pnpr-cache");
        // Best-effort: a real failure here (e.g. a permission problem)
        // resurfaces with a precise error on the first store/cache write
        // during resolution, so there's nothing actionable to report yet.
        let _ = std::fs::create_dir_all(&store_dir);
        let _ = std::fs::create_dir_all(&cache_dir);
        let verdict_cache = VerdictCache::open(&cache_dir.join("lockfile-verdicts.sqlite")).ok();
        InstallAccelerator {
            store_dir: StoreDir::new(store_dir),
            cache_dir,
            client: Arc::new(ThrottledClient::new_for_installs()),
            configs: Mutex::new(HashMap::new()),
            verdict_cache,
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

/// Handle `POST /v1/install`: verify the client's input lockfile under
/// the client's policy, resolve against the client's registries, and
/// return the resolved lockfile. No tarball leaves the server — the
/// client fetches them itself.
pub(crate) async fn handle_install(runtime: &InstallAccelerator, body: Bytes) -> Response {
    let request: InstallRequest = match serde_json::from_slice(&body) {
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

    resolve_response(&lockfile)
}

/// gzip level for the response body. Level 6 (the gzip default) shrinks
/// the JSON lockfile ~16% over level 1 — the win that matters once the
/// server is across a latency link, where fewer bytes means fewer TCP
/// slow-start round trips — while level 9 adds under a percent for several
/// times the CPU.
const GZIP_LEVEL: u32 = 6;

/// Build the install response: the resolved lockfile and stats as a
/// gzipped JSON object. The client writes the lockfile, then fetches
/// every tarball itself.
fn resolve_response(lockfile: &Lockfile) -> Response {
    let total_packages = lockfile.packages.as_ref().map_or(0, |packages| packages.len());
    let header = serde_json::json!({
        "lockfile": serde_json::to_value(lockfile).unwrap_or(serde_json::Value::Null),
        "stats": { "totalPackages": total_packages },
    });
    json_gzip_response(&header)
}

/// Render input-lockfile policy violations into the response body
/// (`{ "violations": [...] }`) so the client rebuilds the identical
/// `VerifyError` and aborts the same way the local gate would.
fn violation_response(violations: &[serde_json::Value]) -> Response {
    json_gzip_response(&serde_json::json!({ "violations": violations }))
}

/// Serialize `value` to JSON and gzip it into a `200` response body.
fn json_gzip_response(value: &serde_json::Value) -> Response {
    let body = serde_json::to_vec(value).unwrap_or_else(|_| b"{}".to_vec());
    let mut encoder = GzEncoder::new(Vec::new(), Compression::new(GZIP_LEVEL));
    if encoder.write_all(&body).is_err() {
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, "gzip failed");
    }
    let gzipped = match encoder.finish() {
        Ok(gzipped) => gzipped,
        Err(_) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "gzip failed"),
    };

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/json")
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
/// rendered violations so the caller can deliver them to the client. A
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
