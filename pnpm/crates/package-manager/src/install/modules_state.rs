pub(crate) use integrity::frozen_tree_intact;

pub(super) use build_markers::{gvs_build_marker_present, gvs_build_markers_may_require_recovery};
pub(super) use merge_metadata::{
    current_contains_dep_path, merge_filtered_modules_metadata, merge_pending_builds,
};

mod build_markers;

mod merge_metadata;

mod integrity;

use super::{
    BTreeMap, Config, HoistedDependencies, Host, IncludedDependencies, InstallError, LayoutVersion,
    Lockfile, Modules, ModulesNodeLinker, NodeLinker, PNPM_VERSION, PackageManifest, Path, PathBuf,
    write_modules_manifest,
};
use pnpm_cmd_shim::bin_dir_is_relocatable;
use rayon::prelude::*;

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

/// Whether a tree that moved with its project can be reused at all: only on
/// unix, where directory links are relative symlinks rather than junctions,
/// and only for an isolated or hoisted tree outside a global virtual store,
/// whose links point into a store that registers projects by their path.
pub(crate) fn tree_may_move(config: &Config, node_linker: NodeLinker) -> bool {
    cfg!(unix) && !config.enable_global_virtual_store && node_linker != NodeLinker::Pnp
}

/// Whether a tree that moved with its project keeps working where it is now:
/// [`tree_may_move`], and every importer, hoist, virtual-store slot and
/// hoisted-package `.bin` holds only bins that name their paths relative to
/// themselves, inside the directory holding `config.modules_dir`.
pub(crate) fn moved_tree_is_reusable(
    config: &Config,
    node_linker: NodeLinker,
    project_manifests: &[(PathBuf, &PackageManifest)],
    lockfile: &Lockfile,
) -> bool {
    let Some(root) = config.modules_dir.parent() else { return false };
    tree_may_move(config, node_linker)
        && importer_bins_are_relocatable(config, project_manifests, root)
        && match node_linker {
            NodeLinker::Hoisted => hoisted_bins_are_relocatable(config, root),
            _ => virtual_store_bins_are_relocatable(config, lockfile, root),
        }
}

fn importer_bins_are_relocatable(
    config: &Config,
    project_manifests: &[(PathBuf, &PackageManifest)],
    root: &Path,
) -> bool {
    let modules_dir_name: &std::ffi::OsStr =
        config.modules_dir.file_name().unwrap_or_else(|| std::ffi::OsStr::new("node_modules"));
    bin_dir_is_relocatable(&config.modules_dir.join(".bin"), root)
        && project_manifests
            .iter()
            .filter(|(project_dir, _)| project_dir != root)
            .all(|(project_dir, _)| {
                bin_dir_is_relocatable(&project_dir.join(modules_dir_name).join(".bin"), root)
            })
}

/// The bins of the packages hoisted into the virtual store, and each slot's
/// own.
fn virtual_store_bins_are_relocatable(config: &Config, lockfile: &Lockfile, root: &Path) -> bool {
    let layout = crate::VirtualStoreLayout::legacy(
        config.virtual_store_dir.clone(),
        config.virtual_store_dir_max_length as usize,
    );
    bin_dir_is_relocatable(&config.virtual_store_dir.join("node_modules").join(".bin"), root)
        && lockfile.snapshots
            .as_ref()
            .is_none_or(|snapshots| {
                snapshots
                    .par_iter()
                    .all(|(key, _)| {
                        slot_bins_are_relocatable(
                            &layout.slot_dir(key).join("node_modules"),
                            key,
                            root,
                        )
                    })
            })
}

/// The `.bin` dirs the hoisted linker writes inside the packages it placed,
/// enumerated from the `hoistedLocations` the install recorded. A record that
/// cannot be read, or a location naming a directory outside the tree, refuses
/// the move.
fn hoisted_bins_are_relocatable(config: &Config, root: &Path) -> bool {
    let Ok(Some(modules)) = pnpm_modules_yaml::read_modules_manifest::<Host>(&config.modules_dir)
    else {
        return false;
    };
    modules.hoisted_locations
        .iter()
        .flat_map(BTreeMap::values)
        .flatten()
        .all(|location| {
            let package_dir = root.join(location);
            pnpm_fs::is_subdir(root, &package_dir)
                && bin_dir_is_relocatable(&package_dir.join("node_modules").join(".bin"), root)
        })
}

