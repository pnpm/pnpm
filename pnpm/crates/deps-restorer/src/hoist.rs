//! Hoisting decides which transitive dependencies also surface outside
//! their isolated virtual-store locations. The result is persisted to
//! `.modules.yaml` and drives symlink and bin creation.
//!
//! Bin linking for hoisted aliases is handled at the call site
//! ([`crate::InstallFrozenLockfile::run`]) by re-using
//! [`crate::link_direct_dep_bins`] against the private and public
//! hoisted modules dirs — the hoist pass itself only computes the
//! alias-list inputs that pass needs.

use indexmap::IndexMap;
use pnpm_config::matcher::Matcher;
use pnpm_lockfile::{PackageKey, PackageMetadata, PkgName, ProjectSnapshot, SnapshotEntry};
use pnpm_modules_yaml::HoistKind;
use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    path::PathBuf,
};

/// On-disk shape persisted as `hoistedDependencies` in `.modules.yaml`.
/// Insertion order is part of pnpm's output contract.
pub type HoistedDependencies = IndexMap<String, IndexMap<String, HoistKind>>;

/// Per-snapshot graph view used by the hoist traversal. Built from
/// `lockfile.snapshots:` + `lockfile.packages:` via
/// [`build_hoist_graph`].
#[derive(Debug, Clone)]
pub struct HoistGraphNode {
    /// Package name as it appears on the lockfile key (= the
    /// `<name>` segment of `<virtual_store>/<key.virtual_store_name>/node_modules/<name>`).
    pub name: PkgName,
    /// Children indexed by alias (the name they're linked under in the
    /// parent's `node_modules`). For npm-alias entries the alias and
    /// the resolved package name diverge — the hoist pass keeps the
    /// alias because that's what becomes the directory name in the
    /// hoisted location too.
    pub children: IndexMap<String, PackageKey>,
    /// Virtual-store directory name used by pnpm to order equal-depth nodes.
    pub sort_key: String,
    /// Whether the package declares a bin. `false` when the lockfile's
    /// `packages:` metadata doesn't carry the field (treat as "no bin"
    /// rather than guessing).
    pub has_bin: bool,
}

/// Build the hoist graph from a v9 lockfile's `snapshots:` + `packages:`.
///
/// Skips snapshots whose metadata key isn't in `packages` — same
/// degraded behaviour as [`crate::deps_graph::build_deps_graph`]; the
/// hoist pass simply won't see the missing snapshot.
///
#[must_use]
pub fn build_hoist_graph(
    snapshots: &HashMap<PackageKey, SnapshotEntry>,
    packages: &HashMap<PackageKey, PackageMetadata>,
) -> HashMap<PackageKey, HoistGraphNode> {
    build_hoist_graph_with_max_length(
        snapshots,
        packages,
        pnpm_modules_yaml::DEFAULT_VIRTUAL_STORE_DIR_MAX_LENGTH as usize,
    )
}

#[must_use]
pub fn build_hoist_graph_with_max_length(
    snapshots: &HashMap<PackageKey, SnapshotEntry>,
    packages: &HashMap<PackageKey, PackageMetadata>,
    virtual_store_dir_max_length: usize,
) -> HashMap<PackageKey, HoistGraphNode> {
    use rayon::prelude::*;
    snapshots
        .par_iter()
        .filter_map(|(key, snapshot)| {
            let metadata_key = key.without_peer();
            let metadata = packages.get(&metadata_key)?;
            let mut children = IndexMap::new();
            for dependency_map in [&snapshot.dependencies, &snapshot.optional_dependencies] {
                let mut dep_entries: Vec<_> =
                    dependency_map.iter().flat_map(|map| map.iter()).collect();
                dep_entries.sort_by_cached_key(|entry| entry.0.to_string());
                for (alias, dep_ref) in dep_entries {
                    // `dep_ref.resolve` is `None` for `link:` deps —
                    // workspace siblings that live outside the virtual
                    // store, which are skipped here.
                    if let Some(child) = dep_ref.resolve(alias) {
                        children.insert(alias.to_string(), child);
                    }
                }
            }
            Some((
                key.clone(),
                HoistGraphNode {
                    name: key.name.clone(),
                    children,
                    sort_key: key.to_virtual_store_name(virtual_store_dir_max_length),
                    has_bin: metadata.has_bin == Some(true),
                },
            ))
        })
        .collect()
}

