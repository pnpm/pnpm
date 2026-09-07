use super::{
    BTreeMap, Config, HashSet, HoistedDependencies, IncludedDependencies, InstallError,
    LayoutVersion, Lockfile, Modules, ModulesNodeLinker, NodeLinker, PNPM_VERSION, PackageManifest,
    Path, VersionPart, write_modules_manifest,
};

/// Translate pacquet's [`Config::node_linker`] into the
/// [`pnpm_modules_yaml::NodeLinker`] enum used on disk. The two
/// enums share the same variant set (`isolated`, `hoisted`, `pnp`),
/// the values of the `nodeLinker` string.
pub(super) fn map_node_linker(linker: NodeLinker) -> ModulesNodeLinker {
    match linker {
        NodeLinker::Isolated => ModulesNodeLinker::Isolated,
        NodeLinker::Hoisted => ModulesNodeLinker::Hoisted,
        NodeLinker::Pnp => ModulesNodeLinker::Pnp,
    }
}

/// Whether a parsed `.modules.yaml` records the same layout settings
/// (`nodeLinker`, hoist patterns, store / virtual-store paths,
/// `virtualStoreDirMaxLength`, included dep groups, layout version) the
/// current install would produce. A mismatch disqualifies the no-op
/// short-circuit.
///
/// Takes the already-parsed [`Modules`] so the up-to-date fast path can
/// share one parse across the consistency, newly-allowed, and
/// unapproved-ignored checks.
pub(super) fn modules_consistent_with(
    modules: &pnpm_modules_yaml::ModulesLayout,
    config: &Config,
    node_linker: NodeLinker,
    included: IncludedDependencies,
) -> bool {
    // A `virtualStoreOnly` install populates the virtual store and stops,
    // so the modules directory it leaves behind has no importer symlinks,
    // bins, or hoisted packages. It can never satisfy an ordinary
    // install, however well its recorded settings line up — the no-op
    // short-circuit would leave the linking permanently undone.
    if modules.virtual_store_only == Some(true) && !config.virtual_store_only {
        return false;
    }
    modules.included == included && modules_layout_consistent_with(modules, config, node_linker)
}

