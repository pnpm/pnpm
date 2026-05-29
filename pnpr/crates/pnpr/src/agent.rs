//! pnpm-agent fast path: server-side dependency resolution plus
//! file-level store deduplication, exposed as an additive, opt-in
//! protocol alongside pnpr's npm-compatible API.
//!
//! Two endpoints, ported from the pnpm-agent TypeScript proof of
//! concept onto pacquet's resolver and content-addressable store:
//!
//! * `POST /v1/install` — resolve a project server-side and stream an
//!   NDJSON response: `D` lines (file digests the client is missing),
//!   `I` lines (pre-packed store-index entries), a final `L` line with
//!   the lockfile and stats, or an `E` line on a mid-stream error.
//! * `POST /v1/files` — serve a batch of files by digest as a gzip
//!   binary stream the client writes straight into its CAFS.
//!
//! **Deferred from the TypeScript agent (single default registry, no
//! per-request override):** multi-project workspaces, incremental
//! resolution from a client-supplied lockfile, `overrides`, and
//! `minimumReleaseAge`. Responses are buffered rather than truly
//! streamed; both are tracked follow-ups.

mod diff;
mod protocol;
mod resolve;

use std::{
    io::Write as _,
    path::PathBuf,
    sync::{Arc, OnceLock},
};

use crate::config::Config as RegistryConfig;

use axum::{
    body::{Body, Bytes},
    http::{StatusCode, header},
    response::Response,
};
use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
use flate2::{Compression, write::GzEncoder};
use pacquet_config::Config as PacquetConfig;
use pacquet_network::ThrottledClient;
use pacquet_store_dir::{StoreDir, StoreIndex};

use self::protocol::{FilesRequest, InstallRequest, is_valid_sha512_hex};

/// Per-server pacquet runtime: a single leaked `Config` (store, cache,
/// and registry are stable for the server's lifetime) plus a shared
/// HTTP client. Leaking once per server — rather than per request —
/// gives the install path the `&'static Config` it requires without an
/// unbounded per-request leak. Held lazily in a [`OnceLock`] on the
/// server's state so servers that never receive an agent request pay
/// nothing, and so each server in a multi-server test process gets its
/// own store rather than sharing a process-global one.
pub(crate) struct AgentRuntime {
    config: &'static PacquetConfig,
    client: Arc<ThrottledClient>,
    registry: String,
}

impl AgentRuntime {
    pub(crate) fn get_or_init<'a>(
        cell: &'a OnceLock<AgentRuntime>,
        config: &RegistryConfig,
    ) -> &'a AgentRuntime {
        cell.get_or_init(|| AgentRuntime::build(config))
    }

    fn build(config: &RegistryConfig) -> AgentRuntime {
        let registry = config
            .uplinks
            .values()
            .next()
            .map(|uplink| uplink.url.clone())
            .unwrap_or_else(|| "https://registry.npmjs.org/".to_string());
        let store_dir = config.storage.join("agent-store");
        let cache_dir = config.storage.join("agent-cache");
        let _ = std::fs::create_dir_all(&store_dir);
        let _ = std::fs::create_dir_all(&cache_dir);

        let mut pacquet_config = PacquetConfig::new();
        pacquet_config.store_dir = StoreDir::new(store_dir);
        pacquet_config.cache_dir = cache_dir;
        pacquet_config.registry = registry.clone();
        pacquet_config.modules_dir = PathBuf::from("node_modules");
        pacquet_config.lockfile = true;
        pacquet_config.verify_store_integrity = true;

        AgentRuntime {
            config: pacquet_config.leak(),
            client: Arc::new(ThrottledClient::new_for_installs()),
            registry,
        }
    }
}

/// Handle `POST /v1/install`.
pub(crate) async fn handle_install(runtime: &AgentRuntime, body: Bytes) -> Response {
    let request: InstallRequest = match serde_json::from_slice(&body) {
        Ok(request) => request,
        Err(err) => return json_error(StatusCode::BAD_REQUEST, &err.to_string()),
    };

    if request.project_count() > 1 {
        return json_error(
            StatusCode::BAD_REQUEST,
            "multi-project workspace resolution is not yet supported by the agent",
        );
    }

    let lockfile = match resolve::resolve(runtime.config, &runtime.client, &request).await {
        Ok(lockfile) => lockfile,
        Err(err) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
    };

    let packages = resolve::collect_packages(&lockfile, &runtime.registry);

    if let Err(err) = resolve::fetch_uncached(runtime.config, &runtime.client, &packages).await {
        return json_error(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string());
    }

    let store = match StoreIndex::open_readonly_in(&runtime.config.store_dir) {
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

    let result = match diff::compute_diff(&store, &diff_packages, &request.store_integrities) {
        Ok(result) => result,
        Err(err) => return json_error(StatusCode::INTERNAL_SERVER_ERROR, &err.to_string()),
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

/// Handle `POST /v1/files`.
pub(crate) async fn handle_files(runtime: &AgentRuntime, body: Bytes) -> Response {
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

    let store_dir = &runtime.config.store_dir;

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
        payload.extend_from_slice(&digest_bytes);
        payload.extend_from_slice(&(content.len() as u32).to_be_bytes());
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
