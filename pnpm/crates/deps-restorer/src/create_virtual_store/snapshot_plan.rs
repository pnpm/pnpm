//! Decide which snapshots this install must materialize, pairing each
//! with the store-index cache key [`super::CasPrefetch::start`]
//! derived for it.

use super::{
    CreateVirtualStoreError, SnapshotCacheKey, SnapshotWithCacheKey, gvs_slot_needs_rebuild,
    integrity_equal, snapshot_deps_equal,
};
use crate::{SkippedSnapshots, VirtualStoreLayout};
use pnpm_lockfile::{LockfileResolution, PackageKey, PackageMetadata, SnapshotEntry};
use pnpm_reporter::{BrokenModulesLog, LogEvent, LogLevel, Reporter};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

/// `'a` is the lifetime of the lockfile maps the resulting
/// [`SnapshotPlan`] borrows from; `'b` covers the inputs the plan pass
/// only reads while running.
pub(super) struct SnapshotPlanInputs<'a, 'b> {
    pub snapshots: &'a HashMap<PackageKey, SnapshotEntry>,
    pub packages: &'a HashMap<PackageKey, PackageMetadata>,
    /// What the previous install materialized. `None` on a first
    /// install; when present, a snapshot whose wiring and integrity are
    /// unchanged *and* whose slot is still on disk is left alone.
    pub current_snapshots: Option<&'b HashMap<PackageKey, SnapshotEntry>>,
    pub current_packages: Option<&'b HashMap<PackageKey, PackageMetadata>>,
    pub layout: &'b VirtualStoreLayout,
    pub allow_build_policy: &'b crate::AllowBuildPolicy,
    /// Snapshots the installability pass ruled out on this host.
    pub skipped: &'b SkippedSnapshots,
    pub link_dependencies: bool,
    /// `--force` re-materializes every slot, so both skip paths — the
    /// current-lockfile comparison and the global-virtual-store
    /// existence probe — are disabled here, whether or not the caller
    /// also nulled out `current_snapshots`.
    pub force: bool,
    pub is_hoisted: bool,
    pub include_optional_dependencies: bool,
    /// One derivation `Result` per lockfile snapshot, taken by the
    /// entry that keeps it. See [`super::CasPrefetch::start`], which
    /// guarantees the every-snapshot coverage.
    pub cache_keys: &'b mut HashMap<PackageKey, Result<SnapshotCacheKey, CreateVirtualStoreError>>,
}

/// The snapshots to install, the ones deliberately left alone, and the
/// slots whose global-virtual-store build marker forces a rebuild.
pub(super) struct SnapshotPlan<'a> {
    /// Snapshots this install materializes.
    pub survivors: Vec<SnapshotWithCacheKey<'a>>,
    /// Snapshots the current-lockfile check or the global-virtual-store
    /// existence probe skipped. They contribute no link work, but their
    /// store-index rows still feed the build phase's `is_built` gate,
    /// so a warm reinstall does not re-run approved build scripts.
    pub skipped_entries: Vec<SnapshotWithCacheKey<'a>>,
    pub marker_rebuilds: HashSet<PackageKey>,
    pub has_git_hosted_survivor: bool,
}