/// Per-importer direct-dependency map.
///
/// Outer key is the importer id (`"."` for the root project; workspace
/// projects extend this in [#431]). Inner map is alias → snapshot key,
/// preserving npm-alias semantics — the alias is the directory name
/// linked under the project's `node_modules`, and the snapshot key
/// resolves where the link points.
///
/// [#431]: https://github.com/pnpm/pacquet/issues/431
pub type DirectDepsByImporter = IndexMap<String, IndexMap<String, PackageKey>>;

/// Build a [`DirectDepsByImporter`] from the lockfile's `importers:`
/// section, restricted to the supplied dependency groups.
///
/// Peer-only entries don't belong in the direct-deps map because peers
/// materialize through their host.
///
/// Accepts an iterator over `(importer_id, &ProjectSnapshot)` pairs
/// rather than the lockfile's full `&HashMap` so the caller can
/// restrict the input to the importer set actually being installed.
/// Today the frozen-lockfile call site passes the full `importers`
/// map — workspace install (pnpm/pacquet#431) landed in [#443] and
/// pacquet now installs every entry — so the iterator-shaped
/// signature lets future selected-projects (`--filter`) installs
/// pass a filtered iterator without touching this function. The
/// `link:` workspace-sibling entries are skipped via
/// [`pnpm_lockfile::ImporterDepVersion::as_regular`] inside the
/// loop.
///
/// [#443]: https://github.com/pnpm/pacquet/pull/443
pub fn build_direct_deps_by_importer<'a, Iter>(
    importers: Iter,
    dependency_groups: impl IntoIterator<Item = pnpm_package_manifest::DependencyGroup>,
) -> DirectDepsByImporter
where
    Iter: IntoIterator<Item = (&'a String, &'a ProjectSnapshot)>,
{
    use pnpm_package_manifest::DependencyGroup;

    let mut result: DirectDepsByImporter = IndexMap::new();
    let mut importers: Vec<_> = importers.into_iter().collect();
    importers.sort_by(|a, b| a.0.cmp(b.0));
    let dependency_groups: Vec<_> = dependency_groups.into_iter().collect();
    for (importer_id, project_snapshot) in importers {
        // Package identity follows the direct-linker's caller precedence.
        let mut resolved_by_alias = HashMap::new();
        for group in &dependency_groups {
            if matches!(group, DependencyGroup::Peer) {
                continue;
            }
            let Some(map) = project_snapshot.get_map_by_group(*group) else { continue };
            for (name, spec) in map {
                let Some(key) = spec.version.resolved_key(name) else { continue };
                resolved_by_alias.entry(name.to_string()).or_insert(key);
            }
        }

        // Key positions follow pnpm's manifest merge order.
        let mut deps: IndexMap<String, PackageKey> = IndexMap::new();
        for group in [DependencyGroup::Dev, DependencyGroup::Prod, DependencyGroup::Optional] {
            if !dependency_groups.contains(&group) {
                continue;
            }
            let Some(map) = project_snapshot.get_map_by_group(group) else { continue };
            let mut entries: Vec<_> = map.iter().collect();
            entries.sort_by_cached_key(|entry| entry.0.to_string());
            for (name, _) in entries {
                let alias = name.to_string();
                let Some(key) = resolved_by_alias.get(&alias) else { continue };
                deps.entry(alias).or_insert_with(|| key.clone());
            }
        }
        result.insert(importer_id.clone(), deps);
    }
    result
}

/// Inputs to [`get_hoisted_dependencies`].
pub struct HoistInputs<'a> {
    pub graph: &'a HashMap<PackageKey, HoistGraphNode>,
    pub direct_deps_by_importer: &'a DirectDepsByImporter,
    /// Snapshot keys that should not be hoisted because they were
    /// skipped (typically: skipped optional deps). The hoist traversal still
    /// walks into them so the children of a skipped optional dep can
    /// be considered for hoisting.
    pub skipped: &'a HashSet<PackageKey>,
    /// Boolean matcher built from `Config.hoist_pattern`.
    pub private_pattern: Matcher,
    /// Boolean matcher built from `Config.public_hoist_pattern`.
    pub public_pattern: Matcher,
    /// `hoist-workspace-packages`: workspace project name → absolute
    /// project dir, for every named non-root project. When present,
    /// each name is considered for hoisting like a root-level alias
    /// (v11 merges them into the root importer's children with
    /// direct deps taking precedence) and, when a pattern matches,
    /// the hoisted-modules entry symlinks straight to the project
    /// dir. `None` when the config knob is off.
    pub hoisted_workspace_packages: Option<&'a IndexMap<String, PathBuf>>,
}

