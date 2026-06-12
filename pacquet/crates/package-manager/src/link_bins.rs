use crate::{PackageManifests, SkippedSnapshots};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_cmd_shim::{
    BinOrigin, FsCreateDirAll, FsEnsureExecutableBits, FsReadDir, FsReadFile, FsReadHead,
    FsReadToString, FsSetExecutable, FsWalkFiles, FsWrite, Host, LinkBinsError, PackageBinSource,
    link_bins_of_packages,
};
use pacquet_lockfile::{LockfileResolution, PackageKey, PackageMetadata, PkgName, SnapshotEntry};
use rayon::prelude::*;
use std::{
    collections::{HashMap, HashSet},
    fs, io,
    path::{Path, PathBuf},
    sync::Arc,
};

/// Read the `package.json` of every direct dependency under `modules_dir`
/// and link its bins into `<modules_dir>/.bin`.
///
/// `dep_names` is the list of direct-dependency keys as they appear in
/// `package.json`, the same names already symlinked under `<modules_dir>/`
/// by [`crate::SymlinkDirectDependencies`]. We resolve `package.json` via
/// the symlink (`fs::read` follows it transparently) so the read targets
/// the real package contents in the virtual store.
///
/// Driven on rayon because each location's read+parse is independent.
/// Mirrors pnpm v11's `linkBinsOfPackages` call site for direct deps:
/// <https://github.com/pnpm/pnpm/blob/4750fd370c/installing/deps-installer/src/install/index.ts#L1539>.
pub fn link_direct_dep_bins(modules_dir: &Path, dep_names: &[String]) -> Result<(), LinkBinsError> {
    let direct_dep_locations: Vec<PathBuf> =
        dep_names.iter().map(|name| modules_dir.join(name)).collect();
    // Swallow only `NotFound`: a direct-dep symlink target can
    // legitimately be missing right after a partial pacquet run, or
    // be an in-progress install. Every other IO error (permission
    // denied, EIO, etc.) and every JSON parse error must surface as
    // `LinkBinsError::{ReadManifest, ParseManifest}` so the failure
    // is diagnosable rather than hiding behind a missing `.bin`
    // entry. Matches the read-side error policy in
    // `pacquet_cmd_shim::link_bins`.
    let bin_sources: Vec<PackageBinSource> = direct_dep_locations
        .par_iter()
        .filter_map(|location| {
            let manifest_path = location.join("package.json");
            let bytes = match fs::read(&manifest_path) {
                Ok(bytes) => bytes,
                Err(error) if error.kind() == io::ErrorKind::NotFound => return None,
                Err(error) => {
                    return Some(Err(LinkBinsError::ReadManifest { path: manifest_path, error }));
                }
            };
            let manifest: serde_json::Value = match serde_json::from_slice(&bytes) {
                Ok(manifest) => manifest,
                Err(error) => {
                    return Some(Err(LinkBinsError::ParseManifest { path: manifest_path, error }));
                }
            };
            Some(Ok(PackageBinSource::new(location.clone(), Arc::new(manifest))))
        })
        .collect::<Result<_, _>>()?;
    if bin_sources.is_empty() {
        return Ok(());
    }
    link_bins_of_packages::<Host>(&bin_sources, &modules_dir.join(".bin"))
}

