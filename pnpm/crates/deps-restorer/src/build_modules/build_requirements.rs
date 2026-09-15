use super::slots::PkgRoots;
use pnpm_lockfile::{PackageKey, PackageMetadata, SnapshotEntry};
use pnpm_package_manifest::{
    BINDING_GYP, files_build_triggers, parse_manifest, pkg_build_triggers, pkg_requires_build,
};
use pnpm_patching::{ExtendedPatchInfo, MANIFEST_FILE_NAME, preview_patch};
use std::collections::{HashMap, HashSet};

/// Whether each configured patch adds build work its package's published
/// manifest does not declare, keyed by the peer-stripped package key.
///
/// Everything keyed off `requiresBuild` — the allow-build gate, the build
/// graph, the side-effects cache key — is decided before the build phase
/// applies the patch, so build work a patch introduces would otherwise
/// stay invisible until after the decisions that need it. Previewing the
/// patch keeps all three describing the package that ends up on disk.
///
/// Answered once per patch rather than once per snapshot, and only for a
/// package `published_requires_build` does not already answer `true` for:
/// peer variants share both the extracted manifest and the patch, and each
/// preview reads and parses two files.
///
/// A patch that fails to preview is left to the build phase, which
/// applies it for real and surfaces the failure.
/// Whether a configured patch adds build work to a snapshot the published
/// manifest did not already bind. [`PkgRoots::canonical`] is asked second: a
/// snapshot the walker dropped has nothing to build, and this way the lookup
/// only runs for the few snapshots whose patch adds build work.
pub(super) fn patch_adds_build(
    key: &PackageKey,
    patch_added_build: &HashMap<PackageKey, bool>,
    pkg_roots: PkgRoots<'_>,
) -> bool {
    patch_added_build
        .get(&key.without_peer())
        .copied()
        .unwrap_or(false)
        && pkg_roots.canonical(key).is_some()
}
/// What [`requires_build_by_key`] reads to decide, per snapshot, whether
/// the build gate has anything to run.
#[derive(Clone, Copy)]
pub(super) struct RequiresBuildInputs<'a> {
    pub(super) snapshots: &'a HashMap<PackageKey, SnapshotEntry>,
    pub(super) skipped: &'a crate::SkippedSnapshots,
    pub(super) pkg_roots: PkgRoots<'a>,
    /// Per-snapshot answers the store-index prefetch already computed.
    /// A miss falls back to inspecting the materialized directory.
    pub(super) prefetched: Option<&'a crate::RequiresBuildBySnapshot>,
    pub(super) patches: Option<&'a HashMap<PackageKey, ExtendedPatchInfo>>,
}
/// Whether each snapshot needs its build scripts run: what the package
/// published, plus what its configured patch adds.
pub(super) fn requires_build_by_key(inputs: RequiresBuildInputs<'_>) -> HashMap<PackageKey, bool> {
    let RequiresBuildInputs {
        snapshots,
        skipped,
        pkg_roots,
        prefetched,
        patches,
    } = inputs;
    let published: HashMap<PackageKey, bool> = snapshots
        .keys()
        // Skip snapshots that never landed on disk. `pkg_requires_build`
        // would just return `false` for a missing dir, but the walk would
        // still spend a syscall per skipped key — the filter
        // short-circuits that on installs with large optional fan-out.
        .filter(|key| !skipped.contains(key))
        .map(|key| {
            let requires = match (
                pkg_roots.canonical(key).as_deref(),
                prefetched.and_then(|map| map.get(key).copied()),
            ) {
                (None, _) => false,
                (_, Some(requires)) => requires,
                (Some(pkg_root), None) => pkg_requires_build(pkg_root),
            };
            (key.clone(), requires)
        })
        .collect();

    let patch_added_build = patch_added_build_by_package(patches, &published, pkg_roots);
    published
        .into_iter()
        .map(|(key, requires)| {
            let requires = requires || patch_adds_build(&key, &patch_added_build, pkg_roots);
            (key, requires)
        })
        .collect()
}
/// What decides whether the side-effects cache can fire at all: the READ side
/// (the prefetch surfaced cache rows) or the WRITE side (the install will be
/// populating new cache entries after a successful build).
pub(super) struct SideEffectsCacheGate {
    pub(super) side_effects_cache: bool,
    pub(super) side_effects_cache_write: bool,
    pub(super) has_publisher: bool,
    pub(super) frozen_store: bool,
    pub(super) has_engine_name: bool,
    pub(super) can_write_store: bool,
    pub(super) has_packages: bool,
    pub(super) has_cache_rows: bool,
}
pub(super) fn side_effects_cache_gate_active(gate: &SideEffectsCacheGate) -> bool {
    if !gate.has_engine_name || !gate.has_packages {
        return false;
    }
    let read_gate_active = gate.side_effects_cache && gate.has_cache_rows;
    let write_gate_active = (gate.side_effects_cache_write || gate.has_publisher)
        && !gate.frozen_store
        && gate.can_write_store;
    read_gate_active || write_gate_active
}
pub(super) fn patch_added_build_by_package(
    patches: Option<&HashMap<PackageKey, ExtendedPatchInfo>>,
    published_requires_build: &HashMap<PackageKey, bool>,
    pkg_roots: PkgRoots<'_>,
) -> HashMap<PackageKey, bool> {
    let Some(patches) = patches else { return HashMap::new() };
    let mut answers = HashMap::with_capacity(patches.len());
    for (key, published) in published_requires_build {
        // A package already bound for the build gate needs no preview: the
        // patch cannot subtract the build its manifest declares.
        if *published {
            continue;
        }
        // Peer-stripped, because patches are configured at the (name,
        // version) granularity rather than per peer-resolution variant.
        let metadata_key = key.without_peer();
        if answers.contains_key(&metadata_key) {
            continue;
        }
        let Some(adds_build) = previewed_patch_adds_build(patches, &metadata_key, key, pkg_roots)
        else {
            continue;
        };
        answers.insert(metadata_key, adds_build);
    }
    answers
}
/// Whether the package the configured patch would leave needs a build pass.
///
/// The same triggers `pkg_requires_build` reads off an extracted package: the
/// manifest's install scripts, and the presence of `.hooks/` or a `binding.gyp`
/// the manifest does not opt out of. `None` when nothing can be previewed.
///
/// Answered for the whole patched package rather than for the patch alone,
/// because the `gypfile` opt-out couples the two: a package can ship a
/// `binding.gyp` *and* `gypfile: false`, which leaves it build-free until a
/// patch rewrites the manifest. The `binding.gyp` the build then needs is one
/// the package already had, so a trigger set built only from the patch's own
/// written paths would miss it.
pub(super) fn previewed_patch_adds_build(
    patches: &HashMap<PackageKey, ExtendedPatchInfo>,
    metadata_key: &PackageKey,
    key: &PackageKey,
    pkg_roots: PkgRoots<'_>,
) -> Option<bool> {
    let patch_file_path = patches.get(metadata_key)?.patch_file_path.as_deref()?;
    let pkg_root = pkg_roots.canonical(key)?;
    let preview = preview_patch(&pkg_root, patch_file_path).ok()?;
    let mut triggers = pkg_build_triggers(&pkg_root);
    // A `binding.gyp` the patch deletes leaves no gyp build to synthesize, and a
    // manifest it deletes leaves no scripts and no `gypfile` to opt a surviving
    // `binding.gyp` out. `.hooks/` is not subtracted the same way: one deleted
    // entry does not empty the directory, and a package that ships one already
    // answers `true` to `published_requires_build`, so it never reaches this
    // preview.
    for removed in &preview.removed_paths {
        match removed.as_str() {
            BINDING_GYP => triggers.binding_gyp = false,
            MANIFEST_FILE_NAME => triggers.forget_manifest(),
            _ => {}
        }
    }
    triggers.add_files(files_build_triggers(&preview.written_paths));
    // A rewritten manifest replaces the published one the triggers were read
    // from, so its scripts and its `gypfile` value are what the build sees.
    if let Some(patched) = preview.manifest
        .as_deref()
        .and_then(|raw| parse_manifest(raw).ok())
    {
        triggers.read_manifest(&patched);
    }
    Some(triggers.requires_build())
}
/// The snapshots `--ignore-scripts` kept from building, sorted for a
/// stable `.modules.yaml`.
///
/// The caller supplies the snapshots in scope. Normal build-module runs
/// pass every snapshot they inspected; the `ignoreScripts`
/// fast path passes only entries newly materialized by this install.
pub(crate) fn deferred_builds<'a>(
    requires_build: impl IntoIterator<Item = (&'a PackageKey, &'a bool)>,
    ignore_scripts: bool,
) -> Vec<String> {
    if !ignore_scripts {
        return Vec::new();
    }
    let mut deferred: Vec<String> = requires_build
        .into_iter()
        .filter(|&(_, &requires_build)| requires_build)
        .map(|(key, _)| key.to_string())
        .collect();
    deferred.sort();
    deferred
}

