//! Hoisted-linker. Produces the on-disk `node_modules/` tree
//! described by Slice 4's [`crate::LockfileToDepGraphResult`]:
//! removes orphaned directories from the previous install,
//! imports each graph node into its computed directory via
//! [`crate::import_indexed_dir()`], and links bins under every
//! parent's `node_modules/.bin`.
//!
//! Ports upstream's
//! [`installing/deps-restorer/src/linkHoistedModules.ts`](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-restorer/src/linkHoistedModules.ts).
//!
//! Pacquet's linker is synchronous and accepts pre-fetched CAS
//! paths via `cas_paths_by_pkg_id`. Upstream's linker is async
//! and calls `storeController.fetchPackage()` inside the walk;
//! pacquet decouples those layers because pacquet's existing
//! tarball / store-dir / package-fetch machinery is reused
//! verbatim by the install pipeline (Slice 6) before the linker
//! runs. The linker is the final composition step — given a
//! graph and a fully-populated CAS index for every package, it
//! materializes the tree.
//!
//! Concurrency uses [`rayon`]: the hierarchy walk parallelizes
//! at each level (matching upstream's `await Promise.all(...)`
//! per level), and `import_indexed_dir` itself is internally
//! rayon-parallel over CAS entries.

use crate::{
    DepHierarchy, DependenciesGraph, DependenciesGraphNode, ImportIndexedDirError,
    ImportIndexedDirOpts, import_indexed_dir, link_direct_dep_bins,
};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_cmd_shim::LinkBinsError;
use pacquet_config::PackageImportMethod;
use pacquet_lockfile::PkgIdWithPatchHash;
use pacquet_reporter::Reporter;
use rayon::prelude::*;
use std::{
    collections::HashMap,
    fs, io,
    path::{Path, PathBuf},
    sync::atomic::AtomicU8,
};

/// Per-package CAS index. Keyed by [`DependenciesGraphNode::pkg_id_with_patch_hash`],
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
pub type CasPathsByPkgId = HashMap<PkgIdWithPatchHash, HashMap<String, PathBuf>>;

/// Inputs the linker reads from. Borrows everything so callers
/// can keep ownership of the graph / CAS state — the linker
/// doesn't mutate anything but the on-disk tree.
#[derive(Debug)]
pub struct LinkHoistedModulesOpts<'a> {
    pub graph: &'a DependenciesGraph,
    /// Diffed against `graph` to compute orphans. `None` for a
    /// fresh install (no prior lockfile) — no orphans to remove.
    pub prev_graph: Option<&'a DependenciesGraph>,
    /// Per-importer directory hierarchies, keyed by importer
    /// root. Single-importer installs have one entry keyed by
    /// `lockfile_dir`; workspace support will add more.
    pub hierarchy: &'a std::collections::BTreeMap<PathBuf, DepHierarchy>,
    /// Pre-fetched CAS file index per package. The linker
    /// errors with [`LinkHoistedModulesError::MissingCasPaths`]
    /// when a graph node's `pkg_id_with_patch_hash` is missing
    /// from this map and the node is not optional. Optional
    /// nodes are silently skipped, matching upstream's
    /// `if (depNode.optional) return` at
    /// [linkHoistedModules.ts:113](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-restorer/src/linkHoistedModules.ts#L113-L116).
    pub cas_paths_by_pkg_id: &'a CasPathsByPkgId,
    pub import_method: PackageImportMethod,
    /// Install-scoped dedupe state for `pnpm:package-import-method`.
    /// Same value pacquet's isolated path passes; see the
    /// [`crate::import_indexed_dir()`] doc-comment for why it's
    /// install-scoped rather than module-static.
    pub logged_methods: &'a AtomicU8,
    /// Install root, threaded into `pnpm:progress` `imported`'s
    /// `requester`. Same value as the `prefix` in
    /// [`pacquet_reporter::StageLog`].
    pub requester: &'a str,
}

