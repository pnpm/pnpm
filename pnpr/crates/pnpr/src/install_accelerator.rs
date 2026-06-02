//! pnpr install accelerator: server-side dependency resolution plus file-level
//! store deduplication, exposed as an additive, opt-in protocol
//! alongside pnpr's npm-compatible API. The handshake + endpoints are
//! served under one base URL (the `pnprServer`).
//!
//! Three routes, built on pacquet's resolver and content-addressable
//! store:
//!
//! * `GET /-/pnpr` — capability handshake; advertises the supported
//!   protocol versions so a client can negotiate or fail fast.
//! * `POST /v1/install` — resolve a project **against the registries
//!   the client sends** (so the server uses the same source of truth as
//!   the client), then stream an NDJSON response: `D` lines (file
//!   digests the client is missing), `I` lines (pre-packed store-index
//!   entries), a final `L` line with the lockfile and stats, or an `E`
//!   line on a mid-stream error.
//! * `POST /v1/files` — serve a batch of files by digest as a gzip
//!   binary stream the client writes straight into its CAFS.
//!
//! The client's `registry`, `namedRegistries`, `overrides`, and the
//! verification policy (`minimumReleaseAge`, `trustPolicy`, ...) drive
//! resolution and verification. When the client sends its on-disk
//! lockfile, the server verifies it under the client's policy before
//! resolving, then reuses it as the resolution seed (frozen → as-is;
//! non-frozen → reuse-and-update). A multi-project workspace is resolved
//! by reconstructing the workspace on disk (root manifest +
//! `pnpm-workspace.yaml` + member manifests) and letting pacquet's
//! install path discover and resolve every importer. **Deferred:**
//! auth/credential forwarding (so private registries resolve). Responses
//! are buffered rather than truly streamed.

mod diff;
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
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use flate2::{Compression, write::GzEncoder};
use indexmap::IndexMap;
use pacquet_config::Config as PacquetConfig;
use pacquet_lockfile::Lockfile;
use pacquet_lockfile_verification::{collect_resolution_policy_violations, hash_lockfile};
use pacquet_network::ThrottledClient;
use pacquet_package_manager::build_resolution_verifiers;
use pacquet_resolving_npm_resolver::{InMemoryPackageMetaCache, PackageMetaCache};
use pacquet_resolving_resolver_base::ResolutionVerifier;
use pacquet_store_dir::{StoreDir, StoreIndex};