/// Output of [`get_hoisted_dependencies`].
pub struct HoistResult {
    /// `.modules.yaml`'s `hoistedDependencies` shape — keyed by
    /// snapshot key, value is alias → kind.
    pub hoisted_dependencies: HoistedDependencies,
    /// Symlink-pass input: which aliases (and what kind) are mapped
    /// to which source nodes. Map order doesn't matter; symlinks are
    /// fan-out per (node, alias).
    pub hoisted_dependencies_by_node_id: HashMap<PackageKey, HashMap<String, HoistKind>>,
    /// Aliases whose target package declares a bin and were hoisted
    /// privately, paired with the snapshot key the alias resolves to
    /// (so the bin pass can derive the slot directory without a
    /// `realpath`). The install pipeline feeds this into
    /// `link_direct_dep_bins_resolved` against the private hoisted
    /// modules dir to write shims into `<vs>/node_modules/.bin`.
    pub hoisted_aliases_with_bins: Vec<(String, PackageKey)>,
    /// Aliases whose target package declares a bin and were hoisted
    /// publicly. Public-hoist bins land alongside the project's
    /// direct-dep bins in `<root>/node_modules/.bin` — the bins of the
    /// publicly hoisted modules are linked together with the bins of
    /// the project's direct dependencies.
    /// In pacquet's pipeline ordering, `SymlinkDirectDependencies`
    /// runs *before* `hoist`, so the install pipeline does an
    /// additional `link_direct_dep_bins` pass over this list after
    /// the hoist symlinks land.
    pub publicly_hoisted_aliases_with_bins: Vec<String>,
    /// `hoist-workspace-packages` placements: (alias, kind, absolute
    /// project dir) for every workspace project name a hoist pattern
    /// matched. Symlinked by [`symlink_hoisted_dependencies`] straight
    /// to the project dir. Deliberately NOT part of
    /// [`Self::hoisted_dependencies`] — v11 leaves workspace packages
    /// out of `.modules.yaml`'s `hoistedDependencies` too (its graph
    /// lookup misses for a `ProjectId` before the record is written).
    pub hoisted_workspace_aliases: Vec<(String, HoistKind, PathBuf)>,
}

