use crate::{
    ImportIndexedDirError, ImportIndexedDirOpts, SymlinkPackageError, import_indexed_dir,
    retry_config::retry_opts_from_config, symlink_package,
};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_config::Config;
use pacquet_lockfile::LockfileResolution;
use pacquet_network::ThrottledClient;
use pacquet_reporter::{LogEvent, LogLevel, ProgressLog, ProgressMessage, Reporter};
use pacquet_resolving_resolver_base::ResolveResult;
use pacquet_store_dir::{SharedReadonlyStoreIndex, SharedVerifiedFilesCache, StoreIndexWriter};
use pacquet_tarball::{DownloadTarballToStore, MemCache, TarballError};
use serde_json::Value;
use ssri::Integrity;
use std::{
    path::Path,
    sync::{Arc, atomic::AtomicU8},
};

/// Materialize one pre-resolved package on disk:
///
/// * Downloads the tarball into the global store directory.
/// * Imports (reflinks, hardlinks, or copies) the unpacked files into
///   `<slot_dir>/node_modules/<real-name>/`.
/// * Symlinks `<node_modules_dir>/<alias>` to the virtual-store
///   directory.
///
/// `alias` is the local install name in `node_modules`: the manifest
/// key. For an npm-alias entry (`"foo": "npm:bar@^1"`) it's the alias
/// (`foo`); the registry-side name is read from [`ResolveResult::id`].
///
/// `slot_dir` is the per-package virtual-store directory the caller
/// computed from a [`crate::VirtualStoreLayout`]. Under GVS this is
/// `<store_dir>/links/<scope>/<name>/<version>/<hash>`; under the
/// legacy flat layout it is `<virtual_store_dir>/<flat-name>`. The
/// caller resolves the layout once per install and threads the
/// resulting path in so per-package code stays layout-agnostic.
#[must_use]
pub struct InstallPackageFromRegistry<'a> {
    pub tarball_mem_cache: &'a MemCache,
    pub http_client: &'a ThrottledClient,
    pub config: &'static Config,
    pub store_index: Option<&'a SharedReadonlyStoreIndex>,
    pub store_index_writer: Option<&'a Arc<StoreIndexWriter>>,
    /// Install-scoped `verifiedFilesCache` shared across every
    /// per-package fetch. See `DownloadTarballToStore::verified_files_cache`
    /// for the rationale.
    pub verified_files_cache: &'a SharedVerifiedFilesCache,
    /// Warm-cache prefetch result built once per install via
    /// [`pacquet_tarball::prefetch_cas_paths`] — `cache_key →
    /// Arc<cas_paths>`. When `Some`, the
    /// `DownloadTarballToStore::run_without_mem_cache` cache-lookup
    /// branch reads from here before falling back to the per-snapshot
    /// `SQLite` lookup, avoiding `Arc<Mutex<StoreIndex>>` contention on
    /// the resolve hot path.
    pub prefetched_cas_paths: Option<&'a pacquet_tarball::PrefetchedCasPaths>,
    /// Install-scoped dedupe state for `pnpm:package-import-method`.
    /// See `link_file::log_method_once`.
    pub logged_methods: &'a AtomicU8,
    /// Install root, threaded into reporter events (`pnpm:progress`'s
    /// `requester`). Same value as the `prefix` in
    /// [`pacquet_reporter::StageLog`].
    pub requester: &'a str,
    pub node_modules_dir: &'a Path,
    /// Per-package virtual-store directory — output of
    /// [`crate::VirtualStoreLayout::slot_dir`] for this package's
    /// snapshot key. The unpacked files land at
    /// `<slot_dir>/node_modules/<real-name>/`.
    pub slot_dir: &'a Path,
    /// Local install name in `node_modules/`.
    pub alias: &'a str,
    /// Pre-resolved package returned by the resolver chain.
    pub resolution: &'a ResolveResult,
    /// `true` when this is the first edge encountered for this
    /// `(name, version)` slot. Gates the per-package work: the tarball
    /// download, the virtual-store import, and the
    /// `pnpm:progress resolved` / `pnpm:progress imported` emits all
    /// fire on the first visit. Subsequent visitors only refresh the
    /// per-parent symlink under `node_modules_dir/<alias>`, mirroring
    /// upstream's per-package (not per-edge) progress signalling at
    /// <https://github.com/pnpm/pnpm/blob/086c5e91e8/installing/deps-resolver/src/resolveDependencies.ts#L1586>.
    pub first_visit: bool,
}