/// Top-level bin link that mixes direct-dep candidates and hoisted
/// (`publicly_hoisted_aliases_with_bins`) candidates in a single
/// [`link_bins_of_packages`] call so `pacquet_cmd_shim::pick_winner` (private)
/// can apply [`BinOrigin::Direct`] precedence over
/// [`BinOrigin::Hoisted`]. Mirrors upstream's
/// [`linkBinsOfImporter`](https://github.com/pnpm/pnpm/blob/4750fd370c/installing/deps-installer/src/install/index.ts#L1539)
/// and [`preferDirectCmds`](https://github.com/pnpm/pnpm/blob/4750fd370c/bins/linker/src/index.ts#L92)
/// — a hoisted (transitive) dep's bin must never shadow a direct
/// dep's bin with the same name.
///
/// Two-list shape (rather than a single tagged list) keeps the call
/// site cheap: callers already have these names in separate
/// collections — direct deps come from the importer's
/// `dependencies` / `devDependencies` / `optionalDependencies`,
/// hoisted aliases come from the hoist-result's
/// `publicly_hoisted_aliases_with_bins`. Joining them upthread
/// would force every caller to allocate a tagged `Vec`.
///
/// Ports of upstream pnpm's flow that lifecycle-script-created
/// bins should pick up the post-install state of `package.json`
/// (a `postinstall` script can write a binary that didn't exist at
/// extract time and pacquet must shim it). The caller schedules
/// this pass *after* `BuildModules` runs so the manifests-on-disk
/// reflect the post-script state.
pub fn link_top_level_bins(
    modules_dir: &Path,
    direct_dep_names: &[String],
    hoisted_dep_names: &[String],
) -> Result<(), LinkBinsError> {
    let mut bin_sources: Vec<PackageBinSource> = Vec::new();
    // Tag direct deps as `Direct` and hoisted as `Hoisted` so the
    // single downstream `pick_winner` call resolves conflicts via
    // the new [`BinOrigin`] tier.
    for source in read_bin_sources(modules_dir, direct_dep_names)? {
        bin_sources.push(source.with_origin(BinOrigin::Direct));
    }
    // Skip hoisted aliases that already appear under a direct
    // name. Reading the same `package.json` twice wouldn't change
    // the outcome — `pick_winner` would pick the Direct copy
    // anyway — but the work is wasted, and de-duplicating here
    // mirrors upstream's
    // [`preferDirectCmds`](https://github.com/pnpm/pnpm/blob/4750fd370c/bins/linker/src/index.ts#L92)
    // partition shape (filter out hoisted candidates whose name
    // already appears in the direct set).
    let direct_set: HashSet<&str> = direct_dep_names.iter().map(String::as_str).collect();
    let hoisted_only: Vec<String> = hoisted_dep_names
        .iter()
        .filter(|name| !direct_set.contains(name.as_str()))
        .cloned()
        .collect();
    for source in read_bin_sources(modules_dir, &hoisted_only)? {
        bin_sources.push(source.with_origin(BinOrigin::Hoisted));
    }
    if bin_sources.is_empty() {
        return Ok(());
    }
    link_bins_of_packages::<Host>(&bin_sources, &modules_dir.join(".bin"))
}

/// Read each `<modules_dir>/<name>/package.json` and assemble the
/// list of [`PackageBinSource`]s. Same `NotFound`-tolerant /
/// other-IO-fatal policy as [`link_direct_dep_bins`]; factored out
/// so [`link_top_level_bins`] can reuse the read pass for both
/// direct and hoisted candidate lists.
fn read_bin_sources(
    modules_dir: &Path,
    dep_names: &[String],
) -> Result<Vec<PackageBinSource>, LinkBinsError> {
    let locations: Vec<PathBuf> = dep_names.iter().map(|name| modules_dir.join(name)).collect();
    locations
        .par_iter()
        .filter_map(|location| {
            let manifest_path = location.join("package.json");
            let bytes = match fs::read(&manifest_path) {
                Ok(bytes) => bytes,
                Err(error) if error.kind() == io::ErrorKind::NotFound => return None,
                Err(error) => {
                    return Some(Err(LinkBinsError::ReadManifest { path: manifest_path, error }));
                }
            };
            let manifest: serde_json::Value = match serde_json::from_slice(&bytes) {
                Ok(manifest) => manifest,
                Err(error) => {
                    return Some(Err(LinkBinsError::ParseManifest { path: manifest_path, error }));
                }
            };
            Some(Ok(PackageBinSource::new(location.clone(), Arc::new(manifest))))
        })
        .collect()
}

