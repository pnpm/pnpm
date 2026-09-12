pub use direct::{
    PrefetchedBinLookup, PrefetchedDepBin, link_direct_dep_bins,
    link_direct_dep_bins_from_locations, link_direct_dep_bins_prefetched,
    link_direct_dep_bins_resolved, link_project_bins, link_top_level_bins,
    resolve_hoisted_bin_deps, shim_link_options,
};

mod scan;
use scan::{read_package, run_with_readdir};

mod direct;

use crate::{PackageManifests, SkippedSnapshots};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_cmd_shim::{
    FsCreateDirAll, FsEnsureExecutableBits, FsReadDir, FsReadFile, FsReadHead, FsReadToString,
    FsSetExecutable, FsWalkFiles, FsWrite, Host, LinkBinsError, LinkBinsOptions, PackageBinSource,
    collect_packages_in_modules_dir, link_bins_of_packages,
};
use pnpm_lockfile::{LockfileResolution, PackageKey, PackageMetadata, PkgName, SnapshotEntry};
use rayon::prelude::*;
use std::{
    collections::{HashMap, HashSet},
    io,
    path::{Path, PathBuf},
    sync::Arc,
};

/// Error type of [`LinkVirtualStoreBins`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum LinkVirtualStoreBinsError {
    #[display("Failed to read virtual store directory at {dir:?}: {error}")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANAGER_READ_VIRTUAL_STORE))]
    ReadVirtualStore {
        dir: PathBuf,
        #[error(source)]
        error: io::Error,
    },

    #[diagnostic(transparent)]
    LinkBins(#[error(source)] LinkBinsError),
}

/// For every package slot under `<virtual_store_dir>/<pkg>@<ver>/node_modules`,
/// link the bins of that slot's child packages into the slot's *own*
/// `node_modules/.bin` directory.
///
/// Each slot's children (its direct deps in the resolved graph)
/// contribute their bins into `<slot.dir>/node_modules/.bin`.
///
/// Pacquet's virtual store layout already exposes a slot's children as
/// siblings via [`create_symlink_layout`](crate::create_symlink_layout()).
/// So once the symlinks exist, walking
/// the slot's `node_modules` and excluding the package itself gives the
/// child-set to link, and the bins go into the package's own
/// `node_modules/.bin` (i.e. nested *one level deeper* than the slot's
/// `node_modules` directory).
///
/// Path layout produced for a slot `A@1.0.0`:
///
/// ```text
/// <virtual>/A@1.0.0/node_modules/A/node_modules/.bin/<bin>
/// ```
///
/// When `snapshots` is `Some` (the frozen-lockfile case), the slot
/// set is taken from the lockfile and each child's manifest is
/// looked up in `package_manifests` rather than read off disk.
/// When `snapshots` is `None` (install without a lockfile), the
/// linker falls back to enumerating slots and reading manifests via
/// the filesystem.
#[must_use]
pub struct LinkVirtualStoreBins<'a> {
    /// Install-scoped slot-directory mapping (GVS-aware). The layout
    /// knows where each snapshot's slot lives, including under the
    /// global-virtual-store `<scope>/<name>/<version>/<hash>` shape.
    /// See [`crate::VirtualStoreLayout`].
    pub layout: &'a crate::VirtualStoreLayout,
    /// `Some` when the install is lockfile-driven. Iterating the
    /// snapshot map (instead of `read_dir(virtual_store_dir)`)
    /// removes the per-slot directory enumeration and lets us walk
    /// each slot's children from its `dependencies` /
    /// `optionalDependencies` lists without touching the filesystem.
    pub snapshots: Option<&'a HashMap<PackageKey, SnapshotEntry>>,
    /// When present, limit the lockfile-driven pass to these slots.
    /// Install paths pass the snapshots materialized in the current run;
    /// rebuild and filesystem-discovery callers leave it unrestricted.
    pub selected_snapshots: Option<&'a [PackageKey]>,
    /// Lockfile `packages:` section, indexed by `PkgNameVerPeer`
    /// (without peer suffix). Used to filter children by
    /// `hasBin == true` *before* any per-child IO.
    /// Most packages don't declare a bin, so this short-circuits the
    /// bulk of the per-slot work before any path-building or manifest
    /// lookup happens.
    pub packages: Option<&'a HashMap<PackageKey, PackageMetadata>>,
    /// Bundled manifests recovered from the warm-cache prefetch of
    /// `index.db` ([`crate::PackageManifests`]). A hit lets the
    /// linker skip the `package.json` read for that child entirely;
    /// a miss falls back to a disk read so cold-batch packages
    /// installed earlier in the same run still get their bins
    /// linked.
    pub package_manifests: &'a PackageManifests,
    /// Snapshots the installability pass marked optional+incompatible.
    /// Their slots were never created by [`crate::CreateVirtualStore`],
    /// so the bin linker has nothing to walk for them — and any
    /// child-manifest disk read against their non-existent
    /// `<slot>/node_modules/<alias>` would fail. Excluding them up
    /// front matches the rest of the install pipeline's filtering.
    pub skipped: &'a SkippedSnapshots,
    /// [`shim_link_options`] output — pnpm threads the same
    /// `extraNodePaths` into slot-internal bin shims as into importer
    /// bins.
    pub link_options: &'a LinkBinsOptions,
}