/// The inputs of [`ScheduledBuilds::new`].
#[derive(Clone, Copy)]
pub struct ScheduledBuildsInputs<'a> {
    /// This install's materialized snapshots. `None` (a rebuild) schedules
    /// nothing through this path.
    pub materialized_snapshots: Option<&'a [PackageKey]>,
    /// The lockfile's `packages` rows, whose `hasBin` narrows the set to
    /// snapshots with bins. `None` keeps every snapshot.
    pub packages: Option<&'a HashMap<PackageKey, PackageMetadata>>,
    pub allow_build_policy: &'a super::AllowBuildPolicy,
    pub ignore_scripts: bool,
}

/// The snapshots with bins whose build may run after an install's link
/// phase: materialized by this install and allowed by `allowBuilds`.
///
/// Whether a build actually runs also depends on patches, `binding.gyp`
/// and `.hooks`, which the link phase does not evaluate. Over-including a
/// snapshot is safe: its held-back bin is linked by the post-build relink,
/// which runs whenever a build touched a slot. An ignored or denied build
/// is never included, since no script creates its bins later.
pub struct ScheduledBuilds<'a> {
    snapshots: HashSet<&'a PackageKey>,
}

impl<'a> ScheduledBuilds<'a> {
    /// `None` when no dependency build follows the link phase: scripts are
    /// ignored, or a rebuild passes no materialized snapshots.
    #[must_use]
    pub fn new(inputs: ScheduledBuildsInputs<'a>) -> Option<Self> {
        let materialized = inputs.materialized_snapshots.filter(|_| !inputs.ignore_scripts)?;
        let snapshots = materialized
            .iter()
            .filter(|key| {
                inputs.packages.is_none_or(|packages| {
                    packages
                        .get(&key.without_peer())
                        .is_some_and(crate::link_bins::may_have_bin)
                })
            })
            .filter(|key| {
                inputs.allow_build_policy.check(&key.without_peer().to_string()) == Some(true)
            })
            .collect();
        Some(ScheduledBuilds { snapshots })
    }

    #[must_use]
    pub fn includes(&self, snapshot_key: &PackageKey) -> bool {
        self.snapshots.contains(snapshot_key)
    }
}
