//! Hoisted-linker. Produces the on-disk `node_modules/` tree
//! described by Slice 4's [`crate::LockfileToDepGraphResult`]:
//! removes orphaned directories from the previous install,
//! imports each graph node into its computed directory via
//! [`crate::import_indexed_dir()`], and links bins under every
//! parent's `node_modules/.bin`.
//!
//! Pacquet's linker is synchronous and accepts pre-fetched CAS
//! paths via `cas_paths_by_pkg_id`. It decouples downloading from
//! linking because pacquet's existing tarball / store-dir /
//! package-fetch machinery is reused verbatim by the install
//! pipeline (Slice 6) before the linker runs. The linker is the
//! final composition step — given a graph and a fully-populated
//! CAS index for every package, it materializes the tree.
//!
//! Concurrency uses [`rayon`]: the hierarchy walk parallelizes
//! at each level, and `import_indexed_dir` itself is internally
//! rayon-parallel over CAS entries.

pub use dir_clone::HoistedDirCloneCache;

mod dir_clone;

use crate::{
    DepHierarchy, DependenciesGraph, DependenciesGraphNode, ImportIndexedDirError,
    ImportIndexedDirOpts, import_indexed_dir, prune_direct_deps::remove_dep_bins,
};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_cmd_shim::{
    Host, LinkBinsError, LinkBinsOptions, PackageBinSource, ShimTargetCache,
    collect_packages_in_modules_dir, link_bins_of_packages_cached,
};
use pnpm_lockfile::{LockfileResolution, PkgIdWithPatchHash};
use pnpm_reporter::{
    LogEvent, LogLevel, ProgressLog, ProgressMessage, Reporter, StatsLog, StatsMessage,
};
use rayon::prelude::*;
use std::{
    collections::HashMap,
    fs, io,
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
pub type CasPathsByPkgId = HashMap<PkgIdWithPatchHash, Arc<HashMap<String, PathBuf>>>;

/// Inputs the linker reads from. Borrows everything so callers
/// can keep ownership of the graph / CAS state — the linker
/// doesn't mutate anything but the on-disk tree.
#[derive(Debug)]
pub struct LinkHoistedModulesOpts<'a> {
    pub import: crate::PackageImportOptions<'a>,
    pub dir_clone_cache: Option<&'a HoistedDirCloneCache<'a>>,
    pub graph: &'a DependenciesGraph,
    /// Diffed against `graph` to compute orphans. `None` for a
    /// fresh install (no prior lockfile) — no orphans to remove.
    pub prev_graph: Option<&'a DependenciesGraph>,
    /// Per-importer directory hierarchies, keyed by importer
    /// root. Single-importer installs have one entry keyed by
    /// `lockfile_dir`; workspace support will add more.
    pub hierarchy: &'a std::collections::BTreeMap<PathBuf, DepHierarchy>,
    /// Pre-fetched CAS file index per package.
    pub cas_paths_by_pkg_id: &'a CasPathsByPkgId,
    /// Containment root for orphan removal: an orphan directory that
    /// does not sit lexically inside this root is skipped, never
    /// deleted. The walker builds every graph dir through
    /// `safe_join_modules_dir`, so a confined path is the invariant —
    /// this keeps the deletion site from depending on the
    /// constructor's discipline.
    pub confine_root: &'a Path,
    /// Options for every bin this pass links — the
    /// [`crate::shim_link_options`] output for the hoisted linker
    /// (no `extraNodePaths`; hoisted-tree shims never carry
    /// `NODE_PATH`, which pnpm gates on the isolated linker).
    pub link_options: &'a LinkBinsOptions,
}