/// Walk the dependency graph in pnpm's graph-walker order and decide
/// which aliases should be hoisted.
///
/// Returns `None` when the graph is empty.
#[must_use]
pub fn get_hoisted_dependencies<'a>(input: &'a HoistInputs<'a>) -> Option<HoistResult> {
    if input.graph.is_empty() {
        return None;
    }

    let mut visited: HashSet<&'a PackageKey> = HashSet::new();
    let mut entries: Vec<BfsEntry<'a>> = Vec::new();

    let mut direct_deps = IndexMap::new();
    for importer_deps in input.direct_deps_by_importer.values() {
        for (alias, node_id) in importer_deps {
            direct_deps.entry(alias.clone()).or_insert_with(|| node_id.clone());
        }
    }
    if let Some(workspace_packages) = input.hoisted_workspace_packages {
        let mut ordered_direct_deps = IndexMap::new();
        for name in workspace_packages.keys() {
            if let Some(node_id) = direct_deps.get(name) {
                ordered_direct_deps.insert(name.clone(), node_id.clone());
            }
        }
        for (alias, node_id) in direct_deps {
            ordered_direct_deps.entry(alias).or_insert(node_id);
        }
        direct_deps = ordered_direct_deps;
    }

    let mut direct_nodes = Vec::new();
    for importer_deps in input.direct_deps_by_importer.values() {
        for node_id in importer_deps.values() {
            let Some((graph_key, _)) = input.graph.get_key_value(node_id) else { continue };
            if visited.insert(graph_key) {
                direct_nodes.push(graph_key);
            }
        }
    }
    entries.push(BfsEntry {
        depth: -1,
        sort_key: String::new(),
        children: Cow::Owned(direct_deps),
    });
    append_dependency_entries(direct_nodes, 0, input.graph, &mut visited, &mut entries);

    // pnpm sorts graph-walker results by depth and virtual-store path.
    entries.sort_by(|a, b| a.depth.cmp(&b.depth).then_with(|| a.sort_key.cmp(&b.sort_key)));

    // Seed `hoisted_aliases` with every direct-dep name of the root
    // importer (`"."`). Workspace importers' deps don't seed this set
    // because they live in their own `node_modules` and don't
    // collide with the root.
    let mut hoisted_aliases: HashSet<String> = input
        .direct_deps_by_importer
        .get(".")
        .map(|map| map.keys().map(|k| k.to_lowercase()).collect())
        .unwrap_or_default();

    let mut hoisted_dependencies = HoistedDependencies::new();
    let mut hoisted_dependencies_by_node_id: HashMap<PackageKey, HashMap<String, HoistKind>> =
        HashMap::new();
    let mut hoisted_aliases_with_bins: Vec<(String, PackageKey)> = Vec::new();
    let mut publicly_hoisted_aliases_with_bins: Vec<String> = Vec::new();
    // Dedup the bin-alias vecs — pacquet emits
    // `Vec`s to keep the consumer signature simple but de-dups via
    // these sets first. Separate sets for private vs public so an
    // alias hoisted both privately (impossible — public always wins)
    // doesn't collide; in practice each alias lands in exactly one
    // kind.
    let mut private_bins_seen: HashSet<String> = HashSet::new();
    let mut public_bins_seen: HashSet<String> = HashSet::new();

    // `hoist-workspace-packages`: consider each named workspace
    // project for hoisting after every importer's direct deps (v11
    // merges the names into the root children as the LOWEST-precedence
    // entries — any direct dep wins the alias) but before depth-0
    // transitives. One deliberate divergence from v11: a placed
    // workspace name claims its alias, so an equally-named transitive
    // can't also hoist and clobber the link nondeterministically
    // (v11's graph-miss `continue` skips the claim by accident).
    let mut hoisted_workspace_aliases: Vec<(String, HoistKind, PathBuf)> = Vec::new();
    let hoist_workspace_packages =
        |hoisted_aliases: &mut HashSet<String>, out: &mut Vec<(String, HoistKind, PathBuf)>| {
            for (name, dir) in input.hoisted_workspace_packages.into_iter().flatten() {
                let hoist_kind = if input.public_pattern.matches(name) {
                    HoistKind::Public
                } else if input.private_pattern.matches(name) {
                    HoistKind::Private
                } else {
                    continue;
                };
                if !hoisted_aliases.insert(name.to_lowercase()) {
                    continue;
                }
                out.push((name.clone(), hoist_kind, dir.clone()));
            }
        };

    let mut workspace_packages_done = false;
    for entry in &entries {
        if !workspace_packages_done && entry.depth >= 0 {
            hoist_workspace_packages(&mut hoisted_aliases, &mut hoisted_workspace_aliases);
            workspace_packages_done = true;
        }
        for (alias, child_node_id) in entry.children.iter() {
            let hoist_kind = if input.public_pattern.matches(alias) {
                HoistKind::Public
            } else if input.private_pattern.matches(alias) {
                HoistKind::Private
            } else {
                continue;
            };
            let alias_norm = alias.to_lowercase();
            if hoisted_aliases.contains(&alias_norm) {
                continue;
            }
            // Record (childNodeId, alias) → kind unconditionally; the
            // symlink pass tolerates missing nodes via its own guard.
            hoisted_dependencies_by_node_id
                .entry(child_node_id.clone())
                .or_default()
                .insert(alias.clone(), hoist_kind);
            // From here on we need the node — bail if missing or
            // skipped. Note we do NOT add the alias to
            // `hoisted_aliases` in that case, so a later sibling
            // with the same alias still gets a chance.
            let Some(node) = input.graph.get(child_node_id) else { continue };
            if input.skipped.contains(child_node_id) {
                continue;
            }
            if node.has_bin {
                match hoist_kind {
                    HoistKind::Private => {
                        if private_bins_seen.insert(alias.clone()) {
                            hoisted_aliases_with_bins.push((alias.clone(), child_node_id.clone()));
                        }
                    }
                    HoistKind::Public => {
                        if public_bins_seen.insert(alias.clone()) {
                            publicly_hoisted_aliases_with_bins.push(alias.clone());
                        }
                    }
                }
            }
            hoisted_aliases.insert(alias_norm);
            hoisted_dependencies
                .entry(child_node_id.to_string())
                .or_default()
                .insert(alias.clone(), hoist_kind);
        }
    }

    // A graph whose entries are all depth −1 (no transitives) never
    // crossed the depth boundary above.
    if !workspace_packages_done {
        hoist_workspace_packages(&mut hoisted_aliases, &mut hoisted_workspace_aliases);
    }

    Some(HoistResult {
        hoisted_dependencies,
        hoisted_dependencies_by_node_id,
        hoisted_aliases_with_bins,
        publicly_hoisted_aliases_with_bins,
        hoisted_workspace_aliases,
    })
}