/// The subset of [`modules_consistent_with`] that, when it drifts, requires
/// **wiping and recreating** `node_modules`. It deliberately excludes
/// `included`: a `--prod`<->full switch is satisfied by relinking the
/// newly-selected groups plus the targeted removal of the now-excluded
/// ones ([`crate::prune_direct_deps_excluded_by_groups`]), not by
/// deleting the directory. pnpm never purges the root project's
/// `node_modules` for an included mismatch — its `validateModules` only
/// does so for non-root importers (the `lockfileDir !== rootDir` check
/// in `pnpm11/installing/deps-installer/src/install/validateModules.ts`)
/// — so purging here would destroy the user's own non-pnpm entries (a
/// vendored directory, stray files) on a routine flag change. The
/// up-to-date fast path still compares `included` via
/// [`modules_consistent_with`], so the relink it triggers stays correct.
/// On-disk probe backing the frozen no-op short-circuit: the
/// short-circuit skips the materialization walk entirely, so it must
/// first prove the tree it would skip is still whole — pnpm's headless
/// path stats every package dir on every run, which is what repairs a
/// hand-deleted package. One metadata call per snapshot slot plus one
/// per direct-dep link; any missing entry falls through to the full
/// frozen path, which re-materializes it (emitting
/// `pnpm:_broken_node_modules`).
///
/// Under a global virtual store the slot paths depend on graph hashes
/// the short-circuit doesn't compute, and the hoisted linker has no
/// virtual-store slots; both probe only the importer links.
pub(super) fn frozen_tree_intact(
    wanted: &Lockfile,
    modules: &pnpm_modules_yaml::ModulesLayout,
    config: &Config,
    workspace_root: &Path,
    node_linker: NodeLinker,
) -> bool {
    if matches!(node_linker, NodeLinker::Pnp) && !workspace_root.join(crate::PNP_FILENAME).is_file()
    {
        return false;
    }
    let skipped = crate::SkippedSnapshots::from_strings(&modules.skipped);
    let probe_slots =
        !matches!(node_linker, NodeLinker::Hoisted) && !config.enable_global_virtual_store;
    if probe_slots && let Some(snapshots) = wanted.snapshots.as_ref() {
        let layout = crate::VirtualStoreLayout::legacy(
            config.virtual_store_dir.clone(),
            config.virtual_store_dir_max_length as usize,
        );
        let all_slots_present = snapshots.keys().all(|key| {
            if skipped.contains(key) {
                return true;
            }
            // The name is lockfile-controlled: join it with the same
            // traversal-rejecting helper the linkers use, and treat a
            // malformed name as not-intact so the full path's
            // structural lockfile gate rejects it.
            let slot_node_modules = layout.slot_dir(key).join("node_modules");
            match crate::safe_join_modules_dir::safe_join_modules_dir(
                &slot_node_modules,
                &key.name.to_string(),
            ) {
                Ok(dir) => dir.is_dir(),
                Err(_) => false,
            }
        });
        if !all_slots_present {
            return false;
        }
    }
    if !config.symlink {
        return probe_slots;
    }
    let groups = crate::prune_direct_deps::selected_groups(modules.included);
    let modules_dir_name: &std::ffi::OsStr =
        config.modules_dir.file_name().unwrap_or_else(|| std::ffi::OsStr::new("node_modules"));
    wanted.importers.iter().all(|(importer_id, snapshot)| {
        if crate::symlink_direct_dependencies::validate_importer_id(importer_id).is_err() {
            return true;
        }
        let modules_dir =
            crate::symlink_direct_dependencies::importer_root_dir(workspace_root, importer_id)
                .join(modules_dir_name);
        crate::symlink_direct_dependencies::direct_dep_names_for_importer(
            snapshot,
            groups.iter().copied(),
            &skipped,
            false,
        )
        .iter()
        .all(|name| {
            match crate::safe_join_modules_dir::safe_join_modules_dir(&modules_dir, name) {
                // `metadata` follows the link, so a dangling direct-dep
                // symlink (a wiped GVS store, a hand-deleted target)
                // reads as broken and falls through to the repairing
                // full path.
                Ok(link) => std::fs::metadata(link).is_ok(),
                // A malformed alias never probes the disk; the full
                // path rejects it with its own typed error.
                Err(_) => true,
            }
        })
    })
}

/// Whether a GVS install can own slots whose interrupted build or patch
/// application must be recovered from `.pnpm-needs-build`.
///
/// The marker is shared store state, so neither optimistic workspace state nor
/// the frozen importer's symlinks can prove it absent. Only configurations
/// capable of acting on one need to leave those no-op paths.
pub(super) fn gvs_build_markers_may_require_recovery(config: &Config) -> bool {
    config.enable_global_virtual_store
        && (config.dangerously_allow_all_builds
            || config.allow_builds.values().any(|allowed| *allowed)
            || config.patched_dependencies.as_ref().is_some_and(|patches| !patches.is_empty()))
}