/// Failure modes of [`link_hoisted_modules`]. Marked
/// `#[non_exhaustive]` so adding variants in later sub-slices
/// (e.g. side-effects cache, store-controller integration)
/// isn't a breaking API change.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum LinkHoistedModulesError {
    /// Indicates a bug in the caller (pre-fetch incomplete) — the
    /// linker can't conjure files it wasn't given.
    #[display("Missing CAS paths for required package {pkg_id_with_patch_hash:?} at {dir:?}")]
    #[diagnostic(code(ERR_PNPM_LINK_HOISTED_MISSING_CAS))]
    MissingCasPaths { pkg_id_with_patch_hash: PkgIdWithPatchHash, dir: PathBuf },

    /// A hierarchy entry referenced a directory that has no
    /// corresponding entry in `graph`. Slice 4's walker inserts
    /// a graph node every time it inserts a hierarchy entry, so
    /// this shouldn't fire from a real walker result — but
    /// surfacing the inconsistency fails the install fast rather
    /// than producing a partial layout.
    #[display("Hierarchy references {dir:?} but no matching graph node exists")]
    #[diagnostic(code(ERR_PNPM_LINK_HOISTED_MISSING_GRAPH_NODE))]
    MissingGraphNode { dir: PathBuf },

    #[diagnostic(transparent)]
    ImportIndexedDir(#[error(source)] ImportIndexedDirError),

    #[diagnostic(transparent)]
    LinkBins(#[error(source)] LinkBinsError),
}

/// Produce the on-disk hoisted tree from a Slice 4 walk result.
///
/// 1. **Orphan removal.** Every directory in `prev_graph` but
///    not in `graph` is silently `rimraf`'d, together with the
///    bins it linked into its parent's `node_modules/.bin`. Removal happens
///    *before* any insert so the linker doesn't race against
///    itself when a directory name is reused for a different
///    package version.
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
    let removed = remove_orphans(opts.graph, opts.prev_graph, opts.confine_root);

    // Drive each importer's hierarchy in parallel — workspace
    // installs (Slice 9) will have multiple importers; the
    // single-importer case has one and rayon's overhead is
    // negligible.
    let LinkedLevel {
        imported: added,
        held_back_bins_dirs,
    } = opts.hierarchy
        .par_iter()
        .map(|(parent_dir, deps_hierarchy)| {
            link_all_pkgs_in_order::<Reporter>(deps_hierarchy, parent_dir, true, opts)
        })
        .collect::<Result<Vec<LinkedLevel>, _>>()?
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

    Ok(held_back_bins_dirs)
}

/// Phase 1: rimraf every directory that was in the previous
/// install's graph but isn't in the new one, after unlinking the
/// bins it declared from its `node_modules/.bin`. A failed bin unlink
/// is logged and does not keep the directory. Directory removal errors
/// are swallowed silently with the same `EPERM`/`EBUSY` tolerance — a directory
/// we can't remove right now is no worse than leaving a stale
/// entry, and the next install will retry. Returns the orphan count
/// (attempted, not necessarily removed — the same number pnpm's
/// `dirsToRemove.length` reports).
fn remove_orphans(
    graph: &DependenciesGraph,
    prev_graph: Option<&DependenciesGraph>,
    confine_root: &Path,
) -> u64 {
    let Some(prev) = prev_graph else { return 0 };
    let orphan_dirs: Vec<&PathBuf> = prev
        .keys()
        .filter(|dir| !graph.contains_key(*dir))
        .filter(|dir| {
            let confined = dir.starts_with(confine_root)
                && dir
                    .components()
                    .all(|part| !matches!(part, std::path::Component::ParentDir));
            if !confined {
                tracing::warn!(
                    ?dir,
                    ?confine_root,
                    "refusing to remove an orphan directory outside the install root",
                );
            }
            confined
        })
        .collect();
    orphan_dirs
        .par_iter()
        .for_each(|dir| {
            if let Some(modules_dir) = containing_modules_dir(dir)
                && let Err(error) = remove_dep_bins(modules_dir, dir)
            {
                tracing::warn!(?dir, %error, "failed to remove the bins of an orphan package");
            }
            let _ = try_remove_dir(dir);
        });
    orphan_dirs.len() as u64
}

fn containing_modules_dir(pkg_dir: &Path) -> Option<&Path> {
    let parent = pkg_dir.parent()?;
    let is_scope = parent
        .file_name()
        .is_some_and(|name| name.as_encoded_bytes().starts_with(b"@"));
    if is_scope { parent.parent() } else { Some(parent) }
}

/// Single-directory rimraf with error-swallowing semantics.
/// `NotFound` is a no-op (someone else already removed it);
/// everything else (`PermissionDenied`, `Other`) is silently
/// dropped — a stale directory is less bad than a panicked install.
fn try_remove_dir(dir: &Path) -> io::Result<()> {
    match fs::remove_dir_all(dir) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(_) => Ok(()),
    }
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
    parent_dir: &Path,
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
            let below = link_all_pkgs_in_order::<Reporter>(sub_hierarchy, dir, false, opts)?;
            Ok(LinkedLevel { imported: here, held_back_bins_dirs: Vec::new() }.merge(below))
        })
        .collect::<Result<Vec<LinkedLevel>, LinkHoistedModulesError>>()?
        .into_iter()
        .fold(LinkedLevel::default(), LinkedLevel::merge);

    let held_back = link_hierarchy_bins(hierarchy, parent_dir, opts)?;
    // A project's `.bin` is linked again after the builds anyway.
    if !is_project_root {
        linked.held_back_bins_dirs.extend(held_back);
    }
    linked.held_back_bins_dirs.extend(link_bundled_bins(hierarchy, opts)?);

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
    held_back_bins_dirs: Vec<HeldBackBinsDir>,
}