struct BfsEntry<'a> {
    depth: i32,
    sort_key: String,
    children: Cow<'a, IndexMap<String, PackageKey>>,
}

fn append_dependency_entries<'a>(
    nodes: Vec<&'a PackageKey>,
    depth: i32,
    graph: &'a HashMap<PackageKey, HoistGraphNode>,
    visited: &mut HashSet<&'a PackageKey>,
    entries: &mut Vec<BfsEntry<'a>>,
) {
    let mut steps = vec![(nodes, depth)];
    while let Some((nodes, depth)) = steps.pop() {
        for node_id in &nodes {
            entries.push(BfsEntry {
                depth,
                sort_key: graph[*node_id].sort_key.clone(),
                children: Cow::Borrowed(&graph[*node_id].children),
            });
        }
        let next_steps: Vec<Vec<&PackageKey>> = nodes
            .iter()
            .map(|node_id| {
                graph[*node_id]
                    .children
                    .values()
                    .filter_map(|child_id| {
                        let (graph_key, _) = graph.get_key_value(child_id)?;
                        visited.insert(graph_key).then_some(graph_key)
                    })
                    .collect()
            })
            .collect();
        for next_nodes in next_steps.into_iter().rev() {
            if !next_nodes.is_empty() {
                steps.push((next_nodes, depth + 1));
            }
        }
    }
}