use self::{
    protocol::{FilesRequest, InstallRequest, is_valid_sha512_hex},
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

/// Handle `POST /v1/install`.
pub(crate) async fn handle_install(runtime: &InstallAccelerator, body: Bytes) -> Response {
    let request: InstallRequest = match serde_json::from_slice(&body) {
        Ok(request) => request,
        Err(err) => return json_error(StatusCode::BAD_REQUEST, &err.to_string()),
    };

    // Resolve against the client's registries, not the server's own.
    let config = runtime.config_for(&request);

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
        && let Err(response) = verify_input_lockfile(runtime, config, input_lockfile).await
    {
        return response;
    }

    let lockfile = match resolve::resolve(config, &runtime.client, &request).await {
        Ok(lockfile) => lockfile,
        Err(err) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
    };

    let packages = resolve::collect_packages(&lockfile, &config.registry);

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
        if let Err(err) = resolve::fetch_uncached(config, &runtime.client, &packages).await {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string());
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

    let mut ndjson: Vec<u8> = Vec::new();
    for file in &result.missing_files {
        let _ =
            writeln!(ndjson, "D\t{}\t{}\t{}", file.digest, file.size, u8::from(file.executable));
    }
    for entry in &result.package_index {
        let _ = writeln!(
            ndjson,
            "I\t{}\t{}\t{}",
            entry.integrity,
            entry.pkg_id,
            BASE64.encode(&entry.raw),
        );
    }

    let stats = &result.stats;
    let payload = serde_json::json!({
        "lockfile": serde_json::to_value(&lockfile).unwrap_or(serde_json::Value::Null),
        "stats": {
            "totalPackages": stats.total_packages,
            "alreadyInStore": stats.already_in_store,
            "packagesToFetch": stats.packages_to_fetch,
            "filesInNewPackages": stats.files_in_new_packages,
            "filesAlreadyInCafs": stats.files_already_in_cafs,
            "filesToDownload": stats.files_to_download,
            "downloadBytes": stats.download_bytes,
        },
    });
    let _ = writeln!(ndjson, "L\t{}", payload);

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/x-ndjson")
        .body(Body::from(ndjson))
        .expect("static ndjson response is always valid")
}

/// Verify the client's input lockfile under the client's policy. On a
/// clean pass returns `Ok(())`; on a policy violation returns `Err` with
/// a 200 NDJSON response carrying a single `E` line of rendered
/// violations, so the client rebuilds the identical `VerifyError` and
/// aborts the same way the local gate would. A build-verifiers failure
/// (e.g. an invalid exclude pattern) returns a 500.
async fn verify_input_lockfile(
    runtime: &InstallAccelerator,
    config: &'static PacquetConfig,
    lockfile: &Lockfile,
) -> Result<(), Response> {
    // A fresh per-request packument cache shared with the verifier; the
    // on-disk metadata mirror under `<cache_dir>/v11/metadata-full` is
    // warm across requests and is the real verification cache.
    let meta_cache = Arc::new(InMemoryPackageMetaCache::default());
    let verifiers = build_resolution_verifiers(
        config,
        Arc::clone(&runtime.client),
        Some(meta_cache as Arc<dyn PackageMetaCache>),
    )
    .map_err(|err| json_error(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()))?;

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
    let payload = serde_json::json!({ "violations": rendered });
    let body = format!("E\t{payload}\n");
    Err(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/x-ndjson")
        .body(Body::from(body))
        .expect("static ndjson violation response is always valid"))
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

/// Handle `POST /v1/files`.
pub(crate) async fn handle_files(runtime: &InstallAccelerator, body: Bytes) -> Response {
    let request: FilesRequest = match serde_json::from_slice(&body) {
        Ok(request) => request,
        Err(err) => return json_error(StatusCode::BAD_REQUEST, &err.to_string()),
    };

    for (index, file) in request.digests.iter().enumerate() {
        if !is_valid_sha512_hex(&file.digest) {
            return json_error(
                StatusCode::BAD_REQUEST,
                &format!("digests[{index}].digest must be a valid sha512 hex string"),
            );
        }
    }

    let store_dir = &runtime.store_dir;

    // Build the binary payload up front so a missing file surfaces as a
    // clean 500 before any bytes are committed to the response.
    let mut payload: Vec<u8> = Vec::new();
    payload.extend_from_slice(&2u32.to_be_bytes());
    payload.extend_from_slice(b"{}");

    for file in &request.digests {
        let mode = if file.executable { 0o755 } else { 0o644 };
        let Some(path) = store_dir.cas_file_path_by_mode(&file.digest, mode) else {
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, "could not resolve file path");
        };
        let content = match std::fs::read(&path) {
            Ok(content) => content,
            Err(err) => {
                return json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    &format!("{}: {err}", file.digest),
                );
            }
        };
        let Some(digest_bytes) = hex_to_bytes(&file.digest) else {
            return json_error(StatusCode::BAD_REQUEST, "invalid digest");
        };
        // The wire framing encodes the size as a u32; a >4 GiB file would
        // truncate. npm files never approach this, but fail cleanly rather
        // than corrupt the stream.
        let Ok(content_len) = u32::try_from(content.len()) else {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("{}: file too large for the protocol", file.digest),
            );
        };
        payload.extend_from_slice(&digest_bytes);
        payload.extend_from_slice(&content_len.to_be_bytes());
        payload.push(u8::from(file.executable));
        payload.extend_from_slice(&content);
    }
    payload.extend_from_slice(&[0u8; 64]);

    let mut encoder = GzEncoder::new(Vec::new(), Compression::new(1));
    if encoder.write_all(&payload).is_err() {
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, "gzip failed");
    }
    let gzipped = match encoder.finish() {
        Ok(gzipped) => gzipped,
        Err(_) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, "gzip failed"),
    };

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/x-pnpm-install")
        .header(header::CONTENT_ENCODING, "gzip")
        .body(Body::from(gzipped))
        .expect("binary response is always valid")
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