/// Error type of [`LinkVirtualStoreBins`].
#[derive(Debug, Display, Error, Diagnostic)]
pub enum LinkVirtualStoreBinsError {
    #[display("Failed to read virtual store directory at {dir:?}: {error}")]
    #[diagnostic(code(pacquet_package_manager::read_virtual_store))]
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
/// This mirrors `linkBinsOfDependencies` in pnpm's `building/during-install`
/// (see <https://github.com/pnpm/pnpm/blob/4750fd370c/building/during-install/src/index.ts#L258-L309>).
/// pnpm walks each `depNode`, takes its `children` (its direct deps in the
/// resolved graph) and writes their bins into
/// `<depNode.dir>/node_modules/.bin`.
///
/// Pacquet's virtual store layout already exposes a slot's children as
/// siblings via [`create_symlink_layout`](crate::create_symlink_layout()).
/// So once the symlinks exist, walking
/// the slot's `node_modules` and excluding the package itself gives the same
/// child-set pnpm uses, and the bins go into the package's own
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
/// looked up in `package_manifests` rather than read off disk —
/// matching pnpm's `linkBinsOfDependencies` which consumes
/// `bundledManifest` straight out of the `SQLite` store index (see
/// <https://github.com/pnpm/pnpm/blob/4750fd370c/building/during-install/src/index.ts#L289>).
/// When `snapshots` is `None` (install without a lockfile), the
/// linker falls back to enumerating slots and reading manifests via
/// the filesystem, the shape this code had before the
/// lockfile-driven path landed.
#[must_use]
pub struct LinkVirtualStoreBins<'a> {
    /// Install-scoped slot-directory mapping (GVS-aware). Replaces the
    /// previous `virtual_store_dir: &Path` field — the layout already
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
    /// Lockfile `packages:` section, indexed by `PkgNameVerPeer`
    /// (without peer suffix). Used to filter children by
    /// `hasBin == true` *before* any per-child IO — mirrors pnpm's
    /// `dep.hasBin` filter in
    /// [`linkBinsOfDependencies`](https://github.com/pnpm/pnpm/blob/4750fd370c/building/during-install/src/index.ts#L283).
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
        let LinkVirtualStoreBins { layout, snapshots, packages, package_manifests, skipped } = self;
        if let Some(snapshots) = snapshots {
            let has_bin_set = build_has_bin_set(packages);
            run_lockfile_driven::<Sys>(
                layout,
                snapshots,
                has_bin_set.as_ref(),
                package_manifests,
                skipped,
            )
        } else {
            // No snapshots (lockfile absent or empty): fall back to a
            // `read_dir` enumeration. This path only fires for non-
            // frozen installs, which <https://github.com/pnpm/pacquet/issues/432> doesn't activate GVS for, so
            // reading from `layout.package_store_dir()` reproduces
            // today's behaviour exactly when GVS is off.
            run_with_readdir::<Sys>(layout.package_store_dir())
        }
    }
}