/// The `.bin` the install links into the slot's package, and the one next
/// to the package, which the injected-deps syncer links a package's own
/// bins into.
fn slot_bins_are_relocatable(
    slot_modules_dir: &Path,
    key: &pnpm_lockfile::PackageKey,
    root: &Path,
) -> bool {
    bin_dir_is_relocatable(&slot_modules_dir.join(".bin"), root)
        // A malformed lockfile-controlled name fails closed.
        && crate::safe_join_modules_dir::safe_join_modules_dir(
            slot_modules_dir,
            &key.name.to_string(),
        )
        .is_ok_and(|package_dir| {
            bin_dir_is_relocatable(&package_dir.join("node_modules").join(".bin"), root)
        })
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

/// The subset of [`modules_consistent_with`] that, when it drifts, requires
/// **wiping and recreating** `node_modules`. It deliberately excludes
/// `included`: a `--prod`<->full switch is satisfied by relinking the
/// newly-selected groups plus the targeted removal of the now-excluded
/// ones ([`crate::prune_direct_deps_excluded_by_groups`]), not by
/// deleting the directory. pnpm never purges the root project's
/// `node_modules` for an included mismatch: its `validateModules` only
/// does so for non-root importers (the `lockfileDir !== rootDir` check
/// in `pnpm11/installing/deps-installer/src/install/validateModules.ts`)
/// so purging here would destroy the user's own non-pnpm entries (a
/// vendored directory, stray files) on a routine flag change. The
/// up-to-date fast path still compares `included` via
/// [`modules_consistent_with`], so the relink it triggers stays correct.
pub(crate) fn modules_layout_consistent_with(
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
            == config
                .effective_virtual_store_dir()
                .to_string_lossy()
                .as_ref()
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
    let Some(ignored) = modules.ignored_builds
        .as_ref()
        .filter(|set| !set.is_empty())
    else {
        return false;
    };
    // A malformed `allowBuilds` can't be evaluated here; let the full
    // install run so it surfaces the real error instead of silently
    // staying on the fast path.
    let Ok(policy) = crate::AllowBuildPolicy::from_config(config) else {
        return true;
    };
    ignored
        .iter()
        .any(|dep_path| policy.check(dep_path.as_str()) == Some(true))
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
/// an explicit `false` is silently skipped rather than reported, matching
/// `BuildModules`; [`recorded_allow_builds_differ`] is what sees that
/// transition.
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

/// Whether the `allowBuilds` entries the previous install recorded differ
/// from the current setting: an entry flipped between `true` and `false`,
/// or one added or removed. The two predicates above see an ignored build
/// becoming allowed and an approval being withdrawn; this sees the
/// remaining transitions, such as an explicit `false` becoming `true`,
/// which leaves no ignored entry behind to notice. Placeholder entries the
/// approval scaffold writes carry no decision and are ignored.
pub(super) fn recorded_allow_builds_differ(
    modules: &pnpm_modules_yaml::ModulesLayout,
    config: &Config,
) -> bool {
    let recorded = || {
        modules.allow_builds
            .iter()
            .flatten()
            .filter_map(|(spec, value)| match value {
                pnpm_modules_yaml::AllowBuildValue::Bool(decision) => {
                    Some((spec.as_str(), decision))
                }
                pnpm_modules_yaml::AllowBuildValue::String(_) => None,
            })
    };
    recorded().count() != config.allow_builds.len()
        || recorded().any(|(spec, decision)| config.allow_builds.get(spec) != Some(decision))
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
    let Some(ignored) = modules.ignored_builds
        .as_ref()
        .filter(|set| !set.is_empty())
    else {
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
            ignored_builds
                .iter()
                .cloned()
                .map(pnpm_modules_yaml::DepPath::from)
                .collect()
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
        skipped: skipped
            .iter_installability()
            .map(ToString::to_string)
            .collect(),
        store_dir: config.store_dir.display().to_string(),
        virtual_store_dir: config
            .effective_virtual_store_dir()
            .to_string_lossy()
            .into_owned(),
        virtual_store_dir_max_length: config.virtual_store_dir_max_length,
        // The build-approval set this install ran under. A GVS install
        // hashes engine-specific slots for allowed builders, so the
        // recorded set is what a later install diffs against to decide
        // whether its slots need re-linking.
        allow_builds: Some(
            config.allow_builds
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

/// Read a string field off a project manifest, returning `None` when
/// the field is missing or not a JSON string. Pnpm tolerates either
/// shape — `name`/`version` are advisory metadata in this context, so
/// pacquet matches by silently dropping non-string values.
pub(super) fn manifest_string_field(manifest: &PackageManifest, key: &str) -> Option<String> {
    manifest
        .value()
        .get(key)
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
}

#[cfg(test)]
mod tests;