/// Probe the buildable or patched GVS slots this lockfile resolves to.
/// Markers in sibling hash directories belong to other dependency graphs and
/// cannot be recovered by materializing this one. The effective Node version
/// participates only when materialization would run installability checks;
/// constraint-free materialization keys the layout to the detected host Node.
pub(super) fn gvs_build_marker_present(
    wanted: &Lockfile,
    config: &Config,
    lockfile_dir: &Path,
    effective_node_version: Option<&str>,
) -> bool {
    if !gvs_build_markers_may_require_recovery(config) {
        return false;
    }
    let Ok(policy) = crate::AllowBuildPolicy::from_config(config) else {
        return true;
    };
    let Some(snapshots) = wanted.snapshots.as_ref() else {
        return false;
    };
    let eligible_snapshots = snapshots
        .keys()
        .filter(|snapshot_key| {
            crate::snapshot_has_patch(snapshot_key)
                || policy.check(&snapshot_key.without_peer().to_string()) == Some(true)
        })
        .collect::<Vec<_>>();
    let mut marker_candidate = false;
    let mut visited_version_dirs = HashSet::new();
    for &snapshot_key in &eligible_snapshots {
        let metadata = wanted
            .packages
            .as_ref()
            .and_then(|packages| packages.get(&snapshot_key.without_peer()));
        let Some(version_dir) = crate::global_virtual_store_version_dir(
            &config.global_virtual_store_dir,
            snapshot_key,
            metadata,
        ) else {
            return true;
        };
        if !visited_version_dirs.insert(version_dir.clone()) {
            continue;
        }
        let hash_dirs = match std::fs::read_dir(version_dir) {
            Ok(hash_dirs) => hash_dirs,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return true,
        };
        for hash_dir in hash_dirs {
            let Ok(hash_dir) = hash_dir else {
                return true;
            };
            let Ok(file_type) = hash_dir.file_type() else {
                return true;
            };
            if !file_type.is_dir() {
                continue;
            }
            let Ok(pkg_dir) = crate::safe_join_modules_dir::safe_join_modules_dir(
                &hash_dir.path().join("node_modules"),
                &snapshot_key.name.to_string(),
            ) else {
                return true;
            };
            if pkg_dir.join(crate::NEEDS_BUILD_MARKER).is_file() {
                marker_candidate = true;
                break;
            }
        }
        if marker_candidate {
            break;
        }
    }
    if !marker_candidate {
        return false;
    }
    let effective_node_version = match (&wanted.snapshots, &wanted.packages) {
        (Some(snapshots), Some(packages))
            if !config.force
                && !snapshots.is_empty()
                && crate::any_installability_constraint(snapshots, packages) =>
        {
            effective_node_version
        }
        _ => None,
    };
    let layout = crate::virtual_store_layout_for_lockfile(
        config,
        effective_node_version,
        wanted.snapshots.as_ref(),
        wanted.packages.as_ref(),
        Some(&policy),
        Some(lockfile_dir),
    );
    if crate::validate_virtual_store_slot_containment(wanted.snapshots.as_ref(), &layout).is_err() {
        return true;
    }

    for snapshot_key in eligible_snapshots {
        let Ok(pkg_dir) = crate::safe_join_modules_dir::safe_join_modules_dir(
            &layout.slot_dir(snapshot_key).join("node_modules"),
            &snapshot_key.name.to_string(),
        ) else {
            return true;
        };
        if pkg_dir.join(crate::NEEDS_BUILD_MARKER).is_file() {
            return true;
        }
    }
    false
}

/// The `validateModules` half pacquet enforces: when the mutation is
/// not a plain install (upstream `installsOnly === false`), a drift in
/// the persisted layout settings fails with the upstream `*_DIFF`
/// error instead of silently recreating the modules directory. Check
/// order matches upstream `validateModules`. Drift in the fields this
/// does not cover (store dir, node linker, layout version) still takes
/// the recreate path.
pub(super) fn check_modules_settings_diff(
    modules: &pnpm_modules_yaml::ModulesLayout,
    config: &Config,
) -> Result<(), InstallError> {
    if modules.virtual_store_dir_max_length != config.virtual_store_dir_max_length {
        return Err(InstallError::VirtualStoreDirMaxLengthDiff);
    }
    if normalized_pattern(modules.public_hoist_pattern.as_deref())
        != normalized_pattern(config.public_hoist_pattern.as_deref())
    {
        return Err(InstallError::PublicHoistPatternDiff);
    }
    if normalized_pattern(modules.hoist_pattern.as_deref())
        != normalized_pattern(config.hoist_pattern.as_deref())
    {
        return Err(InstallError::HoistPatternDiff);
    }
    Ok(())
}

