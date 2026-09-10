use super::slots::PkgRoots;
use pnpm_lockfile::{PackageKey, SnapshotEntry};
use pnpm_package_manifest::{
    file_path_requires_build, manifest_requires_build, parse_manifest, pkg_requires_build,
};
use pnpm_patching::{ExtendedPatchInfo, preview_patch};
use std::collections::HashMap;

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
    patch_added_build.get(&key.without_peer()).copied().unwrap_or(false)
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
    let RequiresBuildInputs { snapshots, skipped, pkg_roots, prefetched, patches } = inputs;
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
    pub(super) has_store_writer: bool,
    pub(super) has_store_dir: bool,
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
        && gate.has_store_writer
        && gate.has_store_dir;
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
/// Whether previewing the configured patch shows it adding build work.
///
/// The same two triggers `pkg_requires_build` reads off an extracted package:
/// the manifest's install scripts, and the presence of `binding.gyp` /
/// `.hooks/`. A patch can add either. `None` when nothing can be previewed.
pub(super) fn previewed_patch_adds_build(
    patches: &HashMap<PackageKey, ExtendedPatchInfo>,
    metadata_key: &PackageKey,
    key: &PackageKey,
    pkg_roots: PkgRoots<'_>,
) -> Option<bool> {
    let patch_file_path = patches.get(metadata_key)?.patch_file_path.as_deref()?;
    let pkg_root = pkg_roots.canonical(key)?;
    let preview = preview_patch(&pkg_root, patch_file_path).ok()?;
    Some(
        preview.written_paths.iter().any(|path| file_path_requires_build(path))
            || preview.manifest.is_some_and(|manifest| {
                parse_manifest(&manifest).is_ok_and(|manifest| manifest_requires_build(&manifest))
            }),
    )
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