/// Create the hoist symlinks.
///
/// For each (`snapshot_key`, alias, kind) entry, link
/// `<target_dir>/<alias>` → `<layout.slot_dir(key)>/node_modules/<package_name>`,
/// where `<target_dir>` is `<public_hoisted_modules_dir>` for public-kind
/// or `<private_hoisted_modules_dir>` for private-kind. The
/// [`crate::VirtualStoreLayout`] handle resolves the slot directory in
/// either GVS mode (`<store_dir>/links/<scope>/<name>/<version>/<hash>/`)
/// or legacy flat-name mode
/// (`<virtual_store_dir>/<key.virtual_store_name>/`); the hoist code
/// never has to branch on `enable_global_virtual_store` itself.
///
/// Existing symlinks are introspected — if the existing entry is a
/// symlink pointing at a target inside the virtual store
/// (`layout.package_store_dir()` — the GVS links dir or the local
/// `.pnpm` dir) or inside the internal pnpm directory (the parent of
/// `private_hoisted_modules_dir`), the stale symlink is replaced.
/// External symlinks (or non-symlink occupants) are left in place.
///
/// Two-phase to amortize directory creation:
///
/// 1. Walk the input once to collect every `(target, dest)` symlink
///    pair plus the set of scope-dir parents (`<root>/@scope`)
///    needed by scoped aliases.
/// 2. `create_dir_all` the two hoisted-modules roots and each
///    distinct scope dir — once per dir, not per symlink, so a
///    1k-alias install doesn't pay 1k redundant stats on the same
///    handful of parents.
/// 3. `par_iter` the pair list and issue `symlinkat()` syscalls in
///    parallel via rayon. Each pair is now a single syscall — no
///    parent-dir prep — so the only contention is the kernel's
///    inode lock on the parent directory, which is dominated by
///    the syscall latency itself on macOS APFS / Linux ext4.
pub fn symlink_hoisted_dependencies(
    hoisted_by_node_id: &HashMap<PackageKey, HashMap<String, HoistKind>>,
    hoisted_workspace_aliases: &[(String, HoistKind, PathBuf)],
    graph: &HashMap<PackageKey, HoistGraphNode>,
    layout: &crate::VirtualStoreLayout,
    private_hoisted_modules_dir: &std::path::Path,
    public_hoisted_modules_dir: &std::path::Path,
    skipped: &std::collections::HashSet<PackageKey>,
) -> Result<(), crate::SymlinkPackageError> {
    use crate::safe_join_modules_dir::safe_join_modules_dir;
    use rayon::prelude::*;
    use std::{collections::HashSet, io::ErrorKind, path::Path, sync::Arc};

    // Phase 1: collect symlink work as `(Arc<dep_dir>, kind, alias)`
    // tuples. Sharing `dep_dir` via `Arc` avoids cloning the PathBuf
    // (which under legacy flat-name mode wraps the
    // `to_virtual_store_name()` String the lockfile crate flags as
    // "far from optimal") once per alias on a multi-alias node. Most
    // nodes have a single hoisted alias, so the Arc overhead is
    // marginal — but the `slot_dir` lookup itself does work
    // (HashMap probe + String build) so building it just once per
    // node is worth the indirection.
    //
    // The scope-dir set collected here is small (one entry per
    // distinct `@scope/` aliased to the hoist target) and is created
    // serially in phase 2 before parallel symlink syscalls fire.
    let mut work: Vec<(Arc<PathBuf>, HoistKind, &String)> = Vec::new();
    let mut scope_dirs: HashSet<PathBuf> = HashSet::new();
    for (node_id, alias_map) in hoisted_by_node_id {
        // Skipped snapshots never get a virtual-store slot, so a
        // hoist symlink at their slot path would dangle (Unix) or
        // fail as a junction (Windows). `hoisted_dependencies_by_node_id`
        // records the (target, alias) pair unconditionally, so the
        // filter has to run here too.
        if skipped.contains(node_id) {
            continue;
        }
        let Some(node) = graph.get(node_id) else { continue };
        // `node.name` originates from the lockfile, so a traversal-shaped
        // name is guarded here before it becomes the hoist symlink's
        // `<slot>/node_modules/<name>` target.
        let dep_dir = Arc::new(
            safe_join_modules_dir(
                &layout.slot_dir(node_id).join("node_modules"),
                &node.name.to_string(),
            )
            .map_err(crate::SymlinkPackageError::InvalidAlias)?,
        );
        for (alias, kind) in alias_map {
            let target_dir_root: &Path = match kind {
                HoistKind::Public => public_hoisted_modules_dir,
                HoistKind::Private => private_hoisted_modules_dir,
            };
            // Scoped alias (`@scope/name`) → dest parent is
            // `<root>/@scope`, which doesn't exist yet on a fresh
            // install. Unscoped alias → dest parent is `<root>`,
            // which gets created unconditionally below. Compute the
            // parent without materialising the full dest path (saves
            // one PathBuf alloc when not scoped).
            if alias.starts_with('@')
                && let Some(slash) = alias.find('/')
            {
                scope_dirs.insert(target_dir_root.join(&alias[..slash]));
            }
            work.push((Arc::clone(&dep_dir), *kind, alias));
        }
    }

    // `hoist-workspace-packages` placements: same (target, kind,
    // alias) shape, with the target being the workspace project dir
    // itself instead of a virtual-store slot. The alias is a
    // package-manifest `name`, so the scope-dir prep below applies
    // to these too.
    for (alias, kind, project_dir) in hoisted_workspace_aliases {
        if alias.starts_with('@')
            && let Some(slash) = alias.find('/')
        {
            let target_dir_root: &Path = match kind {
                HoistKind::Public => public_hoisted_modules_dir,
                HoistKind::Private => private_hoisted_modules_dir,
            };
            scope_dirs.insert(target_dir_root.join(&alias[..slash]));
        }
        work.push((Arc::new(project_dir.clone()), *kind, alias));
    }

    if work.is_empty() {
        return Ok(());
    }

    // Phase 2: pre-create dirs serially (cheap, dedupe'd, and each
    // is a no-op for already-existing dirs).
    let mkdir = |path: &Path| -> Result<(), crate::SymlinkPackageError> {
        std::fs::create_dir_all(path).map_err(|error| crate::SymlinkPackageError::CreateParentDir {
            dir: path.to_path_buf(),
            error,
        })
    };
    mkdir(private_hoisted_modules_dir)?;
    mkdir(public_hoisted_modules_dir)?;
    for scope in &scope_dirs {
        mkdir(scope)?;
    }

    // Phase 3: fire symlink syscalls in parallel. `try_for_each`
    // short-circuits on first error, propagating it through
    // rayon's collector. `dest` is constructed inside the parallel
    // closure (one `PathBuf::join` allocation per task) so the
    // sequential phase-1 walk doesn't pay for it.
    work.par_iter().try_for_each(
        |(dep_dir, kind, alias)| -> Result<(), crate::SymlinkPackageError> {
            let target_dir_root: &Path = match kind {
                HoistKind::Public => public_hoisted_modules_dir,
                HoistKind::Private => private_hoisted_modules_dir,
            };
            let dest = target_dir_root.join(alias);
            match pnpm_fs::symlink_dir(dep_dir.as_path(), &dest) {
                Ok(()) => Ok(()),
                Err(ref error) if error.kind() == ErrorKind::AlreadyExists => {
                    update_stale_hoist_symlink(
                        dep_dir.as_path(),
                        &dest,
                        layout.package_store_dir(),
                        private_hoisted_modules_dir.parent().expect(
                            "private_hoisted_modules_dir (<vs>/node_modules) always has a parent",
                        ),
                    )
                }
                Err(error) => Err(crate::SymlinkPackageError::SymlinkDir {
                    symlink_target: dep_dir.as_path().to_path_buf(),
                    symlink_path: dest,
                    error,
                }),
            }
        },
    )
}

