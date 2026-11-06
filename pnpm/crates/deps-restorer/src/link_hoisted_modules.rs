//! Hoisted-linker. Produces the on-disk `node_modules/` tree
//! described by [`crate::LockfileToDepGraphResult`]:
//! removes orphaned directories the new plan doesn't place,
//! imports each graph node into its computed directory via
//! [`crate::import_indexed_dir()`], and links bins under every
//! parent's `node_modules/.bin`.
//!
//! Pacquet's linker is synchronous and accepts pre-fetched CAS
//! paths via `cas_paths_by_pkg_id`. It decouples downloading from
//! linking because pacquet's existing tarball / store-dir /
//! package-fetch machinery is reused verbatim by the install
//! pipeline before the linker runs.
//!
//! Concurrency uses [`rayon`]: the hierarchy walk parallelizes
//! at each level, and `import_indexed_dir` itself is internally
//! rayon-parallel over CAS entries.

pub(crate) use bins::HoistedBinLinkOutput;
pub use bins::HoistedBinSources;
pub use dir_clone::HoistedDirCloneCache;

mod bins;

mod dir_clone;

mod orphans;

use crate::{
    DepHierarchy, DependenciesGraph, DependenciesGraphNode, ImportIndexedDirError,
    ImportIndexedDirOpts, import_indexed_dir,
};
use bins::{link_bundled_bins, link_hierarchy_bins};
use derive_more::{Display, Error};
use miette::Diagnostic;
use orphans::remove_orphans;
use pnpm_cmd_shim::{LinkBinsError, LinkBinsOptions};
use pnpm_lockfile::PkgIdWithPatchHash;
use pnpm_reporter::{
    LogEvent, LogLevel, ProgressLog, ProgressMessage, Reporter, StatsLog, StatsMessage,
};
use rayon::prelude::*;
use std::{
    collections::{BTreeSet, HashMap},
    io,
    path::{Path, PathBuf},
    sync::Arc,
};

/// Per-package CAS index. Keyed by [`crate::HoistedPackageMetadata::pkg_id_with_patch_hash`],
/// each entry maps a relative file path (the tarball's archive
/// path, e.g. `package/lib/index.js`) to its absolute location
/// inside the CAS. The hoisted linker accepts the same shape the
/// isolated path's
/// [`CreateVirtualDirBySnapshot.cas_paths`](crate::CreateVirtualDirBySnapshot::cas_paths)
/// field takes per snapshot — one entry per *package*, not per
/// directory, because a single package can land at multiple
/// directories (version conflict → some dirs nest under siblings)
/// and the CAS contents are the same regardless of where they're
/// extracted to.
pub type CasPathsByPkgId = HashMap<PkgIdWithPatchHash, HoistedPackageFiles>;

/// One package's entry in [`CasPathsByPkgId`].
#[derive(Debug, Clone)]
pub struct HoistedPackageFiles {
    pub cas_paths: Arc<pnpm_fs::CasPaths>,
    /// Whether [`Self::cas_paths`] points at mutable local source, taken
    /// from the fetch's effective resolution. See
    /// [`crate::SlotImportSource::is_mutable`].
    pub source_is_mutable: bool,
    /// Whether the mutable source was there to read from. See
    /// [`crate::SlotImportSource::source_exists`].
    pub source_exists: bool,
    /// Whether a build or a patch will write to the imported files. See
    /// [`crate::SlotImportSource::needs_build`].
    pub needs_build: bool,
}

impl HoistedPackageFiles {
    /// Whether a package already installed from these files has to be
    /// imported again. A mutable source can change without the lockfile
    /// changing. A missing one has only an empty file map to offer, which
    /// would replace the installed copy with nothing.
    #[must_use]
    pub fn refreshes_installed_copy(&self) -> bool {
        self.source_is_mutable && self.source_exists
    }
}

impl From<Arc<pnpm_fs::CasPaths>> for HoistedPackageFiles {
    /// Content-addressed files, which are never mutable, of a package
    /// nothing will write to.
    fn from(cas_paths: Arc<pnpm_fs::CasPaths>) -> Self {
        HoistedPackageFiles {
            cas_paths,
            source_is_mutable: false,
            source_exists: true,
            needs_build: false,
        }
    }
}