/// Upstream compares patterns with `?? []`: `None` and an empty list
/// are the same disabled state.
pub(super) fn normalized_pattern(pattern: Option<&[String]>) -> &[String] {
    pattern.unwrap_or(&[])
}

pub(super) fn modules_layout_consistent_with(
    modules: &pnpm_modules_yaml::ModulesLayout,
    config: &Config,
    node_linker: NodeLinker,
) -> bool {
    // A `virtualStoreOnly` install (`pnpm fetch`) records empty hoist
    // patterns because it deliberately did no hoisting. Diffing those
    // against the follow-up install's real patterns would read as drift
    // and purge the directory the fetch just populated, so the
    // comparison is skipped and the follow-up completes the linking
    // instead.
    // Patterns compare normalized (upstream's `?? []`): `None` and an
    // empty list are the same disabled state, so the pair must not read
    // as layout drift — a purge every install for `hoistPattern: []`
    // projects, and a spurious `*_DIFF` error for `add` / `remove`. A
    // `virtualStoreOnly` install records empty patterns deliberately, so
    // it skips the comparison entirely and lets the follow-up install
    // complete the linking instead of purging.
    let hoist_patterns_match = modules.virtual_store_only == Some(true)
        || (normalized_pattern(modules.hoist_pattern.as_deref())
            == normalized_pattern(config.hoist_pattern.as_deref())
            && normalized_pattern(modules.public_hoist_pattern.as_deref())
                == normalized_pattern(config.public_hoist_pattern.as_deref()));
    modules.layout_version == Some(LayoutVersion)
        && modules.node_linker == Some(map_node_linker(node_linker))
        && hoist_patterns_match
        && modules.virtual_store_dir_max_length == config.virtual_store_dir_max_length
        && modules.store_dir == config.store_dir.display().to_string()
        && modules.virtual_store_dir
            == config.effective_virtual_store_dir().to_string_lossy().as_ref()
}

/// Whether `.modules.yaml` records any ignored build that the current
/// `allowBuilds` policy now allows.
///
/// When `true`, the frozen no-op fast path must not short-circuit: the
/// install has to rebuild the newly-allowed package, re-running the
/// builds an `allowBuilds` change un-ignored even on an otherwise
/// up-to-date install. pacquet achieves this by letting the full frozen
/// install run, whose `BuildModules` re-evaluates the policy and
/// rebuilds the now-allowed package (already built deps are skipped by
/// the side-effects-cache `is_built` gate).
pub(super) fn has_newly_allowed_ignored_builds(
    modules: &pnpm_modules_yaml::ModulesLayout,
    config: &Config,
) -> bool {
    let Some(ignored) = modules.ignored_builds.as_ref().filter(|set| !set.is_empty()) else {
        return false;
    };
    // A malformed `allowBuilds` can't be evaluated here; let the full
    // install run so it surfaces the real error instead of silently
    // staying on the fast path.
    let Ok(policy) = crate::AllowBuildPolicy::from_config(config) else {
        return true;
    };
    ignored.iter().any(|dep_path| policy.check(dep_path.as_str()) == Some(true))
}

/// Whether the current `allowBuilds` policy withdraws an approval that
/// `.modules.yaml` recorded, leaving the package undecided again.
///
/// The counterpart to [`has_newly_allowed_ignored_builds`]: a build the
/// previous install ran is absent from `ignoredBuilds`, so nothing else
/// on the frozen no-op fast path notices it is no longer approved
/// (<https://github.com/pnpm/pnpm/issues/11035>).
///
/// Only a withdrawal to *undecided* counts. An entry the user flipped to
/// an explicit `false` is silently skipped rather than reported, so it
/// leaves the fast path intact — matching `BuildModules`.
pub(super) fn has_revoked_allowed_builds(
    modules: &pnpm_modules_yaml::ModulesLayout,
    config: &Config,
) -> bool {
    let Some(recorded) = modules.allow_builds.as_ref() else { return false };
    recorded
        .iter()
        .filter(|(_, value)| matches!(value, pnpm_modules_yaml::AllowBuildValue::Bool(true)))
        .any(|(spec, _)| !config.allow_builds.contains_key(spec))
}

