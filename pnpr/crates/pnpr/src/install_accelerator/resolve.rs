//! Server-side dependency resolution backed by pacquet.
//!
//! Mirrors the pnpm-agent TypeScript flow: write a throwaway project,
//! resolve it lockfile-only (so `node_modules` is never linked), read
//! the produced lockfile back, then fetch into the shared store only
//! the packages that aren't cached yet. The store index that results
//! is the source of truth the [`super::diff`] pass reads.

use std::{
    collections::HashSet,
    sync::{Arc, atomic::AtomicU8},
};

use dashmap::DashMap;
use pacquet_config::{Config, NodeLinker};
use pacquet_lockfile::{Lockfile, LockfileResolution};
use pacquet_network::ThrottledClient;
use pacquet_package_manager::{Install, ResolvedPackages};
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pacquet_reporter::SilentReporter;
use pacquet_store_dir::{SharedVerifiedFilesCache, StoreIndex, StoreIndexWriter, store_index_key};
use pacquet_tarball::{DownloadTarballToStore, MemCache, RetryOpts};

use super::protocol::InstallRequest;

/// A resolved package distilled from the lockfile, carrying everything
/// needed both to fetch it (`tarball_url`) and to diff it (`integrity`,
/// `pkg_id`).
pub struct ResolvedPkg {
    pub pkg_id: String,
    pub integrity: String,
    pub tarball_url: String,
}

#[derive(Debug)]
pub enum ResolveError {
    Io(std::io::Error),
    Manifest(String),
    Install(String),
    NoLockfile,
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolveError::Io(err) => write!(f, "{err}"),
            ResolveError::Manifest(msg) => write!(f, "failed to read manifest: {msg}"),
            ResolveError::Install(msg) => write!(f, "resolution failed: {msg}"),
            ResolveError::NoLockfile => write!(f, "resolution produced no lockfile"),
        }
    }
}

impl From<std::io::Error> for ResolveError {
    fn from(err: std::io::Error) -> Self {
        ResolveError::Io(err)
    }
}

/// Resolve a single-project request lockfile-only and return the
/// produced lockfile. The store is intentionally left untouched here
/// (no tarball is fetched); [`fetch_uncached`] populates it afterward.
pub async fn resolve(
    config: &'static Config,
    client: &Arc<ThrottledClient>,
    request: &InstallRequest,
) -> Result<Lockfile, ResolveError> {
    let project = request.single_project();

    let temp = tempfile::Builder::new().prefix("pnpr-resolve-").tempdir()?;
    let dir = temp.path();
    let manifest_path = dir.join("package.json");
    let manifest_json = serde_json::json!({
        "name": "pnpr-resolve",
        "version": "0.0.0",
        "dependencies": project.dependencies,
        "devDependencies": project.dev_dependencies,
    });
    let manifest_bytes =
        serde_json::to_vec(&manifest_json).map_err(|err| ResolveError::Install(err.to_string()))?;
    tokio::fs::write(&manifest_path, manifest_bytes).await?;

    let manifest = PackageManifest::from_path(manifest_path)
        .map_err(|err| ResolveError::Manifest(err.to_string()))?;

    // Seed resolution from the client's lockfile when present, matching
    // pnpm's resolution-reuse: frozen → use it as-is (already verified
    // by the caller before this point); non-frozen → reuse its pins for
    // unchanged entries and resolve only what's new/changed
    // (`preferFrozenLockfile` + `update: false`). With no lockfile it's a
    // fresh resolve. `frozen_lockfile` is meaningful only with a lockfile
    // to freeze; without one we resolve fresh rather than error.
    let input_lockfile = request.lockfile.as_ref();
    let lockfile_path = dir.join(Lockfile::FILE_NAME);
    if let Some(lockfile) = input_lockfile {
        lockfile
            .save_to_path(&lockfile_path)
            .map_err(|err| ResolveError::Install(err.to_string()))?;
    }
    let frozen_lockfile = request.frozen_lockfile && input_lockfile.is_some();

    let resolved_packages: ResolvedPackages = DashMap::new();
    let tarball_mem_cache: Arc<MemCache> = Arc::new(MemCache::default());
    let _logged = AtomicU8::new(0);

    Install {
        tarball_mem_cache,
        resolved_packages: &resolved_packages,
        http_client: client,
        http_client_arc: Arc::clone(client),
        config,
        manifest: &manifest,
        lockfile: input_lockfile,
        lockfile_path: input_lockfile.map(|_| lockfile_path.as_path()),
        dependency_groups: vec![
            DependencyGroup::Prod,
            DependencyGroup::Dev,
            DependencyGroup::Optional,
        ],
        frozen_lockfile,
        prefer_frozen_lockfile: Some(true),
        ignore_manifest_check: false,
        skip_runtimes: false,
        // The lockfile was already verified under the client's policy
        // (in `handle_install`) before we get here, so the install path
        // must not re-verify it.
        trust_lockfile: true,
        update_checksums: false,
        is_full_install: true,
        supported_architectures: None,
        node_linker: NodeLinker::Isolated,
        lockfile_only: true,
        update_seed_policy: pacquet_package_manager::UpdateSeedPolicy::KeepAll,
    }
    .run::<SilentReporter>()
    .await
    .map_err(|err| ResolveError::Install(err.to_string()))?;

    let lockfile = Lockfile::load_wanted_from_dir(dir)
        .map_err(|err| ResolveError::Install(err.to_string()))?
        .ok_or(ResolveError::NoLockfile)?;

    Ok(lockfile)
}