/// Pre-compute the set of package keys whose lockfile metadata sets
/// `hasBin: true`. Mirrors pnpm's filter at
/// [`during-install/src/index.ts:283`](https://github.com/pnpm/pnpm/blob/4750fd370c/building/during-install/src/index.ts#L283):
/// most packages don't declare a bin, so short-circuiting the
/// per-child manifest lookup with this set is the cheapest win on
/// warm-cache installs.
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
///   immediately. Conflating this case with `None` (the bug Copilot
///   flagged at <https://github.com/pnpm/pacquet/pull/333#discussion_r3222807548>)
///   would force per-child work the lockfile already ruled out.
fn build_has_bin_set(
    packages: Option<&HashMap<PackageKey, PackageMetadata>>,
) -> Option<HashSet<PackageKey>> {
    let packages = packages?;
    Some(
        packages
            .iter()
            .filter(|(_, meta)| {
                // Runtime resolutions (`Binary` / `Variations`)
                // always synthesize a `package.json` carrying the
                // lockfile-declared `bin` field
                // (see `synthesize_runtime_manifest_bytes` in
                // `install_package_by_snapshot.rs`), so they
                // always have bins — even when `hasBin` is absent
                // from the lockfile metadata (which pnpm v11 does
                // not emit for runtime entries today). Including
                // them here unconditionally keeps the bin-link
                // dispatch consistent with the synthesis step.
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

/// Walk the lockfile's `snapshots:` map, build each slot's bin output
/// directory lexically, and link every child's bins into it. The
/// child set comes from `snapshot.dependencies` +
/// `snapshot.optional_dependencies`, filtered by `has_bin_set` so
/// packages that don't declare a bin never make it into the
/// per-slot path-building or manifest-lookup work. The corresponding
/// manifest comes from [`PackageManifests`] (no disk read) or, for
/// cold-batch packages that prefetch missed, a fallback
/// `package.json` read through the existing symlink at
/// `<slot>/node_modules/<alias>`.
fn run_lockfile_driven<Sys>(
    layout: &crate::VirtualStoreLayout,
    snapshots: &HashMap<PackageKey, SnapshotEntry>,
    has_bin_set: Option<&HashSet<PackageKey>>,
    package_manifests: &PackageManifests,
    skipped: &SkippedSnapshots,
) -> Result<(), LinkVirtualStoreBinsError>
where
    Sys: FsReadFile
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
    let slot_entries: Vec<(&PackageKey, &SnapshotEntry)> =
        snapshots.iter().filter(|(slot_key, _)| !skipped.contains(slot_key)).collect();
    slot_entries.par_iter().try_for_each(|(slot_key, snapshot)| {
        let children = snapshot
            .dependencies
            .iter()
            .flatten()
            .chain(snapshot.optional_dependencies.iter().flatten());

        // First pass: figure out which packages contribute a bin to
        // this slot's `node_modules/.bin`. Two kinds:
        //
        // 1. Every child whose manifest declares `bin`. Cheap to
        //    detect via `has_bin_set` (pre-built from the lockfile's
        //    `packages:` rows). Without a child or a self-bin the
        //    slot needs no `.bin` directory at all, so the early
        //    return below skips ~95% of slots on a real-world
        //    lockfile (measured on the integrated-benchmark
        //    fixture).
        //
        // 2. The slot's own package, when it carries a bin. Pnpm's
        //    [`linkBinsOfDependencies`](https://github.com/pnpm/pnpm/blob/29a42efc3b/building/during-install/src/index.ts#L272-L298)
        //    appends `depNode` to the bin-source list unconditionally
        //    (line 287) and lets the inner reader's manifest check
        //    drop self when there's nothing to write — so for a
        //    package like `hello-world-js-bin` (no deps, one bin)
        //    pnpm writes `<slot>/node_modules/<pkg>/node_modules/.bin/<pkg>`
        //    as a self-shim. An earlier version of this function
        //    skipped the self-bin on the assumption that pnpm did the
        //    same. The
        //    `same_global_virtual_store_layout_*` parity tests
        //    surfaced that assumption as a divergence: pnpm did write
        //    the self-shim. Mirror it here.
        let with_bin: Vec<(&PkgName, PackageKey)> = children
            .filter_map(|(alias, dep_ref)| {
                // `link:` deps live outside the virtual store and
                // expose their bins via the workspace project's
                // own `package.json`, not through a snapshot — skip
                // them here.
                let child_key = dep_ref.resolve(alias)?;
                let metadata_key = child_key.without_peer();
                let keep = match has_bin_set {
                    Some(set) => set.contains(&metadata_key),
                    None => true,
                };
                keep.then_some((alias, metadata_key))
            })
            .collect();
        let self_metadata_key = slot_key.without_peer();
        let self_has_bin = match has_bin_set {
            Some(set) => set.contains(&self_metadata_key),
            // No `has_bin_set` — fall back to the conservative
            // include-self path. The downstream manifest read in
            // `link_bins_of_packages` filters out a self with no
            // actual `bin` field, so an over-inclusion at this gate
            // costs at most one `package.json` read.
            None => true,
        };
        if with_bin.is_empty() && !self_has_bin {
            return Ok(());
        }

        let slot_dir = layout.slot_dir(slot_key);
        let modules_dir = slot_dir.join("node_modules");
        let self_pkg_dir = slot_own_pkg_dir(&modules_dir, slot_key);
        let bins_dir = self_pkg_dir.join("node_modules/.bin");

        let mut bin_sources: Vec<PackageBinSource> =
            Vec::with_capacity(with_bin.len() + usize::from(self_has_bin));
        for (alias, metadata_key) in with_bin {
            let child_location = pkg_dir_under(&modules_dir, alias);
            if let Some(manifest) = package_manifests.get(&metadata_key) {
                // Hot path: parsed manifest already in memory from
                // the warm-cache prefetch. Both the prefetch map
                // and `PackageBinSource` hold the manifest via
                // [`Arc`], so this is a refcount bump rather than a
                // deep clone of the JSON tree. Avoids the
                // `slots × children`-sized clone fan-out that
                // dominated the previous version of this path on
                // warm-cache installs.
                bin_sources.push(PackageBinSource::new(child_location, Arc::clone(manifest)));
            } else {
                // Cold-batch fallback: package was downloaded
                // earlier in the run, so its row isn't in the
                // prefetched manifest map yet. Reading from disk
                // here is the same code path as the non-lockfile
                // install — see [`run_with_readdir`].
                match read_package::<Sys>(&child_location) {
                    Ok(Some(pkg)) => bin_sources.push(pkg),
                    Ok(None) => {}
                    Err(error) => return Err(LinkVirtualStoreBinsError::LinkBins(error)),
                }
            }
        }

        // Self-bin source (slot's own package), when its lockfile row
        // declared a bin. Same warm-vs-cold dispatch as the children
        // above. `self_pkg_dir` is an invariant of
        // [`crate::create_virtual_dir_by_snapshot`], so the cold
        // fallback is the same `read_package` used elsewhere.
        if self_has_bin {
            if let Some(manifest) = package_manifests.get(&self_metadata_key) {
                bin_sources.push(PackageBinSource::new(self_pkg_dir, Arc::clone(manifest)));
            } else {
                match read_package::<Sys>(&self_pkg_dir) {
                    Ok(Some(pkg)) => bin_sources.push(pkg),
                    Ok(None) => {}
                    Err(error) => return Err(LinkVirtualStoreBinsError::LinkBins(error)),
                }
            }
        }

        if bin_sources.is_empty() {
            return Ok(());
        }
        link_bins_of_packages::<Sys>(&bin_sources, &bins_dir)
            .map_err(LinkVirtualStoreBinsError::LinkBins)
    })
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

/// Fallback (non-lockfile) path: enumerate slots via `read_dir`,
/// then walk each slot's `node_modules` to discover children. Used
/// only by [`crate::InstallWithFreshLockfile`] today; the lockfile
/// path bypasses every directory enumeration in here.
fn run_with_readdir<Sys>(virtual_store_dir: &Path) -> Result<(), LinkVirtualStoreBinsError>
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
    let slots = match Sys::read_dir(virtual_store_dir) {
        Ok(slots) => slots,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(LinkVirtualStoreBinsError::ReadVirtualStore {
                dir: virtual_store_dir.to_path_buf(),
                error,
            });
        }
    };
    let slots: Vec<PathBuf> = slots.collect();
    slots.par_iter().try_for_each(|slot_dir| {
        let modules_dir = slot_dir.join("node_modules");
        let Some(self_pkg_dir) = find_slot_own_package_dir(slot_dir, &modules_dir) else {
            return Ok(());
        };
        // Probe the slot's own package directory before walking its
        // children. Without the probe, an incomplete slot whose
        // `node_modules/<pkg>` is missing but whose sibling deps are
        // still present would have `link_bins_excluding` collect the
        // siblings and `create_dir_all` the missing `<pkg>` chain to
        // hold the shims, leaving an orphan package directory on
        // disk. This path runs only for [`crate::InstallWithFreshLockfile`]
        // and visits ~direct-deps slots (small N), so the probe cost
        // is trivial; the lockfile-driven path bypasses this by
        // treating the slot's own pkg dir as an invariant of
        // [`crate::create_virtual_dir_by_snapshot`].
        if Sys::read_dir(&self_pkg_dir).is_err() {
            return Ok(());
        }
        let bins_dir = self_pkg_dir.join("node_modules/.bin");
        link_bins_excluding::<Sys>(&modules_dir, &bins_dir, &self_pkg_dir)
            .map_err(LinkVirtualStoreBinsError::LinkBins)
    })
}

/// Locate the slot's own package directory inside `<slot>/node_modules`.
///
/// The slot directory's name encodes the package name as
/// `<scope>+<name>@<version>` for the simple case (see
/// [`pacquet_lockfile::PkgNameVerPeer::to_virtual_store_name`]). For
/// peer-resolved slots the version segment itself contains additional
/// `@`-separated peer specs joined by `_`, e.g.
/// `ts-node@10.9.1_@types+node@18.7.19_typescript@5.1.6`. The `@` after
/// `typescript` is part of a peer's version, not the package-name
/// boundary. Parsing from the right (`rfind('@')`) would split there
/// and silently break peer-resolved slots; parse from the left
/// instead, skipping a leading `@` that belongs to a scoped package.
///
/// Returns `None` only when the slot name fails to parse — there's no
/// filesystem probe for the resolved candidate. The previous version
/// stat-equivalent-ed the path with `Sys::read_dir` to short-circuit
/// missing slots, but on a 1267-package fixture that was 1267
/// wasted `open(O_DIRECTORY) + close` round-trips on the hot path of
/// every warm install. The slot's own package directory is an
/// invariant of [`crate::create_virtual_dir_by_snapshot`]; the
/// downstream `link_bins_excluding` handles `NotFound` from its own
/// `read_dir` of `<slot>/node_modules` cleanly when the invariant
/// ever does break, so the probe is pure overhead.
fn find_slot_own_package_dir(slot_dir: &Path, modules_dir: &Path) -> Option<PathBuf> {
    let slot_name = slot_dir.file_name()?.to_str()?;

    // The package-name half is everything before the **first** `@`,
    // ignoring a single leading `@` that belongs to a scoped name
    // (`@scope+pkg@...` → start the `@` search at offset 1).
    // After `to_virtual_store_name`, `/` in scoped names becomes `+`,
    // so the package-name half can never contain `@` itself.
    let scoped = slot_name.starts_with('@');
    let search_start = usize::from(scoped);
    let at = search_start + slot_name[search_start..].find('@')?;
    let name_part = &slot_name[..at];

    // `+` separates `<scope>+<name>` for scoped packages, and *only*
    // for scoped packages. Gating on `scoped` avoids misparsing a
    // hypothetical unscoped name that contains `+`: `PkgName::parse`
    // does not reject non-URL-safe characters (only npm's
    // `validate-npm-package-name` warns about them), so an unscoped
    // name like `foo+bar` could in principle reach here and would
    // otherwise be split into `foo` / `bar`.
    let pkg_dir = match scoped.then(|| name_part.split_once('+')).flatten() {
        Some((scope, name)) => modules_dir.join(scope).join(name),
        None => modules_dir.join(name_part),
    };
    Some(pkg_dir)
}

/// Like [`pacquet_cmd_shim::link_bins`] but skipping the slot's own package
/// from the candidate set. Without this, a slot for `tsc@5.0.0` would link
/// its own `tsc` bin into its own `node_modules/.bin`, which pnpm doesn't.
fn link_bins_excluding<Sys>(
    modules_dir: &Path,
    bins_dir: &Path,
    exclude: &Path,
) -> Result<(), LinkBinsError>
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
    let mut packages: Vec<PackageBinSource> = Vec::new();

    let entries = match Sys::read_dir(modules_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(LinkBinsError::ReadModulesDir { dir: modules_dir.to_path_buf(), error });
        }
    };

    for path in entries {
        let Some(name) = path.file_name() else {
            continue;
        };
        let name_str = name.to_string_lossy();
        if name_str.starts_with('.') {
            continue;
        }

        if name_str.starts_with('@') {
            // Only `NotFound` is plausibly skippable here (a
            // concurrent scope-dir delete). Other errors —
            // permission denied, EIO, AppArmor deny — would mean
            // the bins for every package under this scope silently
            // disappear, so surface them instead of letting them
            // hide.
            let scope_entries = match Sys::read_dir(&path) {
                Ok(entries) => entries,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(LinkBinsError::ReadModulesDir { dir: path.clone(), error });
                }
            };
            for sub_path in scope_entries {
                if paths_eq(&sub_path, exclude) {
                    continue;
                }
                if let Some(pkg) = read_package::<Sys>(&sub_path)? {
                    packages.push(pkg);
                }
            }
            continue;
        }

        if paths_eq(&path, exclude) {
            continue;
        }
        if let Some(pkg) = read_package::<Sys>(&path)? {
            packages.push(pkg);
        }
    }

    if packages.is_empty() {
        return Ok(());
    }

    link_bins_of_packages::<Sys>(&packages, bins_dir)
}

fn read_package<Sys: FsReadFile>(
    location: &Path,
) -> Result<Option<PackageBinSource>, LinkBinsError> {
    let manifest_path = location.join("package.json");
    let bytes = match Sys::read_file(&manifest_path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(LinkBinsError::ReadManifest { path: manifest_path, error }),
    };
    let manifest: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|error| LinkBinsError::ParseManifest { path: manifest_path, error })?;
    Ok(Some(PackageBinSource::new(location.to_path_buf(), Arc::new(manifest))))
}

fn paths_eq(lhs: &Path, rhs: &Path) -> bool {
    // Lexical comparison is enough; both paths come from the same
    // `node_modules` walk and don't go through canonicalisation.
    lhs == rhs
}

#[cfg(test)]
mod tests;
