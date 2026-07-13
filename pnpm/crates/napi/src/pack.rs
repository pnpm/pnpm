//! `pack` — build a publishable `.tgz` from a project directory.
//!
//! Wraps [`pacquet_pack::api`], replacing Bit's side-load of
//! `@pnpm/releasing.commands`' internal `publish/pack.js`. The Rust `api` is
//! synchronous and does blocking filesystem work plus (unless
//! `ignoreScripts`) runs the `prepack` / `prepare` / `postpack` lifecycle
//! scripts, so it runs on a blocking pool. Lifecycle-script output is
//! forwarded to the optional `onLog` sink through [`NodeBridgeReporter`].

use std::{collections::HashMap, path::PathBuf};

use napi_derive::napi;
use pacquet_catalogs_types::Catalogs;
use pacquet_config::{NodeLinker, PNPM_VERSION};

use crate::{
    error::to_napi_error,
    install::engine_call_lock,
    reporter_bridge::{EngineCallGuard, LogSink, NodeBridgeReporter},
};

/// Inputs for [`pack`]. Mirrors [`PackOptions`] in `index.d.ts`.
#[napi(object)]
pub struct PackOptions {
    pub dir: String,
    pub workspace_dir: Option<String>,
    pub pack_destination: Option<String>,
    pub out: Option<String>,
    pub ignore_scripts: Option<bool>,
    pub pack_gzip_level: Option<u32>,
    pub embed_readme: Option<bool>,
    pub dry_run: Option<bool>,
    pub extra_bin_paths: Option<Vec<String>>,
    pub extra_env: Option<HashMap<String, String>>,
}

/// Result of [`pack`]. Mirrors [`PackResult`] in `index.d.ts`.
#[napi(object)]
pub struct PackResult {
    pub published_manifest: serde_json::Value,
    pub contents: Vec<String>,
    pub tarball_path: String,
    /// Uncompressed size in bytes. Exposed as an f64 to stay a plain JS
    /// `number` (a package tarball never approaches the 2^53 safe-integer
    /// limit).
    pub unpacked_size: f64,
}

#[napi]
pub async fn pack(options: PackOptions, on_log: Option<LogSink>) -> napi::Result<PackResult> {
    // Share the serialization lock with install/rebuild: they all drive the
    // same process-global log sink, so overlapping calls would misroute events.
    let _guard = engine_call_lock().lock().await;
    // Restores the previous sink on drop, whatever path `pack` returns on.
    let _sink_guard = EngineCallGuard::new(on_log);

    let pack_opts = pacquet_pack::PackOptions {
        dir: PathBuf::from(&options.dir),
        // Bit does not use catalog: specifiers; workspace catalog loading is
        // deferred until a consumer needs it. See pnpm/plans/NAPI.md.
        catalogs: Catalogs::default(),
        ignore_scripts: options.ignore_scripts.unwrap_or(false),
        unsafe_perm: false,
        embed_readme: options.embed_readme.unwrap_or(false),
        pack_gzip_level: options.pack_gzip_level,
        node_linker: NodeLinker::default(),
        skip_manifest_obfuscation: false,
        user_agent: format!("pnpm/{PNPM_VERSION} napi"),
        extra_bin_paths: options
            .extra_bin_paths
            .unwrap_or_default()
            .into_iter()
            .map(PathBuf::from)
            .collect(),
        extra_env: options.extra_env.unwrap_or_default(),
        workspace_dir: options.workspace_dir.map(PathBuf::from),
        dry_run: options.dry_run.unwrap_or(false),
        pack_destination: options.pack_destination,
        out: options.out,
        // Bit drives its own `readPackage` hook through the napi bridge and
        // loads no `beforePacking` pnpmfiles, so the hook loop is a no-op.
        pnpmfiles: Vec::new(),
    };

    let result = tokio::task::spawn_blocking(move || {
        // `api` is async; drive it to completion on the blocking thread so the
        // synchronous tarball write still runs off the async worker threads.
        tokio::runtime::Handle::current()
            .block_on(pacquet_pack::api::<NodeBridgeReporter, pacquet_pack::Host>(&pack_opts))
    })
    .await;

    match result {
        Ok(Ok(packed)) => Ok(PackResult {
            published_manifest: packed.published_manifest,
            contents: packed.contents,
            tarball_path: packed.tarball_path,
            unpacked_size: packed.unpacked_size as f64,
        }),
        Ok(Err(pack_error)) => Err(to_napi_error(&pack_error)),
        Err(join_error) => Err(napi::Error::from_reason(format!(
            "pack task panicked or was cancelled: {join_error}",
        ))),
    }
}
