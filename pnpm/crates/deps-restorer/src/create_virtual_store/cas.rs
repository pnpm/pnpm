mod cached_builds;
mod capabilities;
mod temporary_builds;

use super::{
    ConcurrentCheck, CreateVirtualStore, CreateVirtualStoreError, CreateVirtualStoreOutput,
    SnapshotCacheKey, cas_paths_key, settle_check,
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
        self,
    ) -> Result<CreateVirtualStoreOutput, CreateVirtualStoreError> {
        self.run_with_check::<Report>(None).await
    }

    /// [`Self::run`] with `check` polled alongside the fetch: through
    /// the planning awaits and the cold downloads, and to its verdict
    /// before this returns. A link pass blocks the task and does not
    /// poll it.
    pub async fn run_with_check<Report: Reporter>(
        mut self,
        mut check: Option<ConcurrentCheck<'_>>,
    ) -> Result<CreateVirtualStoreOutput, CreateVirtualStoreError> {
        let result = if self.ctx.linker.kind == NodeLinker::Loaded {
            Box::pin(self.run_cas::<Report>(&mut check)).await
        } else {
            self.run_inner::<Report>(&mut check).await
        };
        settle_check(check, result).await
    }

    pub(super) async fn run_cas<Report: Reporter>(
        self,
        check: &mut Option<ConcurrentCheck<'_>>,
    ) -> Result<CreateVirtualStoreOutput, CreateVirtualStoreError> {
        validate_config(self.ctx.config)?;
        let mut selected = self.ctx
            .select_loaded_snapshots(self.entries.snapshots)
            .expect("loaded linker selection")
            .clone();
        let fetch_context = crate::InstallContext {
            linker: crate::ModuleLinkerContext { kind: NodeLinker::Hoisted, ..self.ctx.linker },
            ..self.ctx.clone()
        };
        let materialize_context = self.ctx;
        let mut installer = CreateVirtualStore { ctx: &fetch_context, ..self };
        let (mut fetched, prefetch) =
            installer.run_retaining::<Report>(installer.entries.snapshots, check).await?;
        let physical = prepare_loaded_packages(
            &mut selected,
            installer.entries.snapshots,
            installer.entries.packages,
            &mut fetched,
            installer.ctx,
        )?;
        installer.fetching.cas_prefetch = prefetch;
        let promoted = temporary_builds::build::<Report>(
            &mut installer,
            &mut selected,
            &mut fetched,
            &physical,
            check,
        )
        .await?;
        materialize_loaded::<Report>(
            installer,
            materialize_context,
            &selected,
            promoted,
            fetched,
            check,
        )
        .await
    }
}

pub(super) fn retain_native_addon_packages(
    selected: &mut HashMap<PackageKey, SnapshotEntry>,
    snapshots: Option<&HashMap<PackageKey, SnapshotEntry>>,
    fetched: &CreateVirtualStoreOutput,
) {
    let Some(snapshots) = snapshots else { return };
    let Some(cas_paths) = fetched.cas_paths_by_pkg_id.as_ref() else { return };
    let roots = snapshots
        .keys()
        .filter(|key| {
            cas_paths
                .get(&cas_paths_key(key))
                .is_some_and(|files| {
                    files.cas_paths
                        .keys()
                        .any(|path| path.ends_with(".node"))
                })
        })
        .cloned()
        .collect::<Vec<_>>();
    retain_dependency_closure(selected, snapshots, roots);
}

fn prepare_loaded_packages(
    selected: &mut HashMap<PackageKey, SnapshotEntry>,
    snapshots: Option<&HashMap<PackageKey, SnapshotEntry>>,
    packages: Option<&HashMap<PackageKey, pnpm_lockfile::PackageMetadata>>,
    fetched: &mut CreateVirtualStoreOutput,
    context: &crate::InstallContext<'_>,
) -> Result<HashSet<PackageKey>, CreateVirtualStoreError> {
    let mut cached_builds = cached_builds::retain(selected, snapshots, packages, fetched, context);
    fetched.cas_paths_by_snapshot = cached_build_maps(fetched, &cached_builds);
    retain_native_addon_packages(selected, snapshots, fetched);
    capabilities::retain_filesystem_packages(selected, snapshots, fetched)?;
    let physical = selected.keys().cloned().collect();
    retain_build_misses(selected, snapshots, fetched, &cached_builds);
    cached_builds.retain(|key, _| !selected.contains_key(key));
    fetched.cas_paths_by_snapshot = cached_build_maps(fetched, &cached_builds);
    validate_cas_builds(fetched, selected, &cached_builds, context.config)?;
    Ok(physical)
}

fn retain_build_misses(
    selected: &mut HashMap<PackageKey, SnapshotEntry>,
    snapshots: Option<&HashMap<PackageKey, SnapshotEntry>>,
    fetched: &CreateVirtualStoreOutput,
    cached: &HashMap<PackageKey, String>,
) {
    let Some(snapshots) = snapshots else { return };
    let roots = fetched.requires_build_by_snapshot
        .iter()
        .filter(|(key, requires_build)| **requires_build && !cached.contains_key(*key))
        .map(|(key, _)| key.clone())
        .collect();
    retain_dependency_closure(selected, snapshots, roots);
}

fn cached_build_maps(
    fetched: &CreateVirtualStoreOutput,
    cached: &HashMap<PackageKey, String>,
) -> Option<crate::CasPathsBySnapshot> {
    let mut maps = HashMap::new();
    for (key, cache_key) in cached {
        let Some(overlay) = fetched.side_effects.maps
            .get(key)
            .and_then(|overlays| overlays.get(cache_key))
        else {
            continue;
        };
        maps.insert(key.clone(), Arc::new(overlay.files.clone()));
    }
    (!maps.is_empty()).then_some(maps)
}