/// The sorted `name@version` keys `.modules.yaml` recorded as ignored
/// builds that the current `allowBuilds` policy still leaves unapproved
/// (`None`), or `None` when there are none.
///
/// The up-to-date fast paths use this to keep `strictDepBuilds`
/// enforced across reruns: `ignoredBuilds` is seeded from `.modules.yaml`
/// on the up-to-date path and the ignored-builds check still throws, so a
/// rerun after an `ERR_PNPM_IGNORED_BUILDS` failure must not exit 0.
/// Packages a later policy explicitly denies (`Some(false)`) are excluded
/// — those are silently skipped, never reported — matching a full
/// install's `BuildModules`. Newly-allowed packages are handled
/// by [`has_newly_allowed_ignored_builds`], which skips the fast path.
///
/// A malformed `allowBuilds` spec surfaces as `Err` (e.g.
/// `ERR_PNPM_INVALID_VERSION_UNION`) rather than being swallowed: the
/// fast-path callers fall through to the full install on `Err`, which
/// re-evaluates the policy and reports the real error.
pub(super) fn unapproved_recorded_ignored_builds(
    modules: &pnpm_modules_yaml::ModulesLayout,
    config: &Config,
) -> Result<Option<Vec<String>>, pnpm_config::version_policy::VersionPolicyError> {
    let Some(ignored) = modules.ignored_builds.as_ref().filter(|set| !set.is_empty()) else {
        return Ok(None);
    };
    let policy = crate::AllowBuildPolicy::from_config(config)?;
    let mut names: Vec<String> = ignored
        .iter()
        .filter(|dep_path| policy.check(dep_path.as_str()).is_none())
        .map(|dep_path| dep_path.as_str().to_string())
        .collect();
    names.sort();
    Ok((!names.is_empty()).then_some(names))
}

