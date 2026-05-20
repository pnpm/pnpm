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
///   `<virtual_store_dir>/<virtual-store-name>/node_modules/<real-name>/`.
/// * Symlinks `<node_modules_dir>/<alias>` to the virtual-store
///   directory.
///
/// `alias` is the local install name in `node_modules`: the manifest
/// key. For an npm-alias entry (`"foo": "npm:bar@^1"`) it's the alias
/// (`foo`); the registry-side name is read from [`ResolveResult::id`].
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
    /// Install-scoped dedupe state for `pnpm:package-import-method`.
    /// See `link_file::log_method_once`.
    pub logged_methods: &'a AtomicU8,
    /// Install root, threaded into reporter events (`pnpm:progress`'s
    /// `requester`). Same value as the `prefix` in
    /// [`pacquet_reporter::StageLog`].
    pub requester: &'a str,
    pub node_modules_dir: &'a Path,
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

impl<'a> InstallPackageFromRegistry<'a> {
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
            logged_methods,
            requester,
            node_modules_dir,
            alias,
            resolution,
            first_visit,
        } = self;

        let real_name = resolution.id.name.to_string();
        let version = resolution.id.suffix.to_string();
        let virtual_store_name = format!("{}@{}", real_name.replace('/', "+"), version);
        let package_id = format!("{real_name}@{version}");

        // The virtual store always uses the registry-returned name
        // so npm-alias entries share a single virtual store directory
        // with their non-aliased counterparts. The exposed symlink
        // under `node_modules/` uses the manifest key (`alias`) so
        // both forms can coexist in the same parent.
        let save_path = config
            .virtual_store_dir
            .join(&virtual_store_name)
            .join("node_modules")
            .join(&real_name);

        let symlink_path = node_modules_dir.join(alias);

        if first_visit {
            let (tarball_url, integrity) = extract_tarball(&resolution.resolution)?;
            let unpacked_size = manifest_unpacked_size(resolution.manifest.as_ref());

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
                package_url: tarball_url,
                package_id: &package_id,
                requester,
                prefetched_cas_paths: None,
                retry_opts: retry_opts_from_config(config),
                auth_headers: &config.auth_headers,
                ignore_file_pattern: None,
                offline: config.offline,
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

/// Pull the tarball URL + integrity hash out of the resolver-produced
/// resolution. Refuses any shape the npm install path can't fetch.
fn extract_tarball(
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
fn manifest_unpacked_size(manifest: Option<&Value>) -> Option<usize> {
    // `usize::try_from` so a `u64` value larger than the host's
    // `usize` (32-bit targets) degrades to "no hint" rather than
    // truncating silently and producing an undersized pre-allocation.
    manifest?
        .get("dist")?
        .get("unpackedSize")?
        .as_u64()
        .and_then(|value| usize::try_from(value).ok())
}

#[cfg(test)]
mod tests;