/// Partition the lockfile's snapshots into what this install must do
/// and what it may leave alone.
///
/// Validation is deliberately asymmetric: survivors keep the strict
/// cache-key derivation `Result`, because the install will actually
/// fetch and link them, so a malformed resolution must fail before the
/// warm batch starts rather than several seconds into it. Skipped
/// snapshots get a lenient pass — they are not being installed, and
/// swallowing a per-snapshot error there costs only a prefetch row.
pub(super) fn plan_snapshots<'a, Reporter: self::Reporter>(
    inputs: SnapshotPlanInputs<'a, '_>,
) -> Result<SnapshotPlan<'a>, CreateVirtualStoreError> {
    let SnapshotPlanInputs {
        snapshots,
        packages,
        current_snapshots,
        current_packages,
        layout,
        allow_build_policy,
        skipped,
        link_dependencies,
        force,
        is_hoisted,
        include_optional_dependencies,
        cache_keys,
    } = inputs;

    // The slot probe goes through `layout.slot_dir` because under GVS
    // the slot lives at `<global_virtual_store_dir>/...`, and probing
    // `<virtual_store_dir>/<flat-name>` would find nothing and report
    // every warm slot as broken. See pnpm/pacquet#442 for why the
    // current-lockfile skip keeps its store-index rows.
    let mut marker_probe_keys = HashSet::new();
    let mut marker_rebuilds = HashSet::new();
    let mut has_git_hosted_survivor = false;
    let snapshot_entries = snapshots
        .iter()
        // Reason 1: installability skip. Drop entirely.
        .filter(|(snapshot_key, _)| !skipped.contains(snapshot_key))
        // Reason 2: warm-slot skip. Drop survivors that already match
        // the previous install, or whose content-addressed global-
        // virtual-store slot already exists. This is a fallible fold
        // because a warm-slot lstat error must abort the install rather
        // than quietly converting the slot into a rebuild on every run.
        .try_fold(Vec::new(), |mut entries, (snapshot_key, snapshot)| {
            let current_slot_matches = (|| -> Result<bool, CreateVirtualStoreError> {
                // The hoisted linker writes no virtual-store slot, so
                // this probe cannot judge it (pnpm/pnpm#14001).
                if is_hoisted {
                    return Ok(false);
                }
                let wanted_metadata = packages.get(&snapshot_key.without_peer());
                // A `file:` dependency's source is mutable, so neither
                // an unchanged lockfile nor an existing slot is
                // evidence its copy is current.
                if matches!(
                    wanted_metadata.map(|meta| &meta.resolution),
                    Some(LockfileResolution::Directory(_)),
                ) {
                    return Ok(false);
                }
                let current_entry_unchanged = !force
                    && current_snapshots
                        .and_then(|current_snapshots| current_snapshots.get(snapshot_key))
                        .is_some_and(|current_snapshot| {
                            snapshot_deps_equal(current_snapshot, snapshot)
                                && integrity_equal(
                                    current_packages
                                        .and_then(|p| p.get(&snapshot_key.without_peer())),
                                    wanted_metadata,
                                )
                        });
                // A global-virtual-store slot path is content-addressed:
                // the graph hash covers the snapshot's wiring, integrity,
                // and engine, so an existing slot is current even when no
                // current lockfile survives — a wiped `node_modules`
                // takes `<virtual_store_dir>/lock.yaml` with it, and
                // without this probe such a restore re-links every slot
                // the store already holds (pnpm/pnpm#14510). Mirrors the
                // GVS fast path in pnpm's `lockfileToDepGraph`.
                let gvs_slot_is_authoritative = layout.enable_global_virtual_store() && !force;
                if !current_entry_unchanged && !gvs_slot_is_authoritative {
                    return Ok(false);
                }
                let dir = layout
                    .slot_dir(snapshot_key)
                    .join("node_modules")
                    .join(snapshot_key.name.to_string());
                if !probe_slot_entry(&dir, EntryKind::Dir)? {
                    // Only a slot the current lockfile vouches for is
                    // "broken" when missing; a mere GVS-existence miss is
                    // a fresh materialization.
                    if current_entry_unchanged {
                        Reporter::emit(&LogEvent::BrokenModules(BrokenModulesLog {
                            level: LogLevel::Debug,
                            missing: dir.to_string_lossy().into_owned(),
                        }));
                    }
                    // A missing slot has no build marker either, so the
                    // survivor marker rescan after this fold need not
                    // stat under it again.
                    marker_probe_keys.insert(snapshot_key.clone());
                    return Ok(false);
                }
                // The importer populates shared GVS slots in place, so an
                // existing directory may be an import another install is
                // still filling or died halfway through (see
                // `import_into_shared_dir`). Without a current-lockfile
                // record vouching that a previous install completed the
                // slot, require the importer's own completion invariant —
                // pnpm's `pkgExistsAtTargetDir` probes `package.json`,
                // which the import places last. A rare package whose file
                // map lacks `package.json` merely re-materializes, and
                // the import then short-circuits on its actual marker.
                if !current_entry_unchanged
                    && !probe_slot_entry(&dir.join("package.json"), EntryKind::File)?
                {
                    return Ok(false);
                }
                if !optional_children_match(
                    snapshot_key,
                    snapshot,
                    layout,
                    skipped,
                    link_dependencies,
                    include_optional_dependencies,
                )? {
                    return Ok(false);
                }
                // The completion marker only covers the file import: the
                // slot's child symlinks are written concurrently with it
                // (`rayon::join` in `CreateVirtualDirBySnapshot::run`),
                // so a crash can leave a marker-complete slot with links
                // missing. A current-lockfile record is only written by
                // a completed install and so vouches for the links too;
                // without one, probe every child the symlink layout
                // would have created.
                if !current_entry_unchanged
                    && !regular_children_match(
                        snapshot_key,
                        snapshot,
                        layout,
                        skipped,
                        link_dependencies,
                    )?
                {
                    return Ok(false);
                }
                let needs_rebuild =
                    gvs_slot_needs_rebuild(layout, allow_build_policy, snapshot_key);
                marker_probe_keys.insert(snapshot_key.clone());
                if needs_rebuild {
                    marker_rebuilds.insert(snapshot_key.clone());
                }
                Ok(!needs_rebuild)
            })()?;
            if !current_slot_matches {
                let cache_key = cache_keys
                    .remove(snapshot_key)
                    .expect("CasPrefetch::start derived a cache key for every lockfile snapshot")?;
                has_git_hosted_survivor |= cache_key.is_git_hosted;
                entries.push((snapshot_key, snapshot, cache_key.value));
            }
            Ok::<_, CreateVirtualStoreError>(entries)
        })?;
    if !is_hoisted {
        marker_rebuilds.extend(
            snapshot_entries
                .iter()
                .filter(|(snapshot_key, _, _)| !marker_probe_keys.contains(*snapshot_key))
                .filter(|(snapshot_key, _, _)| {
                    gvs_slot_needs_rebuild(layout, allow_build_policy, snapshot_key)
                })
                .map(|(snapshot_key, _, _)| (*snapshot_key).clone()),
        );
    }

    // A parallel `Vec` rather than a filter later: the partition's
    // manifest and side-effects loop has to see the full snapshot set,
    // not just survivors.
    let survivor_keys: std::collections::HashSet<&PackageKey> =
        snapshot_entries.iter().map(|(k, _, _)| *k).collect();
    let skipped_entries: Vec<SnapshotWithCacheKey<'_>> = snapshots
        .iter()
        .filter(|(snapshot_key, _)| !survivor_keys.contains(snapshot_key))
        // Installability-skipped snapshots are excluded from
        // `skipped_entries` too — they were never installed, so
        // there's no store-index row to keep warm for the
        // build-cache lookup. Only the current-lockfile-skip
        // path (`snapshot_entries` filtered above) should contribute
        // here.
        .filter(|(snapshot_key, _)| !skipped.contains(snapshot_key))
        .map(|(snapshot_key, snapshot)| {
            let cache_key = cache_keys
                .remove(snapshot_key)
                .and_then(Result::ok)
                .and_then(|cache_key| cache_key.value);
            (snapshot_key, snapshot, cache_key)
        })
        .collect();
    Ok(SnapshotPlan {
        survivors: snapshot_entries,
        skipped_entries,
        marker_rebuilds,
        has_git_hosted_survivor,
    })
}

