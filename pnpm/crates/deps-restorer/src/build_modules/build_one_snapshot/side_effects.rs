use super::{
    super::{
        BuildModulesError, HashMap, PackageKey, Path, PathBuf, Reporter, materialize_side_effects,
        store_index_key_for_resolution,
    },
    BuildCandidate, BuildOneSnapshot, global_slot_carries_overlay, report_broken_slot,
};
use std::sync::atomic::Ordering;

/// The side-effects cache key, computed once per snapshot before the
/// `is_built` gate. The same value is later consumed by the WRITE-path
/// upload after `run_postinstall_hooks` succeeds, so recomputing it
/// there would just duplicate work — `deps_state_cache` makes the second
/// call free anyway, but routing through one value keeps the gate-side
/// and write-side keys provably identical.
///
/// `None` when the cache gate can't fire (no engine, no graph, etc.);
/// both downstream consumers short-circuit on `None`.
///
/// The `deps_state_cache` is shared across all scheduled nodes via
/// `Mutex` because `calc_dep_state` is recursive and memoizes — a
/// per-task cache would defeat the memoization for diamond-shaped
/// subgraphs.
pub(super) fn side_effects_cache_key(
    context: &BuildOneSnapshot<'_>,
    snapshot_key: &PackageKey,
    candidate: &BuildCandidate<'_>,
) -> Option<String> {
    let (graph, engine) = context.dep_graph.zip(context.engine_name)?;
    // Poison-recover: `calc_dep_state` mutates the cache by
    // inserting one entry per recursive walk node, each
    // insert atomic from `HashMap`'s POV. A panic mid-walk
    // leaves the map in a usable state — the worst case is
    // an unfinished sub-walk that the next caller will redo.
    let mut cache_guard =
        context.deps_state_cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    Some(pnpm_graph_hasher::calc_dep_state(
        graph,
        &mut cache_guard,
        snapshot_key,
        &pnpm_graph_hasher::CalcDepStateOptions {
            engine_name: engine,
            // `None` for unpatched snapshots leaves the
            // `;patch=...` segment off the cache key entirely.
            patch_file_hash: candidate.patch.map(|patch| patch.hash.as_str()),
            // The deps-graph hash is included only when scripts
            // will run. A patched-only snapshot leaves it off so
            // the cache key stays stable across dep-graph changes
            // that don't affect this package's patched output.
            include_dep_graph_hash: candidate.should_run_scripts,
        },
    ))
}
/// Side-effects-cache `is_built` gate. Past the policy gate, this
/// snapshot would otherwise run its scripts — but if the prefetch
/// surfaced a matching side-effects-cache entry, the build is already
/// represented on disk (seeded on a previous install) and can be
/// skipped. An explicit `pacquet rebuild` (`force_rebuild`) always
/// re-runs the scripts, so it bypasses this gate.
pub(super) fn already_built<Reporter: self::Reporter>(
    context: &BuildOneSnapshot<'_>,
    snapshot_key: &PackageKey,
    candidate: &BuildCandidate<'_>,
    cache_key: Option<&str>,
) -> Result<bool, BuildModulesError> {
    if !candidate.force_rebuild
        && context.side_effects_cache
        && let Some(maps_by_snapshot) = context.side_effects_maps_by_snapshot
        && let Some(maps) = maps_by_snapshot.get(snapshot_key)
        && let Some(key) = cache_key
        && let Some(overlay) = maps.get(key)
    {
        return satisfy_from_side_effects_cache::<Reporter>(
            context,
            snapshot_key,
            (key, overlay),
            (&candidate.name, &candidate.version),
        );
    }
    Ok(false)
}
/// Whether a side-effects-cache hit already put this snapshot's build output
/// on disk, so the build can be skipped.
///
/// The warm link placed only the pristine tarball files in the project-local
/// slot. The cached build's output (the side-effects `added` / `deleted`
/// overlay) still has to land on disk before the build is skipped, or the
/// package is left in its pre-build state — e.g. a postinstall that downloads
/// a binary leaves nothing behind on the warm reinstall. The side-effects diff
/// is applied at import time.
pub(super) fn satisfy_from_side_effects_cache<Reporter: self::Reporter>(
    context: &BuildOneSnapshot<'_>,
    snapshot_key: &PackageKey,
    cached: (&str, &HashMap<String, PathBuf>),
    named: (&str, &str),
) -> Result<bool, BuildModulesError> {
    let (key, overlay) = cached;
    tracing::debug!(
        target: "pacquet::build",
        ?snapshot_key,
        cache_key = key,
        "side-effects cache hit; skipping build",
    );
    if global_slot_carries_overlay(context, snapshot_key, overlay) {
        return Ok(true);
    }
    // The overlay carries the patched / built contents, so it has to reach
    // every hoisted copy for the same reason patch application does.
    context.slot_mutations.store(true, Ordering::Relaxed);
    for pkg_dir in context.pkg_roots().all(snapshot_key) {
        // No slot to materialize into (skipped / never linked) — nothing for
        // the build phase to do either.
        if !pkg_dir.exists() {
            continue;
        }
        match materialize_overlay_into_slot::<Reporter>(context, &pkg_dir, overlay) {
            OverlayOutcome::Materialized => {}
            OverlayOutcome::Rebuild(error) => {
                tracing::warn!(
                    target: "pacquet::build",
                    ?snapshot_key,
                    cache_key = key,
                    %error,
                    "failed to materialize side-effects cache overlay; rebuilding",
                );
                return Ok(false);
            }
            OverlayOutcome::Broken(error) => {
                return report_broken_slot::<Reporter>(
                    context,
                    snapshot_key,
                    &pkg_dir,
                    named,
                    error,
                )
                .map(|()| true);
            }
        }
    }
    Ok(true)
}
/// What materializing a cached overlay into one slot left behind.
pub(super) enum OverlayOutcome {
    Materialized,
    /// Staging failed with the slot's base files intact, so the normal build
    /// path can re-run the script over them and re-seed the cache.
    ///
    /// Side-effects `added` blobs aren't re-verified (see
    /// [`pnpm_store_dir::build_file_maps_from_index`]), so a CAS blob deleted
    /// out from under the store surfaces here.
    Rebuild(BuildModulesError),
    /// A stage-and-swap failed mid-replace and left the slot without its base
    /// files. Rebuilding against that would run scripts on an incomplete dir
    /// (or skip them when the manifest is gone) and let the install finish
    /// with a broken package.
    Broken(BuildModulesError),
}
pub(super) fn materialize_overlay_into_slot<Reporter: self::Reporter>(
    context: &BuildOneSnapshot<'_>,
    pkg_dir: &Path,
    overlay: &HashMap<String, PathBuf>,
) -> OverlayOutcome {
    match materialize_side_effects::<Reporter>(
        context.logged_methods,
        context.import_method,
        pkg_dir,
        overlay,
    ) {
        Ok(()) => OverlayOutcome::Materialized,
        Err(error) if pkg_dir.join("package.json").exists() => OverlayOutcome::Rebuild(error),
        Err(error) => OverlayOutcome::Broken(error),
    }
}
/// The writes a frozen store would have to make into its read-only slot.
pub(super) struct FrozenStoreWrites {
    pub(super) optional: bool,
    pub(super) has_patch: bool,
    pub(super) should_run_scripts: bool,
}
/// What the side-effects-cache write path uploads.
pub(super) struct SideEffectsUpload<'a> {
    pub(super) metadata_key: &'a PackageKey,
    pub(super) pkg_dir: &'a Path,
    pub(super) cache_key: Option<&'a str>,
    pub(super) patch: Option<&'a pnpm_patching::ExtendedPatchInfo>,
    pub(super) is_patched: bool,
    pub(super) has_side_effects: bool,
}
/// Side-effects-cache WRITE path. After a successful `run_postinstall_hooks`
/// (or a patch application that mutated the dir), re-hash the package
/// directory and queue a `PackageFilesIndex.sideEffects[cache_key] = diff`
/// mutation so a future install can skip the rebuild.
///
/// A frozen store short-circuits before `upload`: its disabled index writer
/// drops queued rows, but `upload` writes CAFS files before queuing them.
/// Otherwise a patched-only snapshot still uploads its post-patch state so
/// subsequent installs hit the cache.
///
/// The other preconditions: cache key composable (engine + graph present),
/// `packages` map available for store-index key selection, and the resolution
/// has a store-backed key.
///
/// All errors are swallowed with a `tracing::warn!`. A failed upload doesn't
/// fail the install: the next install re-runs the build.
pub(super) fn upload_side_effects_cache(
    context: &BuildOneSnapshot<'_>,
    snapshot_key: &PackageKey,
    upload: &SideEffectsUpload<'_>,
) {
    if (!upload.is_patched && !upload.has_side_effects) || context.frozen_store {
        return;
    }
    let (Some(writer), Some(store), Some(cache_key), Some(packages)) =
        (context.store_index_writer, context.store_dir, upload.cache_key, context.packages)
    else {
        return;
    };
    let Some(metadata) = packages.get(upload.metadata_key) else { return };
    let publishes_remotely = upload.has_side_effects
        && context
            .shared_side_effects_publisher
            .is_some_and(|publisher| publisher.can_publish(upload.metadata_key, metadata));
    if !context.side_effects_cache_write && !publishes_remotely {
        return;
    }
    let Some(files_index_file) = store_index_key_for_resolution(
        &metadata.resolution,
        &upload.metadata_key.pkg_id(),
        !context.ignore_scripts,
    ) else {
        return;
    };
    let uploaded = upload_and_publish(
        context,
        snapshot_key,
        (store, writer, &files_index_file, cache_key),
        (upload, metadata),
    );
    if let Err(err) = uploaded {
        tracing::warn!(
            target: "pacquet::build",
            ?err,
            dep_path = %snapshot_key,
            "side-effects cache upload failed; build proceeds",
        );
    }
}
pub(super) fn upload_and_publish(
    context: &BuildOneSnapshot<'_>,
    snapshot_key: &PackageKey,
    store: (
        &pnpm_store_dir::StoreDir,
        &std::sync::Arc<pnpm_store_dir::StoreIndexWriter>,
        &str,
        &str,
    ),
    uploaded: (&SideEffectsUpload<'_>, &pnpm_lockfile::PackageMetadata),
) -> Result<(), pnpm_store_dir::UploadError> {
    let (store, writer, files_index_file, cache_key) = store;
    let (upload, metadata) = uploaded;
    let Some(publisher) = context.shared_side_effects_publisher else {
        return pnpm_store_dir::upload(store, upload.pkg_dir, files_index_file, cache_key, writer);
    };
    let diff = pnpm_store_dir::upload_with_diff(
        store,
        upload.pkg_dir,
        files_index_file,
        cache_key,
        writer,
    )?;
    if upload.has_side_effects
        && let Some(diff) = diff
        && let Some(graph) = context.dep_graph
        && let Err(error) = publisher.publish(
            snapshot_key,
            metadata,
            graph,
            upload.patch.map(|patch| patch.hash.as_str()),
            diff,
            store,
        )
    {
        tracing::warn!(
            target: "pacquet::build",
            dep_path = %snapshot_key,
            %error,
            "remote side-effects publication failed; build proceeds",
        );
    }
    Ok(())
}