impl LinkedLevel {
    fn merge(mut self, other: LinkedLevel) -> LinkedLevel {
        self.imported += other.imported;
        self.held_back_bins_dirs.extend(other.held_back_bins_dirs);
        self
    }
}

// Bundled packages are absent from the graph, so their bins need a separate filesystem pass.
///
/// Every `.bin` of the hoisted tree is linked again after the builds, so it
/// holds back the bins that a build may still create, and returns the directory
/// when it did.
fn link_hierarchy_bins(
    hierarchy: &DepHierarchy,
    parent_dir: &Path,
    opts: &LinkHoistedModulesOpts<'_>,
) -> Result<Option<HeldBackBinsDir>, LinkHoistedModulesError> {
    let modules_dir = parent_dir.join("node_modules");
    let dep_names: Vec<String> = hierarchy.0
        .keys()
        .filter_map(|child_dir| opts.graph.get(child_dir))
        .filter_map(|node| node.alias.clone())
        .collect();
    let held_back = !dep_names.is_empty()
        && crate::link_direct_dep_bins_before_builds(&modules_dir, &dep_names, opts.link_options)
            .map_err(LinkHoistedModulesError::LinkBins)?;

    Ok(held_back.then_some(HeldBackBinsDir { modules_dir, dep_names }))
}

/// Packages the tarball ships in its own `node_modules` are not graph nodes,
/// so [`link_hierarchy_bins`] never sees them; their bins are reachable only
/// from inside the bundling package. They are held back like graph packages'
/// bins, and the directories that held one back are returned for the build
/// phase to link again.
fn link_bundled_bins(
    hierarchy: &DepHierarchy,
    opts: &LinkHoistedModulesOpts<'_>,
) -> Result<Vec<HeldBackBinsDir>, LinkHoistedModulesError> {
    let mut held_back_bins_dirs = Vec::new();
    for child_dir in hierarchy.0.keys() {
        let bundles =
            opts.graph.get(child_dir).is_some_and(|node| node.package.has_bundled_dependencies);
        if !bundles {
            continue;
        }
        let modules_dir = child_dir.join("node_modules");
        let packages: Vec<PackageBinSource> = collect_packages_in_modules_dir::<Host>(&modules_dir)
            .map_err(LinkHoistedModulesError::LinkBins)?
            .into_iter()
            .map(|package| package.with_build_pending(true))
            .collect();
        let held_back = link_bins_of_packages_cached::<Host>(
            &packages,
            &modules_dir.join(".bin"),
            opts.link_options,
            &ShimTargetCache::default(),
        )
        .map_err(LinkHoistedModulesError::LinkBins)?;
        if held_back {
            let dep_names = packages
                .iter()
                .filter_map(|package| package.location.strip_prefix(&modules_dir).ok())
                .map(|name| name.to_string_lossy().into_owned())
                .collect();
            held_back_bins_dirs.push(HeldBackBinsDir { modules_dir, dep_names });
        }
    }
    Ok(held_back_bins_dirs)
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
    let Some(cas_paths) = opts.cas_paths_by_pkg_id.get(&node.package.pkg_id_with_patch_hash) else {
        if node.optional {
            return Ok(false);
        }
        return Err(LinkHoistedModulesError::MissingCasPaths {
            pkg_id_with_patch_hash: node.package.pkg_id_with_patch_hash.clone(),
            dir: node.dir.clone(),
        });
    };
    let selected = crate::select_package_files(cas_paths, opts.import.patterns);
    let cas_paths = &*selected;

    if !opts.dir_clone_cache.is_some_and(|cache| {
        cache.try_import::<Reporter>(node, opts.import, cas_paths)
    }) {
        import_indexed_dir::<Reporter>(
            opts.import.logged_methods,
            opts.import.method,
            &node.dir,
            cas_paths,
            hoisted_import_opts(node),
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
            method: crate::optimistic_wire_method(opts.import.method),
            requester: opts.import.requester.to_owned(),
            to: node.dir.to_string_lossy().into_owned(),
        },
    }));

    Ok(true)
}

/// A hoisted package replaces whatever is at its directory but keeps the
/// nested `node_modules` other nodes were hoisted into. A directory
/// dependency keeps its own symlinks.
fn hoisted_import_opts(node: &DependenciesGraphNode) -> ImportIndexedDirOpts {
    ImportIndexedDirOpts {
        force: true,
        keep_modules_dir: true,
        preserve_symlinks: matches!(node.package.resolution, LockfileResolution::Directory(_)),
        ..ImportIndexedDirOpts::default()
    }
}

#[cfg(test)]
mod tests;