/// Assemble the [`Modules`] payload for [`write_modules_manifest`].
///
/// `hoistedDependencies` is produced by the isolated-linker hoist
/// pass in [`crate::InstallFrozenLockfile::run`] and threaded in
/// here — empty for the no-lockfile path, for installs where both
/// hoist patterns are `None`, and under `nodeLinker: hoisted` (the
/// hoisted linker uses `hoisted_locations` instead). Persisting it
/// lets a subsequent install detect a hoist pattern change and
/// re-hoist appropriately (the partial-install path tracked at
/// pnpm/pacquet#433 will consume it; today every install does the
/// full hoist anyway).
///
/// `hoisted_locations` is the per-depPath list of lockfile-relative
/// directory paths the hoisted linker placed each package at. Empty
/// for the isolated linker (the field is hoisted-only on disk and
/// only meaningful when `nodeLinker: hoisted`). Persisted into
/// [`Modules::hoisted_locations`] when non-empty so the next
/// install's walker can short-circuit re-fetching packages already
/// present on disk and the rebuild path can locate every hoisted
/// directory; absent persistence is what surfaces the
/// `MISSING_HOISTED_LOCATIONS` error during rebuild.
///
/// `skipped` is the depPath list of skipped snapshots: each
/// [`PackageKey`] in the install-time
/// [`crate::SkippedSnapshots`] becomes one string entry; ordering is
/// handled by [`write_modules_manifest`]'s sort-on-write. An empty set
/// produces an empty list — matching the fresh-install case.
///
/// [`PackageKey`]: pnpm_lockfile::PackageKey
/// [`write_modules_manifest`]: pnpm_modules_yaml::write_modules_manifest
#[expect(
    clippy::too_many_arguments,
    reason = "assembles every field of the .modules.yaml manifest from the install's resolved state"
)]
pub(super) fn build_modules_manifest(
    config: &Config,
    node_linker: NodeLinker,
    included: IncludedDependencies,
    hoisted_dependencies: HoistedDependencies,
    hoisted_locations: BTreeMap<String, Vec<String>>,
    injected_deps: BTreeMap<String, Vec<String>>,
    skipped: &crate::SkippedSnapshots,
    ignored_builds: &[String],
    pending_builds: Vec<String>,
    pruned_at: String,
) -> Modules {
    Modules {
        // The `name@version` keys whose build scripts were blocked, so a
        // later install can re-run any that an `allowBuilds` change now
        // allows (see [`has_newly_allowed_ignored_builds`]). `None` when
        // empty, matching pnpm's omit-when-empty encoding.
        ignored_builds: (!ignored_builds.is_empty()).then(|| {
            ignored_builds.iter().cloned().map(pnpm_modules_yaml::DepPath::from).collect()
        }),
        hoist_pattern: config.hoist_pattern.clone(),
        hoisted_dependencies,
        // `Some(empty)` would round-trip on disk as
        // `hoistedLocations: {}`; the field is unset when empty. Drop it
        // when empty so an isolated install doesn't produce a
        // hoisted-only key.
        hoisted_locations: (!hoisted_locations.is_empty()).then_some(hoisted_locations),
        // Per-source-project virtual-store copies of injected `file:`
        // deps (see [`crate::collect_injected_deps`]). Omitted when
        // empty, matching pnpm's omit-when-empty encoding.
        injected_deps: (!injected_deps.is_empty()).then_some(injected_deps),
        included,
        layout_version: Some(LayoutVersion),
        node_linker: Some(map_node_linker(node_linker)),
        // `${name}@${version}`, where the name is the CLI's published
        // npm name. `pacquet` is an in-repo crate name that never
        // reaches disk, and the crate version is not the release
        // version.
        package_manager: format!("pnpm@{PNPM_VERSION}"),
        pending_builds,
        public_hoist_pattern: config.public_hoist_pattern.clone(),
        // RFC 1123 / `toUTCString()` format. The caller decides whether
        // this is a fresh timestamp (a prune ran or first install) or the
        // preserved prior value.
        pruned_at,
        // `iter_installability` excludes fetch-failure entries so they
        // don't get persisted across installs — optional fetch failures
        // are silently swallowed.
        skipped: skipped.iter_installability().map(ToString::to_string).collect(),
        store_dir: config.store_dir.display().to_string(),
        virtual_store_dir: config.effective_virtual_store_dir().to_string_lossy().into_owned(),
        virtual_store_dir_max_length: config.virtual_store_dir_max_length,
        // The build-approval set this install ran under. A GVS install
        // hashes engine-specific slots for allowed builders, so the
        // recorded set is what a later install diffs against to decide
        // whether its slots need re-linking.
        allow_builds: Some(
            config
                .allow_builds
                .iter()
                .map(|(spec, allowed)| {
                    (spec.clone(), pnpm_modules_yaml::AllowBuildValue::Bool(*allowed))
                })
                .collect(),
        ),
        virtual_store_only: config.virtual_store_only.then_some(true),
        ..Default::default()
    }
}

/// Drop `settled` from the `pendingBuilds` the install just wrote, now
/// that the projects' scripts have run.
///
/// A project's debt outlives the `.modules.yaml` write — its scripts run
/// after it — so clearing the record there would forget the debt when a
/// script fails. Re-reading rather than reusing the in-memory value
/// keeps every other field exactly as it was written.
pub(super) fn drain_settled_projects<Sys>(
    modules_dir: &Path,
    settled: &[String],
) -> Result<(), InstallError>
where
    Sys: pnpm_modules_yaml::FsReadToString
        + pnpm_modules_yaml::Clock
        + pnpm_modules_yaml::FsCreateDirAll
        + pnpm_modules_yaml::FsWrite,
{
    if settled.is_empty() {
        return Ok(());
    }
    let Some(mut modules) = pnpm_modules_yaml::read_modules_manifest::<Sys>(modules_dir)
        .map_err(InstallError::ReadModules)?
    else {
        return Ok(());
    };
    let before = modules.pending_builds.len();
    modules.pending_builds.retain(|entry| !settled.contains(entry));
    if modules.pending_builds.len() == before {
        return Ok(());
    }
    write_modules_manifest::<Sys>(modules_dir, modules).map_err(InstallError::WriteModules)
}

