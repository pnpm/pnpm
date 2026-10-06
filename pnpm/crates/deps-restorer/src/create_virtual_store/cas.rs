use super::{
    CreateVirtualStore, CreateVirtualStoreError, CreateVirtualStoreOutput, SnapshotCacheKey,
    cas_paths_key,
};
use pnpm_config::NodeLinker;
use pnpm_lockfile::{PackageKey, Prefix, SnapshotEntry};
use pnpm_reporter::Reporter;
use pnpm_tarball::PrefetchResult;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

impl CreateVirtualStore<'_> {
    pub async fn run<Report: Reporter>(
        mut self,
    ) -> Result<CreateVirtualStoreOutput, CreateVirtualStoreError> {
        if self.ctx.linker.kind == NodeLinker::Loaded {
            return Box::pin(self.run_cas::<Report>()).await;
        }
        self.run_inner::<Report>().await
    }

    pub(super) async fn run_cas<Report: Reporter>(
        self,
    ) -> Result<CreateVirtualStoreOutput, CreateVirtualStoreError> {
        validate_config(self.ctx.config)?;
        let selected = self.ctx
            .select_loaded_snapshots(self.entries.snapshots)
            .expect("loaded linker selection");
        let fetch_context = crate::InstallContext {
            linker: crate::ModuleLinkerContext { kind: NodeLinker::Hoisted, ..self.ctx.linker },
            ..self.ctx.clone()
        };
        let materialize_context = crate::InstallContext {
            linker: crate::ModuleLinkerContext { kind: NodeLinker::Isolated, ..self.ctx.linker },
            ..self.ctx.clone()
        };
        let mut installer = CreateVirtualStore { ctx: &fetch_context, ..self };
        let materialized_snapshots = (!selected.is_empty()).then_some(selected);
        let (fetched, prefetch) = installer.run_retaining::<Report>(materialized_snapshots).await?;
        validate_cas_builds(&fetched, selected, installer.ctx.config)?;
        installer.ctx = &materialize_context;
        installer.entries.snapshots = materialized_snapshots;
        installer.fetching.cas_prefetch = prefetch;
        let mut materialized = installer.run_inner::<Report>().await?;
        materialized.cas_paths_by_pkg_id = fetched.cas_paths_by_pkg_id;
        materialized.package_manifests.extend(fetched.package_manifests);
        materialized.fetch_failed.extend(fetched.fetch_failed);
        Ok(materialized)
    }
}

/// The cache keys of `snapshots`, copied before the fetch pass's plan
/// consumes them.
///
/// A key whose derivation failed is left out. The fetch pass fails on
/// it unless the snapshot is skipped as not installable, and the
/// materialization pass skips that snapshot too.
pub(super) fn retained_cache_keys(
    cache_keys: &HashMap<PackageKey, Result<SnapshotCacheKey, CreateVirtualStoreError>>,
    snapshots: &HashMap<PackageKey, SnapshotEntry>,
) -> HashMap<PackageKey, Result<SnapshotCacheKey, CreateVirtualStoreError>> {
    snapshots
        .keys()
        .filter_map(|snapshot_key| {
            let cache_key = cache_keys
                .get(snapshot_key)?
                .as_ref()
                .ok()?;
            Some((snapshot_key.clone(), Ok(cache_key.clone())))
        })
        .collect()
}