/// Error type of [`InstallPackageFromRegistry`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum InstallPackageFromRegistryError {
    DownloadTarballToStore(#[error(source)] TarballError),
    ImportIndexedDir(#[error(source)] ImportIndexedDirError),
    SymlinkPackage(#[error(source)] SymlinkPackageError),

    /// The resolver produced a resolution shape the npm install path
    /// can't materialize (today: anything other than a tarball
    /// resolution carrying an integrity hash). Surfaces with a
    /// pacquet-internal code; the matching pnpm error is upstream's
    /// generic install failure for the same shape.
    #[display("Unsupported resolution shape for npm install path: {detail}")]
    #[diagnostic(code(pacquet_package_manager::unsupported_resolution))]
    UnsupportedResolution {
        #[error(not(source))]
        detail: String,
    },
}

impl InstallPackageFromRegistry<'_> {
    /// Execute the subroutine.
    pub async fn run<Reporter: self::Reporter>(
        self,
    ) -> Result<(), InstallPackageFromRegistryError> {
        let InstallPackageFromRegistry {
            tarball_mem_cache,
            http_client,
            config,
            store_index,
            store_index_writer,
            verified_files_cache,
            prefetched_cas_paths,
            logged_methods,
            requester,
            node_modules_dir,
            slot_dir,
            alias,
            resolution,
            first_visit,
        } = self;

        let (real_name, version) = real_name_version(resolution).ok_or_else(|| {
            InstallPackageFromRegistryError::UnsupportedResolution {
                detail: format!(
                    "resolver {resolved_via} produced a resolution without a structured \
                     name@version and no manifest name/version to fall back to (alias={alias})",
                    resolved_via = resolution.resolved_via,
                ),
            }
        })?;
        let package_id = format!("{real_name}@{version}");

        // The exposed symlink under `node_modules/` uses the manifest
        // key (`alias`) so an npm-alias entry and its non-aliased
        // counterpart can coexist in the same parent, both pointing
        // at the same registry-named subdirectory inside `slot_dir`.
        let save_path = slot_dir.join("node_modules").join(&real_name);

        let symlink_path = node_modules_dir.join(alias);

        if first_visit {
            let (tarball_url, integrity) = extract_tarball(&resolution.resolution)?;
            let unpacked_size = manifest_unpacked_size(resolution.manifest.as_deref());
            let file_count = manifest_file_count(resolution.manifest.as_deref());

            // `pnpm:progress resolved` mirrors pnpm's emit at
            // <https://github.com/pnpm/pnpm/blob/086c5e91e8/installing/deps-resolver/src/resolveDependencies.ts#L1586>:
            // one event per package once the resolver has picked a
            // version. Emit before the tarball download so consumers
            // see resolved → fetched/found_in_store → imported in
            // order.
            Reporter::emit(&LogEvent::Progress(ProgressLog {
                level: LogLevel::Debug,
                message: ProgressMessage::Resolved {
                    package_id: package_id.clone(),
                    requester: requester.to_owned(),
                },
            }));

            // TODO: skip when it already exists in store?
            let cas_paths = DownloadTarballToStore {
                http_client,
                store_dir: &config.store_dir,
                store_index: store_index.cloned(),
                store_index_writer: store_index_writer.cloned(),
                verify_store_integrity: config.verify_store_integrity,
                verified_files_cache: SharedVerifiedFilesCache::clone(verified_files_cache),
                package_integrity: &integrity,
                package_unpacked_size: unpacked_size,
                package_file_count: file_count,
                package_url: tarball_url,
                package_id: &package_id,
                requester,
                prefetched_cas_paths,
                retry_opts: retry_opts_from_config(config),
                auth_headers: &config.auth_headers,
                ignore_file_pattern: None,
                offline: config.offline,
                // This recursive install path owns its package-status
                // progress directly; no resolve-time prefetch shares a
                // dedupe set with it.
                progress_reported: None,
            }
            .run_with_mem_cache::<Reporter>(tarball_mem_cache)
            .await
            .map_err(InstallPackageFromRegistryError::DownloadTarballToStore)?;

            tracing::info!(target: "pacquet::import", ?save_path, ?symlink_path, "Import package");

            import_indexed_dir::<Reporter>(
                logged_methods,
                config.package_import_method,
                &save_path,
                &cas_paths,
                ImportIndexedDirOpts::default(),
            )
            .map_err(InstallPackageFromRegistryError::ImportIndexedDir)?;

            // `pnpm:progress imported` — see the matching emit in
            // `create_virtual_dir_by_snapshot::run` for the rationale
            // on the optimistic `method` value. `to` is the per-
            // package virtual-store directory the symlink under
            // `node_modules/{alias}` resolves to.
            Reporter::emit(&LogEvent::Progress(ProgressLog {
                level: LogLevel::Debug,
                message: ProgressMessage::Imported {
                    method: crate::optimistic_wire_method(config.package_import_method),
                    requester: requester.to_owned(),
                    to: save_path.to_string_lossy().into_owned(),
                },
            }));
        }

        // The per-parent symlink is the only step that runs on every
        // visit. Mirrors pnpm: one `pnpm:progress` sequence per
        // package, plus one symlink per direct edge.
        symlink_package(&save_path, &symlink_path)
            .map_err(InstallPackageFromRegistryError::SymlinkPackage)?;

        Ok(())
    }
}

/// The package's canonical `(name, version)`. Prefers the resolver's
/// structured `name_ver` (npm registry); falls back to the name/version
/// read from the fetched manifest for resolvers that learn them only
/// after the fetch — remote (non-registry) tarball, git, and file deps,
/// whose name/version live in `package.json`. Mirrors pnpm's
/// [`getManifestFromResponse`](https://github.com/pnpm/pnpm/blob/df990fdb51/installing/deps-resolver/src/resolveDependencies.ts)
/// fallback.
fn real_name_version(resolution: &ResolveResult) -> Option<(String, String)> {
    if let Some(name_ver) = resolution.name_ver.as_ref() {
        return Some((name_ver.name.to_string(), name_ver.suffix.to_string()));
    }
    let manifest = resolution.manifest.as_deref()?;
    let name = manifest.get("name")?.as_str()?.to_string();
    let version = manifest.get("version")?.as_str()?.to_string();
    Some((name, version))
}

/// Pull the tarball URL + integrity hash out of the resolver-produced
/// resolution. Refuses any shape the npm install path can't fetch.
pub(crate) fn extract_tarball(
    resolution: &LockfileResolution,
) -> Result<(&str, Integrity), InstallPackageFromRegistryError> {
    match resolution {
        LockfileResolution::Tarball(t) => {
            let integrity = t.integrity.clone().ok_or_else(|| {
                InstallPackageFromRegistryError::UnsupportedResolution {
                    detail: "tarball resolution missing integrity hash".to_string(),
                }
            })?;
            Ok((t.tarball.as_str(), integrity))
        }
        LockfileResolution::Registry(_)
        | LockfileResolution::Directory(_)
        | LockfileResolution::Git(_)
        | LockfileResolution::Binary(_)
        | LockfileResolution::Variations(_) => {
            Err(InstallPackageFromRegistryError::UnsupportedResolution {
                detail: format!("{resolution:?}"),
            })
        }
    }
}

/// Read `dist.unpackedSize` off the resolver-fetched manifest. Returns
/// `None` when missing or non-numeric — the tarball extractor treats it
/// as a hint, not a hard requirement.
pub(crate) fn manifest_unpacked_size(manifest: Option<&Value>) -> Option<usize> {
    manifest_dist_field(manifest, "unpackedSize")
}

/// Read `dist.fileCount` off the resolver-fetched manifest. Feeds the
/// download priority's per-file pipeline-cost term; `None` when the
/// registry never published one.
pub(crate) fn manifest_file_count(manifest: Option<&Value>) -> Option<usize> {
    manifest_dist_field(manifest, "fileCount")
}

fn manifest_dist_field(manifest: Option<&Value>, field: &str) -> Option<usize> {
    // `usize::try_from` so a `u64` value larger than the host's
    // `usize` (32-bit targets) degrades to "no hint" rather than
    // truncating silently and producing an undersized pre-allocation.
    manifest?.get("dist")?.get(field)?.as_u64().and_then(|value| usize::try_from(value).ok())
}

#[cfg(test)]
mod tests;