impl LinkVirtualStoreBins<'_> {
    pub fn run(self) -> Result<(), LinkVirtualStoreBinsError> {
        self.run_with::<Host>()
    }

    /// DI-driven entry. Production callers go through [`Self::run`] which
    /// turbofishes [`Host`]; tests inject fakes that fail specific fs
    /// operations to cover error paths the real fs can't trigger
    /// portably. See the per-capability DI pattern at
    /// <https://github.com/pnpm/pacquet/pull/332#issuecomment-4345054524>.
    pub fn run_with<Sys>(self) -> Result<(), LinkVirtualStoreBinsError>
    where
        Sys: FsReadDir
            + FsReadFile
            + FsReadToString
            + FsReadHead
            + FsCreateDirAll
            + FsWalkFiles
            + FsWrite
            + FsSetExecutable
            + FsEnsureExecutableBits,
    {
        let LinkVirtualStoreBins {
            layout,
            snapshots,
            selected_snapshots,
            packages,
            package_manifests,
            skipped,
            link_options,
        } = self;
        if let Some(snapshots) = snapshots {
            let has_bin_set = build_has_bin_set(packages);
            let bundling_set = build_bundling_set(packages);
            run_lockfile_driven::<Sys>(
                layout,
                snapshots,
                selected_snapshots,
                BinSlotSets { has_bin: has_bin_set.as_ref(), bundling: &bundling_set },
                package_manifests,
                skipped,
                link_options,
            )
        } else {
            // No snapshots (lockfile absent or empty): fall back to a
            // `read_dir` enumeration. This path only fires for non-
            // frozen installs, which <https://github.com/pnpm/pacquet/issues/432> doesn't activate GVS for, so
            // reading from `layout.package_store_dir()` reproduces
            // today's behaviour exactly when GVS is off.
            run_with_readdir::<Sys>(layout.package_store_dir(), link_options)
        }
    }
}

/// Pre-compute the set of package keys whose lockfile metadata sets
/// `hasBin: true`. Most packages don't declare a bin, so
/// short-circuiting the per-child manifest lookup with this set is
/// the cheapest win on warm-cache installs.
///
/// Return-value semantics distinguish "lockfile metadata absent"
/// from "lockfile metadata says no package has a bin":
///
/// - `None` — the lockfile's `packages:` section wasn't supplied
///   (pathological lockfile shape). We have no info, so the bin
///   linker falls back to the conservative "process every child"
///   path and lets the per-package bin resolver sort it out.
/// - `Some(set)` — the section was present and we used it. The
///   `set` contains only entries with `hasBin == Some(true)`; an
///   *empty* `Some(set)` is authoritative: the lockfile says no
///   package has a bin, and every slot should short-circuit
///   immediately. Conflating this case with `None` (the bug flagged
///   at <https://github.com/pnpm/pacquet/pull/333#discussion_r3222807548>)
///   would force per-child work the lockfile already ruled out.
fn build_has_bin_set(
    packages: Option<&HashMap<PackageKey, PackageMetadata>>,
) -> Option<HashSet<PackageKey>> {
    let packages = packages?;
    Some(
        packages
            .iter()
            .filter(|(_, meta)| {
                meta.has_bin == Some(true)
                    || matches!(
                        meta.resolution,
                        LockfileResolution::Binary(_) | LockfileResolution::Variations(_),
                    )
            })
            .map(|(key, _)| key.clone())
            .collect(),
    )
}

