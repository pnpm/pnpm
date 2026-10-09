//! The lockfile entries a tarball prefetch or store fetch stages, and what
//! staging one can fail on.

use crate::install_package_by_snapshot::tarball_url_and_integrity;
use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_config::Config;
use pnpm_deps_restorer::InstallPackageBySnapshotError;
use pnpm_lockfile::{Lockfile, LockfileResolution, PackageKey, PackageMetadata};
use pnpm_package_is_installable::{
    SupportedArchitectures, WantedPlatformRef, platform_is_supported,
};
use pnpm_store_dir::{SharedReadonlyStoreIndex, store_index_key};
use pnpm_tarball::TarballError;
use std::collections::HashSet;

/// One registry lockfile entry [`TarballPrefetcher::prefetch_lockfile`]
/// may spawn a download for, staged so the whole batch can be filtered
/// through a single store-index existence probe first.
pub(super) struct PendingPrefetch {
    pub(super) store_key: String,
    pub(super) package_id: String,
    pub(super) package_url: String,
    pub(super) integrity: String,
    pub(super) revision_addressed: bool,
}

/// Drop every pending entry whose `(integrity, package_id)` row already
/// exists in `index.db`, with one batched existence probe.
pub(super) async fn without_store_hits(
    index: Option<SharedReadonlyStoreIndex>,
    pending: Vec<PendingPrefetch>,
) -> Vec<PendingPrefetch> {
    let Some(index) = index else {
        return pending;
    };
    let keys: Vec<String> = pending
        .iter()
        .map(|entry| entry.store_key.clone())
        .collect();
    let hits = tokio::task::spawn_blocking(move || {
        let Ok(guard) = index.lock() else {
            return HashSet::new();
        };
        guard.contains_many(&keys).unwrap_or_default()
    })
    .await
    .unwrap_or_default();
    pending
        .into_iter()
        .filter(|entry| !hits.contains(&entry.store_key))
        .collect()
}

/// What the store fetch of a lockfile can fail on.
#[derive(Debug, Display, Error, Diagnostic)]
pub enum StoreFetchError {
    /// A registry entry the install could not fetch either: a named registry
    /// the config lacks, an integrity with nothing to check.
    #[diagnostic(transparent)]
    Entry(#[error(source)] InstallPackageBySnapshotError),
    #[diagnostic(transparent)]
    Download(#[error(source)] TarballError),
}

/// One lockfile entry staged for a fetch: `None` for a resolution that is
/// not a registry tarball or a package the host cannot install, which the
/// materialization never fetches either.
pub(super) fn registry_entry(
    package_key: &PackageKey,
    metadata: &PackageMetadata,
    config: &Config,
    supported_architectures: Option<&SupportedArchitectures>,
) -> Result<Option<PendingPrefetch>, InstallPackageBySnapshotError> {
    if !matches!(&metadata.resolution, LockfileResolution::Registry(_))
        || !host_can_install(metadata, supported_architectures)
    {
        return Ok(None);
    }
    let (tarball_url, integrity) =
        tarball_url_and_integrity(&metadata.resolution, package_key, config)?;
    let Some(integrity) = integrity else {
        return Ok(None);
    };
    let package_id = package_key.pkg_id();
    let integrity = integrity.to_string();
    let revision_addressed = matches!(
        &metadata.resolution,
        LockfileResolution::Registry(registry) if registry.revision.is_some(),
    );
    Ok(Some(PendingPrefetch {
        store_key: store_index_key(&integrity, &package_id),
        package_id,
        package_url: tarball_url.into_owned(),
        integrity,
        revision_addressed,
    }))
}

/// The entries a speculative prefetch downloads: one the install could not
/// fetch is left to the install and its own error.
pub(super) fn registry_entries(
    lockfile: &Lockfile,
    config: &Config,
    supported_architectures: Option<&SupportedArchitectures>,
) -> Vec<PendingPrefetch> {
    let Some(packages) = lockfile.packages.as_ref() else {
        return Vec::new();
    };
    let mut pending = Vec::with_capacity(packages.len());
    for (package_key, metadata) in packages {
        match registry_entry(package_key, metadata, config, supported_architectures) {
            Ok(Some(entry)) => pending.push(entry),
            Ok(None) => {}
            Err(error) => tracing::debug!(
                target: "pacquet::install",
                %package_key,
                ?error,
                "skipping the tarball prefetch of an unfetchable registry entry",
            ),
        }
    }
    pending
}

/// The entries the store fetch downloads: this fetch is the install's own,
/// so an entry it cannot fetch fails it.
pub(super) fn fetchable_entries(
    lockfile: &Lockfile,
    config: &Config,
    supported_architectures: Option<&SupportedArchitectures>,
) -> Result<Vec<PendingPrefetch>, InstallPackageBySnapshotError> {
    let Some(packages) = lockfile.packages.as_ref() else {
        return Ok(Vec::new());
    };
    let mut pending = Vec::with_capacity(packages.len());
    for (package_key, metadata) in packages {
        if let Some(entry) = registry_entry(package_key, metadata, config, supported_architectures)?
        {
            pending.push(entry);
        }
    }
    Ok(pending)
}

fn host_can_install(
    metadata: &PackageMetadata,
    supported_architectures: Option<&SupportedArchitectures>,
) -> bool {
    platform_is_supported(
        WantedPlatformRef {
            os: metadata.os.as_deref(),
            cpu: metadata.cpu.as_deref(),
            libc: metadata.libc.as_deref(),
        },
        supported_architectures,
        pnpm_graph_hasher::host_platform(),
        pnpm_graph_hasher::host_arch(),
        pnpm_graph_hasher::host_libc(),
    )
}
