use crate::{
    ImportIndexedDirError, ImportIndexedDirOpts, SymlinkPackageError, import_indexed_dir,
    retry_config::retry_opts_from_config, symlink_package,
};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_config::Config;
use pacquet_network::ThrottledClient;
use pacquet_package_manifest::PackageManifest;
use pacquet_registry::{Package, PackageTag, PackageVersion, RegistryError};
use pacquet_reporter::{LogEvent, LogLevel, ProgressLog, ProgressMessage, Reporter};
use pacquet_store_dir::{SharedReadonlyStoreIndex, SharedVerifiedFilesCache, StoreIndexWriter};
use pacquet_tarball::{DownloadTarballToStore, MemCache, TarballError};
use std::{
    path::Path,
    sync::{Arc, atomic::AtomicU8},
};

/// This subroutine executes the following and returns the package
/// * Retrieves the package from the registry
/// * Extracts the tarball to global store directory (~/Library/../pacquet)
/// * Links global store directory to virtual dir (node_modules/.pacquet/..)
///
/// `name` is the manifest dependency key — the directory name the
/// package will be exposed as inside `node_modules`. For an npm-alias
/// entry (`"foo": "npm:bar@^1.0.0"`), `name` is the local alias (`foo`)
/// and the actual registry package name (`bar`) is parsed out of
/// `version_range` before the registry lookup.
///
/// `symlink_path` will be appended by `name`. Therefore, it should be
/// resolved into the node_modules folder of a subdependency such as
/// `node_modules/.pacquet/fastify@1.0.0/node_modules`.
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
    pub name: &'a str,
    pub version_range: &'a str,
}

/// Error type of [`InstallPackageFromRegistry`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum InstallPackageFromRegistryError {
    FetchFromRegistry(#[error(source)] RegistryError),
    DownloadTarballToStore(#[error(source)] TarballError),
    ImportIndexedDir(#[error(source)] ImportIndexedDirError),
    SymlinkPackage(#[error(source)] SymlinkPackageError),
}

impl<'a> InstallPackageFromRegistry<'a> {
    /// Execute the subroutine.
    pub async fn run<Reporter: self::Reporter>(
        self,
    ) -> Result<PackageVersion, InstallPackageFromRegistryError> {
        let &InstallPackageFromRegistry { http_client, config, name, version_range, .. } = &self;

        // Strip any `npm:<name>@<range>` alias prefix before talking to
        // the registry. `name` (the manifest key) stays as the directory
        // name inside `node_modules`. Unversioned aliases (`npm:foo`) are
        // resolved to `"latest"` by `resolve_registry_dependency`.
        let (registry_name, version_range) =
            PackageManifest::resolve_registry_dependency(name, version_range);

        // Try parsing as a `PackageTag` first: this covers both the
        // `"latest"` tag (including unversioned `npm:` aliases) and
        // pinned versions like `"1.0.0"`. Semver ranges like `"^1.0.0"`
        // fail `PackageTag::from_str` and fall through to the range
        // resolution branch below.
        Ok(if let Ok(tag) = version_range.parse::<PackageTag>() {
            let package_version = PackageVersion::fetch_from_registry(
                registry_name,
                tag,
                http_client,
                &config.registry,
                &config.auth_headers,
            )
            .await
            .map_err(InstallPackageFromRegistryError::FetchFromRegistry)?;
            self.install_package_version::<Reporter>(&package_version).await?;
            package_version
        } else {
            let package = Package::fetch_from_registry(
                registry_name,
                http_client,
                &config.registry,
                &config.auth_headers,
            )
            .await
            .map_err(InstallPackageFromRegistryError::FetchFromRegistry)?;
            let package_version = package.pinned_version(version_range).unwrap(); // TODO: propagate error for when no version satisfies range
            self.install_package_version::<Reporter>(package_version).await?;
            package_version.clone()
        })
    }

    async fn install_package_version<Reporter: self::Reporter>(
        self,
        package_version: &PackageVersion,
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
            name,
            ..
        } = self;

        let store_folder_name = package_version.to_virtual_store_name();
        let package_id = format!("{0}@{1}", package_version.name, package_version.version);

        // `pnpm:progress resolved` mirrors pnpm's emit at
        // <https://github.com/pnpm/pnpm/blob/086c5e91e8/installing/deps-resolver/src/resolveDependencies.ts#L1586>:
        // one event per package once the resolver has picked a
        // version. In pacquet's no-lockfile path that's the
        // registry-fetched `package_version`; emit before the
        // tarball download so consumers see resolved → fetched/
        // found_in_store → imported in order.
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
            package_integrity: package_version
                .dist
                .integrity
                .as_ref()
                .expect("has integrity field"),
            package_unpacked_size: package_version.dist.unpacked_size,
            package_url: package_version.as_tarball_url(),
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

        // The virtual store always uses the registry-returned name
        // (`package_version.name`), so npm-alias entries share a single
        // virtual store directory with their non-aliased counterparts.
        // The exposed symlink under `node_modules/` uses the manifest
        // key (`name`) so both forms can coexist in the same parent.
        let save_path = config
            .virtual_store_dir
            .join(store_folder_name)
            .join("node_modules")
            .join(&package_version.name);

        let symlink_path = node_modules_dir.join(name);

        tracing::info!(target: "pacquet::import", ?save_path, ?symlink_path, "Import package");

        import_indexed_dir::<Reporter>(
            logged_methods,
            config.package_import_method,
            &save_path,
            &cas_paths,
            ImportIndexedDirOpts::default(),
        )
        .map_err(InstallPackageFromRegistryError::ImportIndexedDir)?;

        symlink_package(&save_path, &symlink_path)
            .map_err(InstallPackageFromRegistryError::SymlinkPackage)?;

        // `pnpm:progress imported` — see the matching emit in
        // `create_virtual_dir_by_snapshot::run` for the rationale on
        // the optimistic `method` value. `to` is the per-package
        // virtual-store directory the symlink under
        // `node_modules/{name}` resolves to.
        Reporter::emit(&LogEvent::Progress(ProgressLog {
            level: LogLevel::Debug,
            message: ProgressMessage::Imported {
                method: crate::optimistic_wire_method(config.package_import_method),
                requester: requester.to_owned(),
                to: save_path.to_string_lossy().into_owned(),
            },
        }));

        Ok(())
    }
}

#[cfg(test)]
mod tests;