/// Pre-compute the set of package keys that ship dependencies inside their
/// own tarball. Presence of the lockfile's `bundledDependencies` field is
/// the signal, whatever its shape — a name list and the `true` form both
/// mean the package carries a populated `node_modules` of its own.
///
/// Unlike [`build_has_bin_set`] there is no "metadata absent" case to
/// distinguish: without a `packages:` section no slot can be known to
/// bundle, and probing every slot's package directory for a `node_modules`
/// that almost never exists would cost a `read_dir` per slot.
fn build_bundling_set(
    packages: Option<&HashMap<PackageKey, PackageMetadata>>,
) -> HashSet<PackageKey> {
    let recorded = packages.into_iter().flatten();
    recorded
        .filter(|(_, meta)| meta.bundled_dependencies.is_some())
        .map(|(key, _)| key.clone())
        .collect()
}

/// The two lockfile-derived slot classifications [`run_lockfile_driven`]
/// consults before doing any per-slot work, from [`build_has_bin_set`] and
/// [`build_bundling_set`] respectively.
#[derive(Clone, Copy)]
struct BinSlotSets<'a> {
    has_bin: Option<&'a HashSet<PackageKey>>,
    bundling: &'a HashSet<PackageKey>,
}

/// Walk the lockfile's `snapshots:` map, build each slot's bin output
/// directory lexically, and link every child's bins into it. The
/// child set comes from `snapshot.dependencies` +
/// `snapshot.optional_dependencies`, filtered by `sets.has_bin` so
/// packages that don't declare a bin never make it into the
/// per-slot path-building or manifest-lookup work. The corresponding
/// manifest comes from [`PackageManifests`] (no disk read) or, for
/// cold-batch packages that prefetch missed, a fallback
/// `package.json` read through the existing symlink at
/// `<slot>/node_modules/<alias>`.
fn run_lockfile_driven<Sys>(
    layout: &crate::VirtualStoreLayout,
    snapshots: &HashMap<PackageKey, SnapshotEntry>,
    selected_snapshots: Option<&[PackageKey]>,
    sets: BinSlotSets<'_>,
    package_manifests: &PackageManifests,
    skipped: &SkippedSnapshots,
    link_options: &LinkBinsOptions,
) -> Result<(), LinkVirtualStoreBinsError>
where
    Sys: FsReadDir
        + FsReadFile
        + FsReadToString
        + FsReadHead
        + FsCreateDirAll
        + FsWalkFiles
        + FsWrite
        + FsSetExecutable
        + FsEnsureExecutableBits,
{
    // `has_bin_set` is `Some` exactly when the lockfile's `packages:`
    // section was present at install start — in which case the set
    // is authoritative and every slot is filtered through it (an
    // empty `Some(set)` means "no package declares a bin", which
    // short-circuits every slot below). When the section was
    // missing we have no info and fall through to processing every
    // child. See [`build_has_bin_set`] for the rationale.
    // Materialise as a `Vec` so rayon can split the work; iterating
    // a `HashMap` directly with `par_iter` would require collecting
    // anyway, and explicit collection here keeps the parallelism
    // contract obvious.
    //
    // Filter out installability-skipped snapshots here: their
    // virtual-store slot was never created (see
    // [`crate::CreateVirtualStore::run`]'s `survivors` filter), so
    // attempting to walk the snapshot's `dependencies` /
    // `optional_dependencies` for bin linking would either fall
    // through to a cold-batch disk read against a non-existent
    // `<slot>/node_modules/<alias>` (returning `None` harmlessly but
    // wasting work) or — worse — create a `<slot>/.../node_modules/.bin`
    // directory under a slot that doesn't exist on disk.
    let selected_snapshots: Option<HashSet<&PackageKey>> =
        selected_snapshots.map(|keys| keys.iter().collect());
    let slot_entries: Vec<(&PackageKey, &SnapshotEntry)> = snapshots
        .iter()
        .filter(|(slot_key, _)| {
            !skipped.contains(slot_key)
                && selected_snapshots.as_ref().is_none_or(|keys| keys.contains(slot_key))
        })
        .collect();
    slot_entries.par_iter().try_for_each(|(slot_key, snapshot)| {
        link_slot_bins::<Sys>(
            &SlotBinContext { layout, sets, package_manifests, link_options },
            slot_key,
            snapshot,
        )
    })
}