/// Whether every child link the symlink layout would create for the
/// snapshot's regular `dependencies` is present in the slot. Mirrors
/// [`crate::create_symlink_layout()`]'s predicate: the slot's own name
/// never gets a link, a `link:` child is linked only when the layout
/// knows the lockfile dir, and a skipped target gets no link. Children
/// the layout would not create are not required to be absent — a
/// shared slot may carry links written by an install with a different
/// skip set, and re-importing would not remove them, so requiring
/// absence would re-materialize the slot on every install.
///
/// Presence is an lstat: the layout creates each symlink regardless of
/// whether its target slot is materialized yet.
fn regular_children_match(
    snapshot_key: &PackageKey,
    snapshot: &SnapshotEntry,
    layout: &VirtualStoreLayout,
    skipped: &SkippedSnapshots,
    link_dependencies: bool,
) -> Result<bool, CreateVirtualStoreError> {
    if !link_dependencies {
        return Ok(true);
    }
    let Some(dependencies) = snapshot.dependencies.as_ref() else {
        return Ok(true);
    };
    let modules_dir = layout.slot_dir(snapshot_key).join("node_modules");
    for (alias, dep_ref) in dependencies {
        if alias == &snapshot_key.name {
            continue;
        }
        let expected = if let Some(target) = dep_ref.resolve(alias) {
            !skipped.contains(&target)
        } else {
            dep_ref.as_link_target().is_some() && layout.lockfile_dir().is_some()
        };
        if !expected {
            continue;
        }
        let Ok(child_path) =
            crate::safe_join_modules_dir::safe_join_modules_dir(&modules_dir, &alias.to_string())
        else {
            return Ok(false);
        };
        if !child_link_present(&child_path)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Whether `child_path` holds the directory link the symlink layout
/// writes. A plain file or directory in its place is a corrupted slot,
/// not a link, so it does not count.
fn child_link_present(child_path: &Path) -> Result<bool, CreateVirtualStoreError> {
    match std::fs::symlink_metadata(child_path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                return Ok(true);
            }
            // On Windows `symlink_dir` may have fallen back to a
            // junction, which `is_symlink` does not report.
            #[cfg(windows)]
            return pnpm_fs::is_symlink_or_junction(child_path).map_err(|error| {
                CreateVirtualStoreError::InspectVirtualStoreSlot {
                    path: child_path.to_path_buf(),
                    error,
                }
            });
            #[cfg(not(windows))]
            Ok(false)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(CreateVirtualStoreError::InspectVirtualStoreSlot {
            path: child_path.to_path_buf(),
            error,
        }),
    }
}

/// Kind of dirent a slot probe expects.
#[derive(Clone, Copy)]
enum EntryKind {
    Dir,
    File,
}

/// Whether `path` exists as the expected kind. `NotFound` — and
/// `NotADirectory`, a file sitting where a parent directory is
/// expected — mean the slot is not there; any other inspection error
/// aborts the install, per the warm-slot fold's contract.
fn probe_slot_entry(path: &Path, kind: EntryKind) -> Result<bool, CreateVirtualStoreError> {
    match std::fs::metadata(path) {
        Ok(metadata) => Ok(match kind {
            EntryKind::Dir => metadata.is_dir(),
            EntryKind::File => metadata.is_file(),
        }),
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory,
            ) =>
        {
            Ok(false)
        }
        Err(error) => Err(CreateVirtualStoreError::InspectVirtualStoreSlot {
            path: path.to_path_buf(),
            error,
        }),
    }
}