/// Extract every registry/tarball package from the lockfile, deriving
/// the tarball URL the same way pacquet's install path does (registry
/// resolutions never store the URL in a v9 lockfile).
pub fn collect_packages(lockfile: &Lockfile, registry: &str) -> Vec<ResolvedPkg> {
    let Some(packages) = lockfile.packages.as_ref() else { return Vec::new() };
    let mut out = Vec::with_capacity(packages.len());
    for (key, metadata) in packages {
        let dep_path = key.to_string();
        let pkg_id = dep_path.split('(').next().unwrap_or(&dep_path).to_string();
        let Some((integrity, tarball_url)) = fetch_info(&metadata.resolution, &pkg_id, registry)
        else {
            continue;
        };
        out.push(ResolvedPkg { pkg_id, integrity, tarball_url });
    }
    out
}

/// Fetch into the shared store every package whose store-index row is
/// absent, populating its `PackageFilesIndex` as a side effect. Cached
/// packages are skipped, matching the server hot-cache no-op.
pub async fn fetch_uncached(
    config: &'static Config,
    client: &Arc<ThrottledClient>,
    packages: &[ResolvedPkg],
) -> Result<(), ResolveError> {
    let store_dir = &config.store_dir;

    let present: HashSet<String> = match StoreIndex::open_readonly_in(store_dir) {
        Ok(index) => index.keys().unwrap_or_default().into_iter().collect(),
        Err(_) => HashSet::new(),
    };

    let to_fetch: Vec<&ResolvedPkg> = packages
        .iter()
        .filter(|pkg| !present.contains(&store_index_key(&pkg.integrity, &pkg.pkg_id)))
        .filter(|pkg| !pkg.tarball_url.is_empty())
        .collect();

    if to_fetch.is_empty() {
        return Ok(());
    }

    let integrities: Vec<Option<ssri::Integrity>> =
        to_fetch.iter().map(|pkg| pkg.integrity.parse::<ssri::Integrity>().ok()).collect();

    let shared_index = StoreIndex::shared_readonly_in(store_dir);
    let (writer, writer_task) = StoreIndexWriter::spawn(store_dir);
    let verified = SharedVerifiedFilesCache::default();

    let downloads = to_fetch.iter().zip(integrities.iter()).filter_map(|(pkg, integrity)| {
        let integrity = integrity.as_ref()?;
        let store_index = shared_index.clone();
        let writer = Arc::clone(&writer);
        let verified = SharedVerifiedFilesCache::clone(&verified);
        Some(async move {
            DownloadTarballToStore {
                http_client: client,
                store_dir,
                store_index,
                store_index_writer: Some(writer),
                verify_store_integrity: config.verify_store_integrity,
                verified_files_cache: verified,
                package_integrity: integrity,
                package_unpacked_size: None,
                package_url: &pkg.tarball_url,
                package_id: &pkg.pkg_id,
                auth_headers: &config.auth_headers,
                requester: "pnpr",
                prefetched_cas_paths: None,
                retry_opts: RetryOpts::default(),
                ignore_file_pattern: None,
                offline: false,
            }
            .run_without_mem_cache::<SilentReporter>()
            .await
            .map_err(|err| ResolveError::Install(err.to_string()))
        })
    });

    let results = futures_util::future::join_all(downloads).await;

    drop(writer);
    let _ = writer_task.await;

    for result in results {
        result?;
    }
    Ok(())
}

/// Derive `(integrity, tarball_url)` for a resolution, mirroring
/// pacquet's `tarball_url_and_integrity`. Returns `None` for git,
/// directory, binary, and variations resolutions (not served by the
/// pnpr install accelerator).
fn fetch_info(
    resolution: &LockfileResolution,
    pkg_id: &str,
    registry: &str,
) -> Option<(String, String)> {
    match resolution {
        LockfileResolution::Tarball(tarball) => {
            let integrity = tarball.integrity.as_ref()?;
            Some((integrity.to_string(), tarball.tarball.clone()))
        }
        LockfileResolution::Registry(registry_resolution) => {
            let (name, version) = split_name_version(pkg_id)?;
            let bare = name.rsplit('/').next().unwrap_or(name);
            let registry = registry.strip_suffix('/').unwrap_or(registry);
            Some((
                registry_resolution.integrity.to_string(),
                format!("{registry}/{name}/-/{bare}-{version}.tgz"),
            ))
        }
        _ => None,
    }
}

/// Split `name@version` into its parts, tolerating a leading scope
/// `@` (`@scope/name@1.2.3` → `("@scope/name", "1.2.3")`).
fn split_name_version(pkg_id: &str) -> Option<(&str, &str)> {
    let at = pkg_id.rfind('@')?;
    if at == 0 {
        return None;
    }
    Some((&pkg_id[..at], &pkg_id[at + 1..]))
}