/// Inputs the linker reads from. Borrows everything so callers
/// can keep ownership of the graph / CAS state — the linker
/// doesn't mutate anything but the on-disk tree.
#[derive(Debug)]
pub struct LinkHoistedModulesOpts<'a> {
    pub import: crate::PackageImportOptions<'a>,
    pub dir_clone_cache: Option<&'a HoistedDirCloneCache<'a>>,
    pub graph: &'a DependenciesGraph,
    /// The package directories the previous install placed. Each one
    /// `graph` does not place is removed. Empty for a fresh install; the
    /// on-disk scan still runs, since that is exactly the state an
    /// interrupted install leaves behind.
    pub previous_dirs: &'a BTreeSet<PathBuf>,
    /// Per-importer directory hierarchies, keyed by importer
    /// root. Single-importer installs have one entry keyed by
    /// `lockfile_dir`.
    pub hierarchy: &'a std::collections::BTreeMap<PathBuf, DepHierarchy>,
    /// Where each importer root of [`hierarchy`](Self::hierarchy) keeps
    /// its dependencies. Its lockfile directory is the containment root
    /// for orphan removal: an orphan directory that does not sit
    /// lexically inside it is skipped, never deleted. The walker builds
    /// every graph dir through `safe_join_modules_dir`, so a confined
    /// path is the invariant. This keeps the deletion site from
    /// depending on the constructor's discipline.
    pub modules_dirs: &'a crate::ImporterModulesDirs,
    /// Pre-fetched CAS file index per package.
    pub cas_paths_by_pkg_id: &'a CasPathsByPkgId,
    /// Options for every bin this pass links — the
    /// [`crate::shim_link_options`] output for the hoisted linker
    /// (no `extraNodePaths`; hoisted-tree shims never carry
    /// `NODE_PATH`, which pnpm gates on the isolated linker).
    pub link_options: &'a LinkBinsOptions,
}