fn optional_children_match(
    snapshot_key: &PackageKey,
    snapshot: &SnapshotEntry,
    layout: &VirtualStoreLayout,
    skipped: &SkippedSnapshots,
    link_dependencies: bool,
    include_optional_dependencies: bool,
) -> Result<bool, CreateVirtualStoreError> {
    optional_children_match_with(
        snapshot_key,
        snapshot,
        layout,
        skipped,
        link_dependencies,
        include_optional_dependencies,
        optional_child_matches,
    )
}

fn optional_child_matches(child_path: &Path, should_exist: bool) -> std::io::Result<bool> {
    if should_exist {
        return match std::fs::metadata(child_path) {
            Ok(metadata) => Ok(metadata.is_dir()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        };
    }
    match std::fs::symlink_metadata(child_path) {
        Ok(_) => Ok(false),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(error) => Err(error),
    }
}

fn optional_children_match_with(
    snapshot_key: &PackageKey,
    snapshot: &SnapshotEntry,
    layout: &VirtualStoreLayout,
    skipped: &SkippedSnapshots,
    link_dependencies: bool,
    include_optional_dependencies: bool,
    mut child_matches: impl FnMut(&Path, bool) -> std::io::Result<bool>,
) -> Result<bool, CreateVirtualStoreError> {
    let Some(optional_dependencies) = snapshot.optional_dependencies.as_ref() else {
        return Ok(true);
    };
    let modules_dir = layout.slot_dir(snapshot_key).join("node_modules");
    for (alias, dep_ref) in optional_dependencies {
        if alias == &snapshot_key.name {
            continue;
        }
        let Ok(child_path) =
            crate::safe_join_modules_dir::safe_join_modules_dir(&modules_dir, &alias.to_string())
        else {
            return Ok(false);
        };
        let should_exist = link_dependencies
            && include_optional_dependencies
            && if let Some(target) = dep_ref.resolve(alias) {
                !skipped.contains(&target)
            } else {
                dep_ref.as_link_target().is_some() && layout.lockfile_dir().is_some()
            };
        let matches = child_matches(&child_path, should_exist).map_err(|error| {
            CreateVirtualStoreError::InspectOptionalDependency { path: child_path.clone(), error }
        })?;
        if !matches {
            return Ok(false);
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests;