/// The inputs [`link_slot_bins`] shares across every slot of one pass.
struct SlotBinContext<'a> {
    layout: &'a crate::VirtualStoreLayout,
    sets: BinSlotSets<'a>,
    package_manifests: &'a PackageManifests,
    link_options: &'a LinkBinsOptions,
}

/// Link one virtual-store slot's `node_modules/.bin`.
///
/// Two kinds of package contribute a bin:
///
/// 1. Every child whose manifest declares `bin`. Cheap to detect via
///    `sets.has_bin` (pre-built from the lockfile's `packages:` rows).
///    Without a child or a self-bin the slot needs no `.bin` directory
///    at all, so the early return below skips ~95% of slots on a
///    real-world lockfile (measured on the integrated-benchmark
///    fixture).
/// 2. The slot's own package, when it carries a bin. It is appended
///    unconditionally and the reader's manifest check drops it when
///    there is nothing to write — so for a package like
///    `hello-world-js-bin` (no deps, one bin) this writes
///    `<slot>/node_modules/<pkg>/node_modules/.bin/<pkg>` as a
///    self-shim.
fn link_slot_bins<Sys>(
    context: &SlotBinContext<'_>,
    slot_key: &PackageKey,
    snapshot: &SnapshotEntry,
) -> Result<(), LinkVirtualStoreBinsError>
where
    Sys: FsReadDir
        + FsReadFile
        + FsReadToString
        + FsReadHead
        + FsCreateDirAll
        + FsWalkFiles
        + FsWrite
        + FsSetExecutable
        + FsEnsureExecutableBits,
{
    let with_bin = children_with_bins(snapshot, context.sets.has_bin);
    let self_metadata_key = slot_key.without_peer();
    let self_has_bin = declares_bin(context.sets.has_bin, &self_metadata_key);
    let self_bundles = context.sets.bundling.contains(&self_metadata_key);
    if with_bin.is_empty() && !self_has_bin && !self_bundles {
        return Ok(());
    }

    let modules_dir = context.layout.slot_dir(slot_key).join("node_modules");
    let self_pkg_dir = slot_own_pkg_dir(&modules_dir, slot_key);
    let bins_dir = self_pkg_dir.join("node_modules/.bin");

    let mut bin_sources: Vec<PackageBinSource> =
        Vec::with_capacity(with_bin.len() + usize::from(self_has_bin));
    push_child_bin_sources::<Sys>(&mut bin_sources, context, &modules_dir, with_bin)?;

    // Packages the tarball ships in its own `node_modules` are not
    // lockfile children, so nothing above sees them; their bins are
    // reachable only from inside the bundling package and have to be
    // shimmed straight from disk.
    if self_bundles {
        bin_sources.extend(
            collect_packages_in_modules_dir::<Sys>(&self_pkg_dir.join("node_modules"))
                .map_err(LinkVirtualStoreBinsError::LinkBins)?,
        );
    }

    // The slot's own package dir is a real directory already, so it
    // doubles as its own resolved location.
    if self_has_bin {
        push_bin_source::<Sys>(
            &mut bin_sources,
            context.package_manifests,
            &self_metadata_key,
            self_pkg_dir.clone(),
            self_pkg_dir,
        )?;
    }

    if bin_sources.is_empty() {
        return Ok(());
    }
    link_bins_of_packages::<Sys>(&bin_sources, &bins_dir, context.link_options)
        .map_err(LinkVirtualStoreBinsError::LinkBins)
}

fn push_child_bin_sources<Sys: FsReadFile>(
    bin_sources: &mut Vec<PackageBinSource>,
    context: &SlotBinContext<'_>,
    modules_dir: &Path,
    with_bin: Vec<(&PkgName, PackageKey, PackageKey)>,
) -> Result<(), LinkVirtualStoreBinsError> {
    for (alias, child_key, metadata_key) in with_bin {
        push_bin_source::<Sys>(
            bin_sources,
            context.package_manifests,
            &metadata_key,
            pkg_dir_under(modules_dir, alias),
            // The child location reaches the child through the slot's
            // alias symlink; the layout knows the symlink's destination
            // without touching the filesystem, so the shim `NODE_PATH`
            // derivation never has to `realpath`.
            pkg_dir_under(
                &context.layout.slot_dir(&child_key).join("node_modules"),
                &child_key.name,
            ),
        )?;
    }
    Ok(())
}