/// Failure modes of [`link_hoisted_modules`]. Marked
/// `#[non_exhaustive]` so adding variants isn't a breaking API
/// change.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum LinkHoistedModulesError {
    /// Indicates a bug in the caller (pre-fetch incomplete) — the
    /// linker can't conjure files it wasn't given.
    #[display("Missing CAS paths for required package {pkg_id_with_patch_hash:?} at {dir:?}")]
    #[diagnostic(code(ERR_PNPM_LINK_HOISTED_MISSING_CAS))]
    MissingCasPaths { pkg_id_with_patch_hash: PkgIdWithPatchHash, dir: PathBuf },

    /// A hierarchy entry referenced a directory that has no
    /// corresponding entry in `graph`. The walker inserts
    /// a graph node every time it inserts a hierarchy entry, so
    /// this shouldn't fire from a real walker result — but
    /// surfacing the inconsistency fails the install fast rather
    /// than producing a partial layout.
    #[display("Hierarchy references {dir:?} but no matching graph node exists")]
    #[diagnostic(code(ERR_PNPM_LINK_HOISTED_MISSING_GRAPH_NODE))]
    MissingGraphNode { dir: PathBuf },

    /// An importer's `node_modules`, or one entry in it, could not be
    /// inspected for the orphan scan. Removal failures are tolerated,
    /// but a path we cannot even read would silently leave orphans
    /// behind — the same distinction pnpm draws between
    /// `readModulesDir`, which only swallows a missing directory, and
    /// `tryRemoveDir`, which swallows everything.
    #[display("Failed to read {path:?} while scanning for orphaned directories")]
    #[diagnostic(code(ERR_PNPM_LINK_HOISTED_READ_MODULES_DIR))]
    ReadModulesDir { path: PathBuf, source: io::Error },

    #[diagnostic(transparent)]
    ImportIndexedDir(#[error(source)] ImportIndexedDirError),

    #[diagnostic(transparent)]
    LinkBins(#[error(source)] LinkBinsError),
}

/// Produce the on-disk hoisted tree from a walk result.
///
/// 1. **Orphan removal.** Every directory the previous install
///    placed but the new plan doesn't, plus every package
///    directory found in an importer's `node_modules` that the
///    new plan doesn't place, is silently `rimraf`'d. Removal
///    happens *before* any insert so the linker doesn't race
///    against itself when a directory name is reused for a
///    different package version.
/// 2. **Per-node import.** The hierarchy is walked top-down,
///    parallel at each level. For every node the previous install did
///    not already leave in place ([`DependenciesGraphNode::present`])
///    the linker calls [`import_indexed_dir()`] with `force: true,
///    keep_modules_dir: true`.
/// 3. **Per-`node_modules` bin link.** After a level's children
///    are all done, `<parent>/node_modules/.bin` is populated
///    from the just-imported direct children's `package.json`.
///
/// Returns the nested `node_modules` directories whose `.bin` held back a bin,
/// for the build phase to link again.
pub fn link_hoisted_modules<Reporter: self::Reporter>(
    opts: &LinkHoistedModulesOpts<'_>,
) -> Result<Vec<HeldBackBinsDir>, LinkHoistedModulesError> {
    link_hoisted_modules_with_sources::<Reporter>(opts).map(|bins| bins.held_back_bins_dirs)
}

pub(crate) fn link_hoisted_modules_with_sources<Reporter: self::Reporter>(
    opts: &LinkHoistedModulesOpts<'_>,
) -> Result<HoistedBinLinkOutput, LinkHoistedModulesError> {
    let removed = remove_orphans(opts)?;

    // Drive each importer's hierarchy in parallel — workspace
    // installs have multiple importers; the
    // single-importer case has one and rayon's overhead is
    // negligible.
    let LinkedLevel { imported: added, bins } = crate::link_pool::in_link_pool(|| {
        opts.hierarchy
            .par_iter()
            .map(|(project_dir, deps_hierarchy)| {
                let modules_dir = opts.modules_dirs.of(project_dir);
                link_all_pkgs_in_order::<Reporter>(deps_hierarchy, &modules_dir, true, opts)
            })
            .collect::<Result<Vec<LinkedLevel>, _>>()
    })?
    .into_iter()
    .fold(LinkedLevel::default(), LinkedLevel::merge);

    // The hoisted linker owns both of the install's `pnpm:stats`
    // emissions: pnpm emits `removed` from `linkHoistedModules` and the
    // isolated linker takes its own pair from `CreateVirtualStore` and
    // `PruneStaleModules`, neither of which emits here. `added` counts
    // the packages this install imported rather than every node in the
    // graph, as pnpm's `depNodes.filter(({ fetching }) => fetching)`
    // does. `added` goes out first, the order both pnpm and the
    // isolated linker emit the pair in.
    Reporter::emit(&LogEvent::Stats(StatsLog {
        level: LogLevel::Debug,
        message: StatsMessage::Added { prefix: opts.import.requester.to_owned(), added },
    }));
    Reporter::emit(&LogEvent::Stats(StatsLog {
        level: LogLevel::Debug,
        message: StatsMessage::Removed { prefix: opts.import.requester.to_owned(), removed },
    }));

    Ok(bins)
}

/// The modules directory physically containing a scoped or unscoped package.
pub(crate) fn containing_modules_dir(pkg_dir: &Path) -> Option<&Path> {
    let parent = pkg_dir.parent()?;
    let is_scope = parent
        .file_name()
        .is_some_and(|name| name.as_encoded_bytes().starts_with(b"@"));
    if is_scope { parent.parent() } else { Some(parent) }
}

/// Phase 2 + 3: recursively import packages then link bins for
/// each `<parent>/node_modules/.bin`.
///
/// Each level of the hierarchy is walked in parallel via
/// rayon's [`IntoParallelRefIterator::par_iter`]. Children at the
/// same level race against each other; the bin-link pass for
/// `parent_dir/node_modules` runs only after every immediate
/// child (and its subtree) has been imported, so the read of
/// `<modules_dir>/<dep>/package.json` during bin linking always
/// sees the fully-populated package.
///
/// [`IntoParallelRefIterator::par_iter`]: rayon::iter::IntoParallelRefIterator::par_iter
fn link_all_pkgs_in_order<Reporter: self::Reporter>(
    hierarchy: &DepHierarchy,
    modules_dir: &Path,
    is_project_root: bool,
    opts: &LinkHoistedModulesOpts<'_>,
) -> Result<LinkedLevel, LinkHoistedModulesError> {
    // Phase 2: import this level's packages + recurse into each
    // one's children. `par_iter` is sufficient — the side effects
    // are on disk and target disjoint directories.
    let mut linked = hierarchy.0
        .par_iter()
        .map(|(dir, sub_hierarchy)| {
            let node = opts.graph
                .get(dir)
                .ok_or_else(|| LinkHoistedModulesError::MissingGraphNode { dir: dir.clone() })?;
            let here = u64::from(import_node::<Reporter>(node, opts)?);
            let below = link_all_pkgs_in_order::<Reporter>(
                sub_hierarchy,
                &dir.join("node_modules"),
                false,
                opts,
            )?;
            Ok(LinkedLevel { imported: here, ..LinkedLevel::default() }.merge(below))
        })
        .collect::<Result<Vec<LinkedLevel>, LinkHoistedModulesError>>()?
        .into_iter()
        .fold(LinkedLevel::default(), LinkedLevel::merge);

    let mut bins = link_hierarchy_bins(hierarchy, modules_dir, opts)?;
    // A project's `.bin` is linked again after the builds anyway.
    if is_project_root {
        bins.held_back_bins_dirs.clear();
    }
    linked.bins.merge(bins);
    linked.bins.merge(link_bundled_bins(hierarchy, opts)?);

    Ok(linked)
}

/// A nested `node_modules` whose `.bin` held back a bin while the builds that
/// may create its target were pending. The build phase links the bins of its
/// `dep_names` again once they ran.
#[derive(Debug, Clone)]
pub struct HeldBackBinsDir {
    pub modules_dir: PathBuf,
    pub dep_names: Vec<String>,
}

/// What a subtree of [`link_all_pkgs_in_order`] did: how many packages it
/// imported, and its nested `.bin` directories that held back a bin.
#[derive(Default)]
struct LinkedLevel {
    imported: u64,
    bins: HoistedBinLinkOutput,
}

impl LinkedLevel {
    fn merge(mut self, other: LinkedLevel) -> LinkedLevel {
        self.imported += other.imported;
        self.bins.merge(other.bins);
        self
    }
}

/// Import one graph node into its target `dir`. `Ok(false)` when
/// nothing was written: the package is already in place, or it is an
/// optional package with no files to import.
fn import_node<Reporter: self::Reporter>(
    node: &DependenciesGraphNode,
    opts: &LinkHoistedModulesOpts<'_>,
) -> Result<bool, LinkHoistedModulesError> {
    // The previous install put this package here and the directory
    // still holds a `package.json` of the recorded version; importing
    // it again would stage-and-swap the whole directory for nothing.
    if node.present {
        return Ok(false);
    }
    let Some(files) = opts.cas_paths_by_pkg_id.get(&node.package.pkg_id_with_patch_hash) else {
        if node.optional {
            return Ok(false);
        }
        return Err(LinkHoistedModulesError::MissingCasPaths {
            pkg_id_with_patch_hash: node.package.pkg_id_with_patch_hash.clone(),
            dir: node.dir.clone(),
        });
    };

    let cas_paths = &*files.cas_paths;
    let import_method = opts.import.method_for(files.source_is_mutable, files.needs_build);
    if files.source_is_mutable
        || !opts.dir_clone_cache.is_some_and(|cache| {
            cache.try_import::<Reporter>(node, opts.import, cas_paths)
        })
    {
        import_indexed_dir::<Reporter>(
            opts.import.logged_methods,
            import_method,
            &node.dir,
            cas_paths,
            hoisted_import_opts(files, opts.import.swap_lock),
        )
        .map_err(LinkHoistedModulesError::ImportIndexedDir)?;
    }

    // `pnpm:progress imported` — see the matching emit in
    // `create_virtual_dir_by_snapshot::run` for the rationale on the
    // optimistic `method` value. Under `nodeLinker: hoisted` that emit
    // never runs (no virtual-store slot is written), so this is the
    // only source of the reporter's `added` counter.
    Reporter::emit(&LogEvent::Progress(ProgressLog {
        level: LogLevel::Debug,
        message: ProgressMessage::Imported {
            method: crate::optimistic_wire_method(import_method),
            requester: opts.import.requester.to_owned(),
            to: node.dir.to_string_lossy().into_owned(),
        },
    }));

    Ok(true)
}

/// A hoisted package replaces whatever is at its directory but keeps the
/// nested `node_modules` other nodes were hoisted into. A directory
/// dependency keeps its own symlinks. A missing mutable source leaves an
/// existing directory alone, as it does for a virtual-store slot.
fn hoisted_import_opts<'a>(
    files: &HoistedPackageFiles,
    swap_lock: Option<&'a crate::ModulesSwapLock>,
) -> ImportIndexedDirOpts<'a> {
    ImportIndexedDirOpts {
        force: files.source_exists || !files.source_is_mutable,
        keep_modules_dir: true,
        symlink_targets: files.source_is_mutable.then_some(crate::SymlinkTargets::Imported),
        swap_lock,
        ..ImportIndexedDirOpts::default()
    }
}

#[cfg(test)]
mod tests;
