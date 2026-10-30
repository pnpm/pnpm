use super::{
    CreateVirtualStore, CreateVirtualStoreError, CreateVirtualStoreOutput, immutable_download,
    materialized_snapshots, prepare_loaded_packages, retain_dependency_closure,
};
mod build;
mod capture;
mod promote;

use crate::ConcurrentCheck;
use pnpm_config::NodeLinker;
use pnpm_lockfile::{LockfileEntries, PackageKey, SnapshotEntry};
use pnpm_reporter::Reporter;
use std::collections::{HashMap, HashSet};

type Snapshots = HashMap<PackageKey, SnapshotEntry>;

pub(super) async fn build<Report: Reporter>(
    installer: &mut CreateVirtualStore<'_>,
    selected: &mut Snapshots,
    fetched: &mut CreateVirtualStoreOutput,
    physical: &HashSet<PackageKey>,
    check: &mut Option<ConcurrentCheck<'_>>,
) -> Result<HashSet<PackageKey>, CreateVirtualStoreError> {
    let config = installer.ctx.config;
    if config.ignore_scripts
        || config.frozen_store
        || !config.side_effects_cache_read()
        || !config.side_effects_cache_write()
    {
        return Ok(HashSet::new());
    }
    let temporary = candidates(installer, selected, fetched, physical)?;
    if temporary.is_empty() {
        return Ok(HashSet::new());
    }
    if let Some(pending) = check.take()
        && matches!(pending.await, crate::CheckVerdict::Abort)
    {
        return Err(CreateVirtualStoreError::ConcurrentCheckAborted);
    }
    let root = tempfile::Builder::new()
        .prefix("pnpm-loaded-build-")
        .tempdir()
        .map_err(|source| CreateVirtualStoreError::LoadedBuildIo {
            path: std::env::temp_dir(),
            source,
        })?;
    Box::pin(build_in::<Report>(installer, selected, fetched, &temporary, root.path())).await
}

async fn build_in<Report: Reporter>(
    installer: &mut CreateVirtualStore<'_>,
    selected: &mut Snapshots,
    fetched: &mut CreateVirtualStoreOutput,
    temporary: &Snapshots,
    root: &std::path::Path,
) -> Result<HashSet<PackageKey>, CreateVirtualStoreError> {
    let layout = installer.ctx.linker.layout.relocated(root);
    let context = crate::InstallContext {
        linker: crate::ModuleLinkerContext {
            kind: NodeLinker::Isolated,
            layout: &layout,
            ..installer.ctx.linker
        },
        ..installer.ctx.clone()
    };
    let (materialized, prefetch) = materialize::<Report>(installer, temporary, &context).await?;
    installer.fetching.cas_prefetch = prefetch;
    let captured = crate::CapturedBuildSideEffects::default();
    let output = build::run::<Report>(installer, temporary, &context, &materialized, &captured)?;
    capture::apply(installer, &context, &output.mutated_snapshot_keys, captured, fetched)?;
    selected.clear();
    selected.extend(materialized_snapshots(installer.ctx.config, installer.entries.snapshots));
    prepare_loaded_packages(
        selected,
        installer.entries.snapshots,
        installer.entries.packages,
        fetched,
        installer.ctx,
    )?;
    selected.retain(|key, _| !fetched.fetch_failed.contains(key));
    promote::retain(installer, &layout, temporary, selected)
}

fn candidates(
    installer: &CreateVirtualStore<'_>,
    selected: &Snapshots,
    fetched: &CreateVirtualStoreOutput,
    physical: &HashSet<PackageKey>,
) -> Result<Snapshots, CreateVirtualStoreError> {
    let Some(snapshots) = installer.entries.snapshots else { return Ok(HashMap::new()) };
    let mut temporary = HashMap::new();
    for key in selected
        .keys()
        .filter(|key| fetched.requires_build_by_snapshot.get(*key) == Some(&true))
    {
        if !installer.ctx.config.force
            && crate::build_modules::slots::built_slot_is_complete(
                &crate::build_modules::slots::virtual_store_dir_for_key(
                    installer.ctx.linker.layout,
                    key,
                ),
            )
            .map_err(CreateVirtualStoreError::LoadedBuild)?
        {
            continue;
        }
        let mut closure = HashMap::new();
        retain_dependency_closure(&mut closure, snapshots, vec![key.clone()]);
        if closure
            .keys()
            .any(|key| {
                physical.contains(key)
                    || fetched.cas_paths_by_pkg_id
                        .as_ref()
                        .and_then(|files| immutable_download(files, key))
                        .is_none()
                    || (fetched.requires_build_by_snapshot.get(key) == Some(&true)
                        && (installer.ctx.allow_build_policy.check(
                            &key.without_peer().to_string(),
                        ) != Some(true)
                            || !installer.ctx.allow_build_policy.caches_build(key)))
            })
        {
            continue;
        }
        temporary.extend(closure);
    }
    Ok(temporary)
}

async fn materialize<Report: Reporter>(
    installer: &mut CreateVirtualStore<'_>,
    snapshots: &Snapshots,
    context: &crate::InstallContext<'_>,
) -> Result<(CreateVirtualStoreOutput, Option<crate::CasPrefetch>), CreateVirtualStoreError> {
    let mut temporary = CreateVirtualStore {
        fetching: crate::VirtualStoreFetchInputs {
            http_client: installer.fetching.http_client,
            store_index_writer: installer.fetching.store_index_writer,
            store_context: None,
            cas_prefetch: installer.fetching.cas_prefetch.take(),
            progress_reported: installer.fetching.progress_reported,
            tarball_mem_cache: installer.fetching.tarball_mem_cache,
            custom_fetcher_session: installer.fetching.custom_fetcher_session,
            planned_canonical_fetches: None,
        },
        ctx: context,
        entries: LockfileEntries { snapshots: Some(snapshots), ..installer.entries },
        current_entries: LockfileEntries::default(),
        importers: installer.importers,
        selection: installer.selection,
        dir_clone_cache: None,
        #[cfg(test)]
        link_concurrency_probe: None,
    };
    temporary.run_retaining::<Report>(installer.entries.snapshots, &mut None).await
}