/// Whether the lockfile says this package declares a bin. No
/// `has_bin_set` means the lockfile had no `packages:` section, so
/// every package stays a candidate and the downstream manifest read
/// decides — an over-inclusion costs at most one `package.json` read.
fn declares_bin(has_bin_set: Option<&HashSet<PackageKey>>, metadata_key: &PackageKey) -> bool {
    has_bin_set.is_none_or(|set| set.contains(metadata_key))
}

/// The snapshot's children that may contribute a bin, as
/// `(alias, child key, metadata key)`. `link:` deps live outside the
/// virtual store and expose their bins through the workspace project's
/// own `package.json`, not through a snapshot, so they are dropped.
fn children_with_bins<'a>(
    snapshot: &'a SnapshotEntry,
    has_bin_set: Option<&HashSet<PackageKey>>,
) -> Vec<(&'a PkgName, PackageKey, PackageKey)> {
    let declared = snapshot
        .dependencies
        .iter()
        .flatten()
        .chain(snapshot.optional_dependencies.iter().flatten());
    declared
        .filter_map(|(alias, dep_ref)| {
            let child_key = dep_ref.resolve(alias)?;
            let metadata_key = child_key.without_peer();
            declares_bin(has_bin_set, &metadata_key).then_some((alias, child_key, metadata_key))
        })
        .collect()
}

/// Add the bin source of the package at `location`: the parsed manifest
/// the warm-cache prefetch already holds, or — for a cold-batch package
/// downloaded during this run, whose row isn't in the prefetched map — a
/// disk read on the same code path the non-lockfile install takes (see
/// [`run_with_readdir`]).
///
/// Both the prefetch map and [`PackageBinSource`] hold the manifest via
/// [`Arc`], so the warm path is a refcount bump rather than a deep clone
/// of the JSON tree.
fn push_bin_source<Sys>(
    bin_sources: &mut Vec<PackageBinSource>,
    package_manifests: &PackageManifests,
    metadata_key: &PackageKey,
    location: PathBuf,
    resolved_location: PathBuf,
) -> Result<(), LinkVirtualStoreBinsError>
where
    Sys: FsReadFile,
{
    if let Some(manifest) = package_manifests.get(metadata_key) {
        bin_sources.push(
            PackageBinSource::new(location, Arc::clone(manifest))
                .with_resolved_location(resolved_location),
        );
        return Ok(());
    }
    match read_package::<Sys>(&location) {
        Ok(Some(pkg)) => {
            bin_sources.push(pkg.with_resolved_location(resolved_location));
            Ok(())
        }
        Ok(None) => Ok(()),
        Err(error) => Err(LinkVirtualStoreBinsError::LinkBins(error)),
    }
}

/// Compute `<slot>/node_modules/<pkg-or-@scope/pkg>` for the slot's
/// own package. The slot's package name lives on the lockfile key,
/// so no filesystem probing is needed (the directory is an invariant
/// maintained by [`crate::create_virtual_dir_by_snapshot`]). Scoped
/// names land at `<modules>/@scope/<name>`, unscoped names at
/// `<modules>/<name>`.
fn slot_own_pkg_dir(modules_dir: &Path, slot_key: &PackageKey) -> PathBuf {
    pkg_dir_under(modules_dir, &slot_key.name)
}

/// Join a package name onto a `node_modules` directory, handling the
/// `@scope/name` split into two path components. Operates on the raw
/// [`PkgName`] (whose `scope` and `bare` fields are already split),
/// not on the virtual-store-name form — for instance the input
/// represents `@types/node`, **not** `@types+node`.
fn pkg_dir_under(modules_dir: &Path, name: &PkgName) -> PathBuf {
    match &name.scope {
        Some(scope) => modules_dir.join(format!("@{scope}")).join(&name.bare),
        None => modules_dir.join(&name.bare),
    }
}

#[cfg(test)]
mod tests;
