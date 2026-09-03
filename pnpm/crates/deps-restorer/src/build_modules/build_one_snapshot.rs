//! Running one package's build scripts.

use std::sync::atomic::{AtomicBool, Ordering};

use super::{
    AllowBuildPolicy, BTreeSet, BuildModulesError, HashMap, LogEvent, LogLevel, Mutex,
    NEEDS_BUILD_MARKER, PackageImportMethod, PackageKey, Path, PathBuf, RebuildOptions, Reporter,
    RunPostinstallHooks, ScriptsPrependNodePath, SkippedOptionalDependencyLog,
    SkippedOptionalPackage, SkippedOptionalReason, SnapshotEntry,
    allow_build_key_from_ignored_build, apply_patch_to_dir, bin_dirs_in_all_parent_dirs,
    discard_failed_global_virtual_store_slot, get_pkg_id_with_patch_hash, materialize_side_effects,
    parse_name_version_from_key, pkg_root_for_key, pkg_roots_for_key, run_postinstall_hooks,
    slot_carries_overlay, store_index_key_for_resolution,
};

/// Per-snapshot build work, called once per ready node by the
/// bounded-parallelism scheduler in
/// [`crate::build_modules::BuildModules::run`].
#[expect(
    clippy::too_many_arguments,
    reason = "the parameters are independent inputs; bundling them into a struct would not improve clarity"
)]
pub(crate) fn build_one_snapshot<Reporter: self::Reporter>(
    snapshot_key: &PackageKey,
    snapshots: &HashMap<PackageKey, SnapshotEntry>,
    packages: Option<&HashMap<PackageKey, pnpm_lockfile::PackageMetadata>>,
    patches: Option<&HashMap<PackageKey, pnpm_patching::ExtendedPatchInfo>>,
    requires_build_map: &HashMap<PackageKey, bool>,
    allow_build_policy: &AllowBuildPolicy,
    side_effects_maps_by_snapshot: Option<&crate::SideEffectsMapsBySnapshot>,
    engine_name: Option<&str>,
    side_effects_cache: bool,
    side_effects_cache_write: bool,
    shared_side_effects_publisher: Option<&crate::shared_side_effects::SharedSideEffectsPublisher>,
    store_dir: Option<&pnpm_store_dir::StoreDir>,
    store_index_writer: Option<&std::sync::Arc<pnpm_store_dir::StoreIndexWriter>>,
    dep_graph: Option<&HashMap<PackageKey, pnpm_graph_hasher::DepsGraphNode<PackageKey>>>,
    deps_state_cache: &Mutex<pnpm_graph_hasher::DepsStateCache<PackageKey>>,
    ignored_builds: &Mutex<BTreeSet<String>>,
    layout: &crate::VirtualStoreLayout,
    pkg_roots_by_key: Option<&HashMap<PackageKey, Vec<PathBuf>>>,
    gather_ancestor_bin_paths: bool,
    modules_dir: &Path,
    lockfile_dir: &Path,
    extra_env: &HashMap<String, String>,
    user_agent: &str,
    scripts_prepend_node_path: ScriptsPrependNodePath,
    script_shell: Option<&Path>,
    shell_emulator: bool,
    unsafe_perm: bool,
    frozen_store: bool,
    ignore_scripts: bool,
    import_method: PackageImportMethod,
    logged_methods: &std::sync::atomic::AtomicU8,
    // Raised before any write that can change a linked slot's contents
    // (side-effects overlay, patch, lifecycle script) — set
    // pre-attempt, so a half-applied write still counts. See
    // `BuildModulesOutput::mutated_slots`.
    slot_mutations: &AtomicBool,
    rebuild: Option<&RebuildOptions>,
) -> Result<(), BuildModulesError> {
    let metadata_key = snapshot_key.without_peer();
    // Look up against the peer-stripped key because patches are
    // configured at the (name, version) granularity in
    // `pnpm-workspace.yaml`, not per peer-resolution variant.
    let patch = patches.and_then(|map| map.get(&metadata_key));
    let has_patch = patch.is_some();
    let requires_build = requires_build_map.get(snapshot_key).copied().unwrap_or(false);

    // Ancestors of a build/patch candidate are included in the
    // sequence (so the topo order stays correct) but only run
    // scripts / apply patches when they themselves are candidates.
    if !requires_build && !has_patch {
        return Ok(());
    }

    let dep_path = metadata_key.to_string();
    let (name, version) = parse_name_version_from_key(&dep_path);

    // An explicit `pacquet rebuild` re-runs the build scripts of the
    // selected packages even when the side-effects cache reports them
    // already built; `force_rebuild` marks those so they bypass the
    // `is_built` gate below. The selection holds allow-build keys (the
    // package name for registry deps, the full pkgId for git/tarball
    // artifacts), so match either form — a selected non-registry artifact
    // is forced past the gate too. The allow-policy gate still applies — a
    // rebuild never builds a disallowed package. Non-selected
    // packages still run the allow-policy gate below (so their
    // `.modules.yaml` ignored-builds record stays intact), but their
    // scripts are suppressed by the rebuild-selection gate after it.
    let force_rebuild = rebuild.is_some_and(|rebuild| {
        rebuild.is_selected(&name)
            || rebuild.is_selected(&allow_build_key_from_ignored_build(&dep_path))
    });

    // The allowBuilds gate only applies when the node has scripts to
    // run. A patched-only package skips this check entirely and
    // proceeds to patch application below.
    //
    // `false` / `None` from the policy set `should_run_scripts =
    // false` (NOT early-return), so the patch still gets applied
    // even when scripts are disallowed.
    let mut should_run_scripts = requires_build && !ignore_scripts;
    if should_run_scripts {
        match allow_build_policy.check(&dep_path) {
            Some(false) => {
                should_run_scripts = false;
            }
            None => {
                // Poison-recover: see the equivalent call site at
                // the end of `BuildModules::run` for the safety
                // argument (BTreeSet insertion is atomic from the
                // data-structure's POV).
                // The patch hash is kept: two copies of a package that
                // differ only by an applied patch are different builds to
                // approve, and pnpm's `dedupePackageNamesFromIgnoredBuilds`
                // reports them apart for the same reason. `dep_path` above
                // has already lost it, so re-derive from the full key.
                let ignored_key = get_pkg_id_with_patch_hash(&snapshot_key.to_string()).to_string();
                ignored_builds
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .insert(ignored_key);
                should_run_scripts = false;
            }
            Some(true) => {}
        }
    }

    // A `pacquet rebuild <pkg>` runs scripts only for the selected
    // packages. Non-selected packages were still evaluated by the policy
    // gate above (so their ignored-builds state is recorded), but their
    // scripts are suppressed here. The side-effects `is_built` gate below
    // is only an optimization and is disabled by default, so this gate —
    // not that short-circuit — is what bounds script execution to the
    // selection.
    if rebuild.is_some() && !force_rebuild {
        should_run_scripts = false;
    }

    // Compute the side-effects cache key once per snapshot, before
    // the `is_built` gate. The same value is later consumed by the
    // WRITE-path upload call after `run_postinstall_hooks`
    // succeeds, so recomputing it there would just duplicate work —
    // `deps_state_cache` makes the second call free anyway, but
    // routing through one `let` keeps the gate-side and write-side
    // keys provably identical.
    //
    // `None` when the cache gate can't fire (no engine, no graph,
    // etc.); both downstream consumers short-circuit on `None`.
    //
    // The `deps_state_cache` is shared across all scheduled nodes via
    // `Mutex` because `calc_dep_state` is recursive and memoizes —
    // a per-task cache would defeat the memoization for
    // diamond-shaped subgraphs.
    let cache_key = (dep_graph.zip(engine_name)).map(|(graph, engine)| {
        // Poison-recover: `calc_dep_state` mutates the cache by
        // inserting one entry per recursive walk node, each
        // insert atomic from `HashMap`'s POV. A panic mid-walk
        // leaves the map in a usable state — the worst case is
        // an unfinished sub-walk that the next caller will redo.
        let mut cache_guard =
            deps_state_cache.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        pnpm_graph_hasher::calc_dep_state(
            graph,
            &mut cache_guard,
            snapshot_key,
            &pnpm_graph_hasher::CalcDepStateOptions {
                engine_name: engine,
                // `None` for unpatched snapshots leaves the
                // `;patch=...` segment off the cache key entirely.
                patch_file_hash: patch.map(|patch| patch.hash.as_str()),
                // The deps-graph hash is included only when scripts
                // will run. A patched-only snapshot leaves it off so
                // the cache key stays stable across dep-graph changes
                // that don't affect this package's patched output.
                include_dep_graph_hash: should_run_scripts,
            },
        )
    });

    // Side-effects-cache `is_built` gate. We're already past the
    // policy gate, so this snapshot would otherwise run its scripts
    // — but if the prefetch surfaced a matching side-effects-cache
    // entry, the build is already represented on disk (seeded on a
    // previous install) and we can skip. An explicit `pacquet rebuild`
    // (`force_rebuild`) always re-runs the scripts, so it bypasses
    // this gate.
    if !force_rebuild
        && side_effects_cache
        && let Some(maps_by_snapshot) = side_effects_maps_by_snapshot
        && let Some(maps) = maps_by_snapshot.get(snapshot_key)
        && let Some(key) = cache_key.as_deref()
        && let Some(overlay) = maps.get(key)
    {
        tracing::debug!(
            target: "pacquet::build",
            ?snapshot_key,
            cache_key = key,
            "side-effects cache hit; skipping build",
        );
        // The warm link placed only the pristine tarball files in the
        // project-local slot. The cached build's output (the
        // side-effects `added` / `deleted` overlay) still has to land on
        // disk before the build is skipped, or the package is left in its
        // pre-build state — e.g. a postinstall that downloads a binary
        // leaves nothing behind on the warm reinstall. The side-effects
        // diff is applied at import time.
        //
        // Skip under the global virtual store: there the slot persists
        // inside the store with its build output already on disk (a cache
        // hit *is* that seeded slot), so there is nothing to re-link —
        // and the slot is read-only under `frozen_store`, where a write
        // would fail with `EROFS`.
        //
        // A materialization failure is usually *not* fatal. Side-effects
        // `added` blobs aren't re-verified (see
        // [`pnpm_store_dir::build_file_maps_from_index`]), so a CAS
        // blob deleted out from under the store surfaces here as an
        // import error. That failure happens while staging the new
        // contents, before the existing slot is touched, so the pristine
        // files are still on disk: treat it as a cache miss and fall
        // through to the normal build path below, which re-runs the script
        // over the intact files and re-seeds the cache.
        //
        // The one case that must *not* silently fall through is a
        // stage-and-swap that failed mid-replace and left the slot without
        // its base files. Rebuilding against that would run scripts on an
        // incomplete dir (or skip them when the manifest is gone) and let
        // the install finish with a broken package. When the manifest is
        // missing after a failed materialization, skip an optional
        // dependency (as for any optional build failure) and surface a
        // hard error otherwise.
        //
        // Under the global virtual store the slot usually *is* the seeded
        // build — it persists inside the store across installs — so the
        // overlay is already on disk and re-linking it would be pure
        // overhead. That only holds while the slot survives, though: a
        // failed build discards it
        // ([`discard_failed_global_virtual_store_slot`]), and a prune or a
        // manual removal can too. The store index keeps the side-effects
        // row either way, so the next install re-imports the slot pristine
        // and still hits the cache. Trusting the hit there would skip the
        // build and leave the package unbuilt, so the slot has to be
        // checked rather than assumed.
        let gvs_slot_already_seeded = layout.enable_global_virtual_store()
            && pkg_root_for_key(layout, pkg_roots_by_key, snapshot_key)
                .is_some_and(|pkg_dir| slot_carries_overlay(&pkg_dir, overlay));

        let satisfied_by_cache = if gvs_slot_already_seeded {
            true
        } else {
            // The overlay carries the patched / built contents, so it
            // has to reach every hoisted copy for the same reason patch
            // application does.
            slot_mutations.store(true, Ordering::Relaxed);
            let mut satisfied = true;
            for pkg_dir in pkg_roots_for_key(layout, pkg_roots_by_key, snapshot_key) {
                // No slot to materialize into (skipped / never linked) —
                // nothing for the build phase to do either.
                if !pkg_dir.exists() {
                    continue;
                }
                match materialize_side_effects::<Reporter>(
                    logged_methods,
                    import_method,
                    &pkg_dir,
                    overlay,
                ) {
                    Ok(()) => {}
                    Err(error) if pkg_dir.join("package.json").exists() => {
                        tracing::warn!(
                            target: "pacquet::build",
                            ?snapshot_key,
                            cache_key = key,
                            %error,
                            "failed to materialize side-effects cache overlay; rebuilding",
                        );
                        satisfied = false;
                        break;
                    }
                    Err(error) => {
                        if snapshots.get(snapshot_key).is_some_and(|entry| entry.optional) {
                            Reporter::emit(&LogEvent::SkippedOptionalDependency(
                                SkippedOptionalDependencyLog {
                                    level: LogLevel::Debug,
                                    details: Some(error.to_string()),
                                    package: SkippedOptionalPackage::Installed {
                                        id: pkg_dir.to_string_lossy().into_owned(),
                                        name,
                                        version,
                                    },
                                    parents: None,
                                    prefix: lockfile_dir.to_string_lossy().into_owned(),
                                    reason: SkippedOptionalReason::BuildFailure,
                                },
                            ));
                            return Ok(());
                        }
                        return Err(error);
                    }
                }
            }
            satisfied
        };
        if satisfied_by_cache {
            return Ok(());
        }
    }

    let optional = snapshots.get(snapshot_key).is_some_and(|entry| entry.optional);

    // Frozen-store backstop. Under the global virtual store the slot
    // directory lives inside the read-only store, so applying a patch
    // or running an approved lifecycle script (the two writes below)
    // would fail with a raw `EROFS`. Refuse up front with guidance.
    // We're past the `is_built` gate, so a cached build has already
    // returned — reaching here means the seed is genuinely missing
    // this package's build output.
    // Bin-linking (the other write) reuses existing symlinks
    // write-free on a complete seed, so only patch/script writes gate.
    if frozen_store && layout.enable_global_virtual_store() && (has_patch || should_run_scripts) {
        if optional {
            // A build/patch failure on an optional dependency is non-fatal
            // (see the lifecycle-script arm below), so a seed missing an
            // optional package's build output skips that build instead of
            // blocking the install.
            Reporter::emit(&LogEvent::SkippedOptionalDependency(SkippedOptionalDependencyLog {
                level: LogLevel::Debug,
                details: Some(format!(
                    "The read-only store (frozenStore) is missing the build output of {name}@{version}.",
                )),
                package: SkippedOptionalPackage::Installed {
                    id: pkg_root_for_key(layout, pkg_roots_by_key, snapshot_key).map_or_else(
                        || snapshot_key.to_string(),
                        |dir| dir.to_string_lossy().into_owned(),
                    ),
                    name,
                    version,
                },
                parents: None,
                prefix: lockfile_dir.to_string_lossy().into_owned(),
                reason: SkippedOptionalReason::BuildFailure,
            }));
            return Ok(());
        }
        return Err(BuildModulesError::FrozenStoreNeedsBuild {
            package: format!("{name}@{version}"),
        });
    }

    // Hoisted snapshots without a recorded `pkgRoot` (the walker
    // dropped them — pre-skipped, optional skip, etc.) take the
    // same exit as the isolated path's `!pkg_dir.exists()` skip.
    let Some(pkg_dir) = pkg_root_for_key(layout, pkg_roots_by_key, snapshot_key) else {
        return Ok(());
    };
    if !pkg_dir.exists() {
        return Ok(());
    }

    // Per-snapshot `extra_bin_paths`. Isolated leaves it empty;
    // hoisted gathers every ancestor's `node_modules/.bin` up to
    // `lockfile_dir` so a lifecycle script invoked at a nested
    // hoisted location can resolve bins added by parents.
    let extra_bin_paths: Vec<PathBuf> = if gather_ancestor_bin_paths {
        bin_dirs_in_all_parent_dirs(&pkg_dir, lockfile_dir)
    } else {
        Vec::new()
    };

    // Apply the patch before running postinstall hooks. A snapshot
    // with a patch entry but no resolved `patch_file_path` is a hard
    // error (`PatchFilePathMissing`).
    // `is_patched` feeds the cache-write gate below
    // (`is_patched || has_side_effects`).
    let is_patched = if let Some(p) = patch {
        let patch_file_path = p.patch_file_path.as_deref().ok_or_else(|| {
            BuildModulesError::PatchFilePathMissing { dep_path: snapshot_key.to_string() }
        })?;
        // Every copy is patched, not just `pkg_dir`. Under the hoisted
        // linker a version conflict nests further copies under their
        // consumers; leaving those unpatched would silently run the very
        // code the patch replaces.
        slot_mutations.store(true, Ordering::Relaxed);
        for patched_dir in pkg_roots_for_key(layout, pkg_roots_by_key, snapshot_key) {
            if !patched_dir.exists() {
                continue;
            }
            apply_patch_to_dir(&patched_dir, patch_file_path)
                .inspect_err(|_| discard_failed_global_virtual_store_slot(layout, snapshot_key))
                .map_err(BuildModulesError::PatchApply)?;
        }
        true
    } else {
        false
    };

    let has_side_effects = if should_run_scripts {
        slot_mutations.store(true, Ordering::Relaxed);
        let result = run_postinstall_hooks::<Reporter>(&RunPostinstallHooks {
            dep_path: &snapshot_key.to_string(),
            pkg_root: &pkg_dir,
            root_modules_dir: modules_dir,
            init_cwd: lockfile_dir,
            extra_bin_paths: &extra_bin_paths,
            extra_env,
            node_execpath: None,
            npm_execpath: None,
            node_gyp_path: None,
            user_agent: Some(user_agent),
            unsafe_perm,
            node_gyp_bin: pnpm_executor::bundled_node_gyp_bin(),
            scripts_prepend_node_path,
            script_shell,
            shell_emulator,
            optional,
        });

        match result {
            Ok(ran) => ran,
            Err(err) => {
                // Before the optional-skip return, so a failed optional
                // build leaves no half-built slot behind either.
                discard_failed_global_virtual_store_slot(layout, snapshot_key);
                if optional {
                    Reporter::emit(&LogEvent::SkippedOptionalDependency(
                        SkippedOptionalDependencyLog {
                            level: LogLevel::Debug,
                            details: Some(err.to_string()),
                            package: SkippedOptionalPackage::Installed {
                                id: pkg_dir.to_string_lossy().into_owned(),
                                name,
                                version,
                            },
                            parents: None,
                            prefix: lockfile_dir.to_string_lossy().into_owned(),
                            reason: SkippedOptionalReason::BuildFailure,
                        },
                    ));
                    return Ok(());
                }
                return Err(BuildModulesError::LifecycleScript(err));
            }
        }
    } else {
        false
    };

    let built_in_isolated_gvs = layout.enable_global_virtual_store() && pkg_roots_by_key.is_none();
    if built_in_isolated_gvs && (has_patch || should_run_scripts) {
        for built_dir in pkg_roots_for_key(layout, pkg_roots_by_key, snapshot_key) {
            if let Err(error) = std::fs::remove_file(built_dir.join(NEEDS_BUILD_MARKER))
                && error.kind() != std::io::ErrorKind::NotFound
            {
                tracing::warn!(
                    target: "pacquet::build",
                    ?error,
                    dep_path = %snapshot_key,
                    "failed to remove the global virtual store build marker",
                );
            }
        }
    }

    // Side-effects-cache WRITE path. After a successful
    // `run_postinstall_hooks` (or a patch application that mutated
    // the dir), re-hash the package directory and queue a
    // `PackageFilesIndex.sideEffects[cache_key] = diff` mutation
    // so a future install can skip the rebuild.
    //
    // A frozen store short-circuits before `upload`: its disabled index writer
    // drops queued rows, but `upload` writes CAFS files before queuing them.
    // Otherwise a patched-only snapshot still uploads its post-patch state so
    // subsequent installs hit the cache.
    //
    // The other preconditions: cache_key composable (engine + graph
    // present), `packages` map available for store-index key selection,
    // and the resolution has a store-backed key.
    //
    // All errors are swallowed with a `tracing::warn!`. A failed
    // upload doesn't fail the install: the next install re-runs the
    // build.
    if (is_patched || has_side_effects)
        && !frozen_store
        && let Some(writer) = store_index_writer
        && let Some(store) = store_dir
        && let Some(cache_key) = cache_key.as_deref()
        && let Some(packages) = packages
        && let Some(metadata) = packages.get(&metadata_key)
        && (side_effects_cache_write
            || (has_side_effects
                && shared_side_effects_publisher
                    .is_some_and(|publisher| publisher.can_publish(&metadata_key, metadata))))
        && let Some(files_index_file) = store_index_key_for_resolution(
            &metadata.resolution,
            &metadata_key.pkg_id(),
            !ignore_scripts,
        )
        && let Err(err) = (|| {
            if let Some(publisher) = shared_side_effects_publisher {
                let diff = pnpm_store_dir::upload_with_diff(
                    store,
                    &pkg_dir,
                    &files_index_file,
                    cache_key,
                    writer,
                )?;
                if has_side_effects
                    && let Some(diff) = diff
                    && let Some(graph) = dep_graph
                    && let Err(error) = publisher.publish(
                        snapshot_key,
                        metadata,
                        graph,
                        patch.map(|patch| patch.hash.as_str()),
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
            } else {
                pnpm_store_dir::upload(store, &pkg_dir, &files_index_file, cache_key, writer)
            }
        })()
    {
        tracing::warn!(
            target: "pacquet::build",
            ?err,
            dep_path = %snapshot_key,
            "side-effects cache upload failed; build proceeds",
        );
    }

    Ok(())
}