/// Includes the executor's implicit `node-gyp rebuild` fallback when a
/// project has `binding.gyp` but no explicit preinstall or install script.
pub(super) fn project_requires_lifecycle_scripts(
    project_dir: &Path,
    manifest: &PackageManifest,
) -> bool {
    let has_lifecycle_script = pnpm_executor::PROJECT_LIFECYCLE_STAGES
        .iter()
        .any(|stage| matches!(manifest.script(stage, true), Ok(Some(_))));
    has_lifecycle_script
        || (matches!(manifest.script("preinstall", true), Ok(None))
            && matches!(manifest.script("install", true), Ok(None))
            && project_dir.join("binding.gyp").exists())
}

/// The `pendingBuilds` list for this install: the builds still owed,
/// carried-over entries first, then the ones this install deferred.
///
/// A build stays owed until something runs it, so a carried-over entry
/// survives unless its subject left the current lockfile or this run is
/// the `pnpm rebuild` that discharged it.
pub(super) fn merge_pending_builds<Deferred>(
    previous: &[String],
    deferred: Deferred,
    current: Option<&Lockfile>,
    rebuild: Option<&crate::RebuildOptions>,
    rebuild_build_policy: Option<&crate::AllowBuildPolicy>,
) -> Vec<String>
where
    Deferred: IntoIterator<Item = String>,
{
    // An importer id and a dep path are both plain strings on disk, so
    // the current lockfile's `importers` — not the shape of the string —
    // decides which one an entry is.
    //
    // Only dependencies are settled here: the build phase has already
    // run by the time this file is written, while a project's scripts
    // run after it. `drain_settled_projects` discharges those once they
    // have actually succeeded. A dependency is settled only when the
    // rebuild both selected it and was allowed to build it — a selected
    // package the policy still blocks stays owed, matching pnpm's "drop
    // only what was actually rebuilt".
    let settled = |entry: &str| {
        let (Some(rebuild), Some(policy)) = (rebuild, rebuild_build_policy) else { return false };
        !current.is_some_and(|current| current.importers.contains_key(entry))
            && rebuild.settles_dependency(entry)
            && policy.check(pnpm_deps_path::remove_suffix(entry)) == Some(true)
    };
    let retained = previous.iter().filter(|entry| {
        current.is_some_and(|current| current_contains_dep_path(current, entry)) && !settled(entry)
    });
    let mut seen = HashSet::new();
    retained.cloned().chain(deferred).filter(|entry| seen.insert(entry.clone())).collect()
}