/// Prepare the loaded linker's fetch-pass rows for its materialization
/// pass, which then reads neither the store index nor the CAFS files of a
/// row the fetch pass already verified.
///
/// `cache_keys` are the materialization pass's snapshots. The rows are
/// narrowed to their keys, and the packages the fetch pass downloaded
/// join them, so the materialization pass links them as warm. A
/// git-hosted download does not join: whether its row may be reused also
/// depends on its recorded prepare state.
pub(super) fn retain_fetch_pass_rows(
    rows: &mut PrefetchResult,
    cache_keys: &HashMap<PackageKey, Result<SnapshotCacheKey, CreateVirtualStoreError>>,
    fetched: &CreateVirtualStoreOutput,
) {
    let row_keys: HashSet<&str> = cache_keys
        .values()
        .filter_map(|cache_key| {
            cache_key
                .as_ref()
                .ok()?
                .value
                .as_deref()
        })
        .collect();
    rows.retain_rows(|cache_key| row_keys.contains(cache_key));
    let Some(downloaded) = fetched.cas_paths_by_pkg_id.as_ref() else { return };
    for (snapshot_key, cache_key) in cache_keys {
        let Ok(SnapshotCacheKey {
            value: Some(cache_key),
            is_git_hosted: false,
        }) = cache_key
        else {
            continue;
        };
        if rows.cas_paths.contains_key(cache_key) {
            continue;
        }
        if let Some(files) = immutable_download(downloaded, snapshot_key) {
            rows.cas_paths.insert(cache_key.clone(), Arc::clone(&files.cas_paths));
            if let Some(&requires_build) = fetched.requires_build_by_snapshot.get(snapshot_key) {
                rows.requires_build.insert(cache_key.clone(), requires_build);
            }
        }
    }
}

fn immutable_download<'a>(
    downloaded: &'a crate::CasPathsByPkgId,
    snapshot_key: &PackageKey,
) -> Option<&'a crate::HoistedPackageFiles> {
    downloaded
        .get(&cas_paths_key(snapshot_key))
        .filter(|files| !files.source_is_mutable && files.source_exists)
}

pub(crate) fn materialized_snapshots(
    config: &pnpm_config::Config,
    snapshots: Option<&HashMap<PackageKey, SnapshotEntry>>,
) -> HashMap<PackageKey, SnapshotEntry> {
    let Some(snapshots) = snapshots else { return HashMap::new() };
    let mut pending: Vec<_> = snapshots
        .keys()
        .filter(|key| {
            config.node_linker_excluded.contains(&key.name.to_string())
                || crate::snapshot_has_patch(key)
                // A runtime's executables are native binaries, which the loader cannot run.
                || key.suffix.prefix() == Prefix::Runtime
        })
        .cloned()
        .collect();
    let mut selected = HashSet::new();
    while let Some(key) = pending.pop() {
        if !selected.insert(key.clone()) {
            continue;
        }
        let Some(snapshot) = snapshots.get(&key) else { continue };
        pending.extend(
            snapshot.dependencies
                .iter()
                .flatten()
                .chain(snapshot.optional_dependencies.iter().flatten())
                .filter_map(|(alias, reference)| reference.resolve(alias)),
        );
    }
    snapshots
        .iter()
        .filter(|(key, _)| selected.contains(*key))
        .map(|(key, snapshot)| (key.clone(), snapshot.clone()))
        .collect()
}

fn validate_cas_builds(
    fetched: &CreateVirtualStoreOutput,
    selected: &HashMap<PackageKey, SnapshotEntry>,
    config: &pnpm_config::Config,
) -> Result<(), CreateVirtualStoreError> {
    if config.ignore_scripts {
        return Ok(());
    }
    for (key, requires_build) in &fetched.requires_build_by_snapshot {
        if *requires_build && !selected.contains_key(key) {
            return Err(CreateVirtualStoreError::CasRequiresMaterialization {
                package: key.name.to_string(),
            });
        }
    }
    Ok(())
}

fn validate_config(config: &pnpm_config::Config) -> Result<(), CreateVirtualStoreError> {
    let message = if !config.enable_global_virtual_store {
        Some("CAS opt-outs require the global virtual store")
    } else if !config.symlink {
        Some("CAS opt-outs require symlink=true for their dependency trees")
    } else if config.node_experimental_package_map {
        Some("nodeExperimentalPackageMap cannot be combined with the CAS loader")
    } else {
        None
    };
    match message {
        Some(message) => Err(CreateVirtualStoreError::CasConfiguration { message }),
        None => Ok(()),
    }
}