/// Failure modes of [`link_hoisted_modules`]. Marked
/// `#[non_exhaustive]` so adding variants in later sub-slices
/// (e.g. side-effects cache, store-controller integration)
/// isn't a breaking API change.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum LinkHoistedModulesError {
    /// A required (non-optional) graph node had no entry in
    /// `cas_paths_by_pkg_id`. Indicates a bug in the caller
    /// (pre-fetch incomplete) — the linker can't conjure files
    /// it wasn't given.
    #[display("Missing CAS paths for required package {pkg_id_with_patch_hash:?} at {dir:?}")]
    #[diagnostic(code(ERR_PACQUET_LINK_HOISTED_MISSING_CAS))]
    MissingCasPaths { pkg_id_with_patch_hash: PkgIdWithPatchHash, dir: PathBuf },

    /// A hierarchy entry referenced a directory that has no
    /// corresponding entry in `graph`. Slice 4's walker inserts
    /// a graph node every time it inserts a hierarchy entry, so
    /// this shouldn't fire from a real walker result — but
    /// surfacing the inconsistency fails the install fast rather
    /// than producing a partial layout. Upstream effectively
    /// does the same (a missing `graph[dir]` triggers a
    /// `Cannot read properties of undefined` `TypeError` on the
    /// next line), pacquet just spells the error out.
    #[display("Hierarchy references {dir:?} but no matching graph node exists")]
    #[diagnostic(code(ERR_PACQUET_LINK_HOISTED_MISSING_GRAPH_NODE))]
    MissingGraphNode { dir: PathBuf },

    #[diagnostic(transparent)]
    ImportIndexedDir(#[error(source)] ImportIndexedDirError),

    #[diagnostic(transparent)]
    LinkBins(#[error(source)] LinkBinsError),
}

/// Produce the on-disk hoisted tree from a Slice 4 walk result.
///
/// 1. **Orphan removal.** Every directory in `prev_graph` but
///    not in `graph` is silently `rimraf`'d. Removal happens
///    *before* any insert so the linker doesn't race against
///    itself when a directory name is reused for a different
///    package version.
/// 2. **Per-node import.** The hierarchy is walked top-down,
///    parallel at each level. For every node the linker calls
///    [`import_indexed_dir()`] with `force: true,
///    keep_modules_dir: true` (matches upstream's
///    `importPackage(..., { force: true, keepModulesDir: true })`).
/// 3. **Per-`node_modules` bin link.** After a level's children
///    are all done, `<parent>/node_modules/.bin` is populated
///    from the just-imported direct children's `package.json`.
///    Matches upstream's `linkBins(modulesDir, binsDir, ...)`
///    at the bottom of `linkAllPkgsInOrder`.
///
/// Optional nodes whose CAS paths are missing are silently
/// skipped (no error, no import). Required nodes surface as
/// [`LinkHoistedModulesError::MissingCasPaths`].
pub fn link_hoisted_modules<Reporter: self::Reporter>(
    opts: &LinkHoistedModulesOpts<'_>,
) -> Result<(), LinkHoistedModulesError> {
    remove_orphans(opts.graph, opts.prev_graph);

    // Drive each importer's hierarchy in parallel — workspace
    // installs (Slice 9) will have multiple importers; the
    // single-importer case has one and rayon's overhead is
    // negligible.
    opts.hierarchy
        .par_iter()
        .map(|(parent_dir, deps_hierarchy)| {
            link_all_pkgs_in_order::<Reporter>(deps_hierarchy, parent_dir, opts)
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(())
}

/// Phase 1: rimraf every directory that was in the previous
/// install's graph but isn't in the new one. Errors are swallowed
/// silently to match upstream's `tryRemoveDir` `EPERM`/`EBUSY`
/// tolerance — a directory we can't remove right now is no worse
/// than leaving a stale entry, and the next install will retry.
fn remove_orphans(graph: &DependenciesGraph, prev_graph: Option<&DependenciesGraph>) {
    let Some(prev) = prev_graph else { return };
    let orphan_dirs: Vec<&PathBuf> = prev.keys().filter(|dir| !graph.contains_key(*dir)).collect();
    orphan_dirs.par_iter().for_each(|dir| {
        let _ = try_remove_dir(dir);
    });
}

/// Single-directory rimraf with the same error-swallowing
/// semantics upstream's
/// [`tryRemoveDir`](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-restorer/src/linkHoistedModules.ts#L70-L86)
/// uses. `NotFound` is a no-op (someone else already removed it);
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
/// each `<parent>/node_modules/.bin`. Mirrors upstream's
/// [`linkAllPkgsInOrder`](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-restorer/src/linkHoistedModules.ts#L88-L153).
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
    opts: &LinkHoistedModulesOpts<'_>,
) -> Result<(), LinkHoistedModulesError> {
    // Phase 2: import this level's packages + recurse into each
    // one's children. `par_iter` is sufficient — the side effects
    // are on disk and target disjoint directories.
    hierarchy
        .0
        .par_iter()
        .map(|(dir, sub_hierarchy)| {
            let node = opts
                .graph
                .get(dir)
                .ok_or_else(|| LinkHoistedModulesError::MissingGraphNode { dir: dir.clone() })?;
            import_node::<Reporter>(node, opts)?;
            link_all_pkgs_in_order::<Reporter>(sub_hierarchy, dir, opts)
        })
        .collect::<Result<Vec<_>, _>>()?;

    // Phase 3: link bins of every immediate child under
    // `parent_dir/node_modules`. The keys of `hierarchy.0` are
    // absolute child directories; bin linking needs the alias
    // names, which come from each child's graph-node `alias`
    // (matches the directory's basename for hoisted layouts).
    let modules_dir = parent_dir.join("node_modules");
    let dep_names: Vec<String> = hierarchy
        .0
        .keys()
        .filter_map(|child_dir| opts.graph.get(child_dir))
        .filter_map(|node| node.alias.clone())
        .collect();
    if !dep_names.is_empty() {
        link_direct_dep_bins(&modules_dir, &dep_names)
            .map_err(LinkHoistedModulesError::LinkBins)?;
    }

    Ok(())
}

/// Import one graph node into its target `dir`. Looks up the
/// node's CAS paths by `pkg_id_with_patch_hash`; if missing and
/// the node is optional, silently returns (matches upstream's
/// `if (depNode.optional) return` on fetch failure). Otherwise
/// calls [`import_indexed_dir()`] with `force: true,
/// keep_modules_dir: true` — the hoisted-linker call shape.
fn import_node<Reporter: self::Reporter>(
    node: &DependenciesGraphNode,
    opts: &LinkHoistedModulesOpts<'_>,
) -> Result<(), LinkHoistedModulesError> {
    let Some(cas_paths) = opts.cas_paths_by_pkg_id.get(&node.pkg_id_with_patch_hash) else {
        if node.optional {
            return Ok(());
        }
        return Err(LinkHoistedModulesError::MissingCasPaths {
            pkg_id_with_patch_hash: node.pkg_id_with_patch_hash.clone(),
            dir: node.dir.clone(),
        });
    };

    import_indexed_dir::<Reporter>(
        opts.logged_methods,
        opts.import_method,
        &node.dir,
        cas_paths,
        ImportIndexedDirOpts { force: true, keep_modules_dir: true },
    )
    .map_err(LinkHoistedModulesError::ImportIndexedDir)
}

#[cfg(test)]
mod tests;