/// Read the existing symlink at `dest` and decide whether it should
/// be replaced. If it already points at `dep_dir`, leave it untouched.
/// If it points inside `package_store_dir` or `internal_pnpm_dir`
/// (a pnpm-internal symlink — e.g., a stale link from a prior non-GVS
/// install), remove it and create a new symlink to `dep_dir`. External
/// symlinks (and non-symlink occupants) are left in place.
///
/// The already-correct fast path skips the unlink + recreate churn (and
/// the transient missing-link window it opens) on warm reinstalls, the
/// same way [`pnpm_fs::force_symlink_dir`] does — see its
/// `existing_symlink_up_to_date` helper.
fn update_stale_hoist_symlink(
    dep_dir: &std::path::Path,
    dest: &std::path::Path,
    package_store_dir: &std::path::Path,
    internal_pnpm_dir: &std::path::Path,
) -> Result<(), crate::SymlinkPackageError> {
    let Ok(existing_raw) = pnpm_fs::read_symlink_dir(dest) else {
        return Ok(());
    };
    let existing = if existing_raw.is_relative() {
        dest.parent().unwrap_or_else(|| std::path::Path::new("")).join(&existing_raw)
    } else {
        existing_raw
    };
    if pnpm_fs::lexical_normalize(&existing) == pnpm_fs::lexical_normalize(dep_dir) {
        return Ok(());
    }
    if !pnpm_fs::is_subdir(package_store_dir, &existing)
        && !pnpm_fs::is_subdir(internal_pnpm_dir, &existing)
    {
        return Ok(());
    }
    pnpm_fs::remove_symlink_dir(dest).map_err(|error| crate::SymlinkPackageError::SymlinkDir {
        symlink_target: dep_dir.to_path_buf(),
        symlink_path: dest.to_path_buf(),
        error,
    })?;
    pnpm_fs::symlink_dir(dep_dir, dest).map_err(|error| crate::SymlinkPackageError::SymlinkDir {
        symlink_target: dep_dir.to_path_buf(),
        symlink_path: dest.to_path_buf(),
        error,
    })
}

#[cfg(test)]
mod tests;