fn retain_dependency_closure(
    selected: &mut HashMap<PackageKey, SnapshotEntry>,
    snapshots: &HashMap<PackageKey, SnapshotEntry>,
    roots: Vec<PackageKey>,
) {
    let mut pending = roots;
    while let Some(key) = pending.pop() {
        if selected.contains_key(&key) {
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
        selected.insert(key, snapshot.clone());
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
        insert_downloaded_row(rows, downloaded, fetched, snapshot_key, cache_key);
    }
}

fn insert_downloaded_row(
    rows: &mut PrefetchResult,
    downloaded: &crate::CasPathsByPkgId,
    fetched: &CreateVirtualStoreOutput,
    snapshot_key: &PackageKey,
    cache_key: &String,
) {
    if rows.cas_paths.contains_key(cache_key) {
        return;
    }
    let Some(files) = immutable_download(downloaded, snapshot_key) else {
        return;
    };
    rows.cas_paths.insert(cache_key.clone(), Arc::clone(&files.cas_paths));
    if let Some(&requires_build) = fetched.requires_build_by_snapshot.get(snapshot_key) {
        rows.requires_build.insert(cache_key.clone(), requires_build);
    }
    if let Some(manifest) = fetched.package_manifests.get(&snapshot_key.without_peer()) {
        rows.manifests.insert(cache_key.clone(), Arc::clone(manifest));
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
    let pending: Vec<_> = snapshots
        .keys()
        .filter(|key| {
            config.node_linker_excluded.contains(&key.name.to_string())
                || crate::snapshot_has_patch(key)
                // A runtime's executables are native binaries, which the loader cannot run.
                || key.suffix.prefix() == Prefix::Runtime
        })
        .cloned()
        .collect();
    let mut selected = HashMap::new();
    retain_dependency_closure(&mut selected, snapshots, pending);
    selected
}

fn validate_cas_builds(
    fetched: &CreateVirtualStoreOutput,
    selected: &HashMap<PackageKey, SnapshotEntry>,
    cached: &HashMap<PackageKey, String>,
    config: &pnpm_config::Config,
) -> Result<(), CreateVirtualStoreError> {
    if config.ignore_scripts {
        return Ok(());
    }
    for (key, requires_build) in &fetched.requires_build_by_snapshot {
        if *requires_build && !selected.contains_key(key) && !cached.contains_key(key) {
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
    } else if config.node_package_map_mode.writes() {
        Some("nodePackageMap cannot be combined with the CAS loader")
    } else {
        None
    };
    match message {
        Some(message) => Err(CreateVirtualStoreError::CasConfiguration { message }),
        None => Ok(()),
    }
}

async fn materialize_loaded<Report: Reporter>(
    installer: CreateVirtualStore<'_>,
    context: &crate::InstallContext<'_>,
    selected: &HashMap<PackageKey, SnapshotEntry>,
    promoted: HashSet<PackageKey>,
    fetched: CreateVirtualStoreOutput,
    check: &mut Option<ConcurrentCheck<'_>>,
) -> Result<CreateVirtualStoreOutput, CreateVirtualStoreError> {
    let context = crate::InstallContext {
        linker: crate::ModuleLinkerContext { kind: NodeLinker::Isolated, ..context.linker },
        ..context.clone()
    };
    let pending = selected
        .iter()
        .filter(|(key, _)| !promoted.contains(*key))
        .map(|(key, snapshot)| (key.clone(), snapshot.clone()))
        .collect::<HashMap<_, _>>();
    let mut installer = CreateVirtualStore {
        ctx: &context,
        entries: pnpm_lockfile::LockfileEntries {
            snapshots: (!pending.is_empty()).then_some(&pending),
            ..installer.entries
        },
        ..installer
    };
    let mut materialized = installer.run_inner::<Report>(check).await?;
    materialized.snapshot_placement.materialized.extend(promoted);
    materialized.snapshot_placement.loaded = Some(selected.keys().cloned().collect());
    merge_fetched(&mut materialized, fetched, &pending);
    Ok(materialized)
}

/// Fold the fetch pass's output into the materialization pass's. The
/// build metadata of the snapshots outside `pending`, the loaded and
/// promoted ones, comes from the fetch pass. A pending snapshot keeps the
/// materialization pass's answer, which withholds the cached output of a
/// slot that must be rebuilt.
pub(super) fn merge_fetched(
    materialized: &mut CreateVirtualStoreOutput,
    fetched: CreateVirtualStoreOutput,
    pending: &HashMap<PackageKey, SnapshotEntry>,
) {
    materialized.cas_paths_by_pkg_id = fetched.cas_paths_by_pkg_id;
    materialized.cas_paths_by_snapshot = fetched.cas_paths_by_snapshot;
    materialized.package_manifests.extend(fetched.package_manifests);
    materialized.package_bins.extend(fetched.package_bins);
    materialized.fetch_failed.extend(fetched.fetch_failed);
    let not_pending = |key: &PackageKey| !pending.contains_key(key);
    materialized.requires_build_by_snapshot.extend(
        fetched.requires_build_by_snapshot.into_iter().filter(|(key, _)| not_pending(key)),
    );
    materialized.side_effects.maps.extend(
        fetched.side_effects.maps.into_iter().filter(|(key, _)| not_pending(key)),
    );
    materialized.side_effects.diffs.extend(
        fetched.side_effects.diffs.into_iter().filter(|(key, _)| not_pending(key)),
    );
}