pub(super) fn merge_filtered_modules_metadata(
    next: &mut Modules,
    previous: &Modules,
    current: &Lockfile,
    selected: &Lockfile,
) {
    for (dep_path, aliases) in &previous.hoisted_dependencies {
        if !retained_only_dep_path(current, selected, dep_path) {
            continue;
        }
        let retained_aliases = next.hoisted_dependencies.entry(dep_path.clone()).or_default();
        for (alias, kind) in aliases {
            retained_aliases.entry(alias.clone()).or_insert(*kind);
        }
    }
    if let Some(previous_locations) = previous.hoisted_locations.as_ref() {
        for (dep_path, locations) in previous_locations {
            if !retained_only_dep_path(current, selected, dep_path) {
                continue;
            }
            let retained_locations = next.hoisted_locations.get_or_insert_default();
            let retained = retained_locations.entry(dep_path.clone()).or_default();
            for location in locations {
                if !retained.contains(location) {
                    retained.push(location.clone());
                }
            }
        }
    }
    let new_pending_builds = std::mem::take(&mut next.pending_builds);
    for dep_path in &previous.pending_builds {
        if retained_only_dep_path(current, selected, dep_path)
            && !next.pending_builds.contains(dep_path)
        {
            next.pending_builds.push(dep_path.clone());
        }
    }
    for dep_path in new_pending_builds {
        if !next.pending_builds.contains(&dep_path) {
            next.pending_builds.push(dep_path);
        }
    }
    let new_ignored_builds = next.ignored_builds.take();
    if let Some(previous_ignored) = previous.ignored_builds.as_ref() {
        for dep_path in previous_ignored {
            if retained_only_dep_path(current, selected, dep_path.as_str()) {
                let retained_ignored = next.ignored_builds.get_or_insert_default();
                retained_ignored.insert(dep_path.clone());
            }
        }
    }
    if let Some(new_ignored_builds) = new_ignored_builds
        && !new_ignored_builds.is_empty()
    {
        next.ignored_builds.get_or_insert_default().extend(new_ignored_builds);
    }
    let new_skipped = std::mem::take(&mut next.skipped);
    for dep_path in &previous.skipped {
        if retained_only_dep_path(current, selected, dep_path) && !next.skipped.contains(dep_path) {
            next.skipped.push(dep_path.clone());
        }
    }
    for dep_path in new_skipped {
        if !next.skipped.contains(&dep_path) {
            next.skipped.push(dep_path);
        }
    }
    // A source the selected install re-materialized has its targets
    // recomputed in `next`, so the previous file's targets for it are
    // stale — a bumped injected dep moves to a new virtual-store slot and
    // the old one is gone. Only sources no selected importer touched carry
    // their previous targets forward.
    let current_injected_sources = injected_source_paths(current);
    let selected_injected_sources = injected_source_paths(selected);
    if let Some(previous_injected) = previous.injected_deps.as_ref() {
        for (source, targets) in previous_injected {
            if current_injected_sources.contains(source)
                && !selected_injected_sources.contains(source)
            {
                let retained_injected = next.injected_deps.get_or_insert_default();
                retained_injected.entry(source.clone()).or_insert_with(|| targets.clone());
            }
        }
    }
}

pub(super) fn retained_only_dep_path(
    current: &Lockfile,
    selected: &Lockfile,
    dep_path: &str,
) -> bool {
    current_contains_dep_path(current, dep_path) && !current_contains_dep_path(selected, dep_path)
}

pub(super) fn injected_source_paths(lockfile: &Lockfile) -> HashSet<String> {
    lockfile
        .snapshots
        .iter()
        .flat_map(|snapshots| snapshots.keys())
        .chain(lockfile.packages.iter().flat_map(|packages| packages.keys()))
        .filter_map(|key| match key.suffix.version() {
            VersionPart::File(path) => Some(path.strip_prefix("./").unwrap_or(path).to_string()),
            VersionPart::Semver(_)
            | VersionPart::NonSemver(_)
            | VersionPart::RegistryQualified { .. } => None,
        })
        .collect()
}

pub(super) fn current_contains_dep_path(current: &Lockfile, dep_path: &str) -> bool {
    if current.importers.contains_key(dep_path) {
        return true;
    }
    let Ok(key) = dep_path.parse::<pnpm_lockfile::PackageKey>() else { return false };
    current.snapshots.as_ref().is_some_and(|snapshots| snapshots.contains_key(&key))
        || current
            .packages
            .as_ref()
            .is_some_and(|packages| packages.contains_key(&key.without_peer()))
}

/// Read a string field off a project manifest, returning `None` when
/// the field is missing or not a JSON string. Pnpm tolerates either
/// shape — `name`/`version` are advisory metadata in this context, so
/// pacquet matches by silently dropping non-string values.
pub(super) fn manifest_string_field(manifest: &PackageManifest, key: &str) -> Option<String> {
    manifest.value().get(key).and_then(|v| v.as_str()).map(ToString::to_string)
}
