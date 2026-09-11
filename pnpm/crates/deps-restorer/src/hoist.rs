//! Hoisting decides which transitive dependencies also surface outside
//! their isolated virtual-store locations. The result is persisted to
//! `.modules.yaml` and drives symlink and bin creation.
//!
//! Bin linking for hoisted aliases is handled at the call site
//! ([`crate::InstallFrozenLockfile::run`]) by re-using
//! [`crate::link_direct_dep_bins`] against the private and public
//! hoisted modules dirs — the hoist pass itself only computes the
//! alias-list inputs that pass needs.

pub use symlinks::symlink_hoisted_dependencies;

mod symlinks;

use indexmap::IndexMap;
use pnpm_lockfile::{PackageKey, PackageMetadata, PkgName, ProjectSnapshot, SnapshotEntry};
use pnpm_matcher::Matcher;
use pnpm_modules_yaml::HoistKind;
use std::{
    borrow::Cow,
    collections::{BTreeMap, HashMap, HashSet},
    path::PathBuf,
};

/// On-disk shape persisted as `hoistedDependencies` in `.modules.yaml`.
/// Insertion order is part of pnpm's output contract.
pub type HoistedDependencies = IndexMap<String, IndexMap<String, HoistKind>>;

/// On-disk shape persisted as `hoistedLocations` in `.modules.yaml`:
/// depPath to the lockfile-relative directories the hoisted linker
/// placed that package at. Read back by the next install so a
/// directory that is already there is not imported again.
pub type HoistedLocations = BTreeMap<String, Vec<String>>;

/// The keys the previous install's `.modules.yaml` recorded as not built:
/// its `ignoredBuilds` (a pkgId, patch hash included) and its
/// `pendingBuilds` (a dep path). A hoisted package matching one of them is
/// handed to the build phase even when it is already in place, so the
/// build policy is applied to it again (`strictDepBuilds`, a newly allowed
/// build) exactly as on an install that imports it.
pub type UnbuiltBuilds = HashSet<String>;

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
            let children = snapshot_children(snapshot);
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
    let mut result: DirectDepsByImporter = IndexMap::new();
    let mut importers: Vec<_> = importers.into_iter().collect();
    importers.sort_by(|a, b| a.0.cmp(b.0));
    let dependency_groups: Vec<_> = dependency_groups.into_iter().collect();
    for (importer_id, project_snapshot) in importers {
        let resolved_by_alias = resolved_keys_by_alias(project_snapshot, &dependency_groups);
        let deps = ordered_direct_deps(project_snapshot, &dependency_groups, &resolved_by_alias);
        result.insert(importer_id.clone(), deps);
    }
    result
}

/// Package identity follows the direct-linker's caller precedence: the
/// first group that resolves an alias owns it.
fn resolved_keys_by_alias(
    project_snapshot: &ProjectSnapshot,
    dependency_groups: &[pnpm_package_manifest::DependencyGroup],
) -> HashMap<String, PackageKey> {
    let mut resolved_by_alias = HashMap::new();
    for group in dependency_groups
        .iter()
        .filter(|group| !matches!(group, pnpm_package_manifest::DependencyGroup::Peer))
    {
        let Some(map) = project_snapshot.get_map_by_group(*group) else { continue };
        for (name, spec) in map {
            let Some(key) = spec.version.resolved_key(name) else { continue };
            resolved_by_alias.entry(name.to_string()).or_insert(key);
        }
    }
    resolved_by_alias
}

/// Key positions follow pnpm's manifest merge order.
fn ordered_direct_deps(
    project_snapshot: &ProjectSnapshot,
    dependency_groups: &[pnpm_package_manifest::DependencyGroup],
    resolved_by_alias: &HashMap<String, PackageKey>,
) -> IndexMap<String, PackageKey> {
    use pnpm_package_manifest::DependencyGroup;

    let mut deps: IndexMap<String, PackageKey> = IndexMap::new();
    for group in [DependencyGroup::Dev, DependencyGroup::Prod, DependencyGroup::Optional]
        .into_iter()
        .filter(|group| dependency_groups.contains(group))
    {
        let Some(map) = project_snapshot.get_map_by_group(group) else { continue };
        let mut entries: Vec<_> = map.iter().collect();
        entries.sort_by_cached_key(|entry| entry.0.to_string());
        for (name, _) in entries {
            let alias = name.to_string();
            let Some(key) = resolved_by_alias.get(&alias) else { continue };
            deps.entry(alias).or_insert_with(|| key.clone());
        }
    }
    deps
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

fn snapshot_children(snapshot: &SnapshotEntry) -> IndexMap<String, PackageKey> {
    let mut children = IndexMap::new();
    for dependency_map in [&snapshot.dependencies, &snapshot.optional_dependencies] {
        let mut dep_entries: Vec<_> = dependency_map.iter().flat_map(|map| map.iter()).collect();
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
    children
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
    let mut direct_deps = merged_direct_deps(input);
    if let Some(workspace_packages) = input.hoisted_workspace_packages {
        direct_deps = order_direct_deps_by_workspace(direct_deps, workspace_packages);
    }
    let direct_nodes = direct_nodes(input, &mut visited);

    let mut entries: Vec<BfsEntry<'a>> =
        vec![BfsEntry { depth: -1, sort_key: String::new(), children: Cow::Owned(direct_deps) }];
    append_dependency_entries(direct_nodes, 0, input.graph, &mut visited, &mut entries);

    // pnpm sorts graph-walker results by depth and virtual-store path.
    entries.sort_by(|a, b| a.depth.cmp(&b.depth).then_with(|| a.sort_key.cmp(&b.sort_key)));

    let mut pass = HoistPass::new(input);
    // `hoist-workspace-packages`: consider each named workspace project
    // for hoisting after every importer's direct deps (v11 merges the
    // names into the root children as the LOWEST-precedence entries —
    // any direct dep wins the alias) but before depth-0 transitives.
    let mut workspace_packages_done = false;
    for entry in &entries {
        if !workspace_packages_done && entry.depth >= 0 {
            pass.hoist_workspace_packages();
            workspace_packages_done = true;
        }
        for (alias, child_node_id) in entry.children.iter() {
            pass.place_child(alias, child_node_id);
        }
    }

    // A graph whose entries are all depth −1 (no transitives) never
    // crossed the depth boundary above.
    if !workspace_packages_done {
        pass.hoist_workspace_packages();
    }

    Some(pass.finish())
}

/// Every importer's direct dependencies, merged into one alias map with
/// the first importer to claim an alias winning it.
fn merged_direct_deps(input: &HoistInputs<'_>) -> IndexMap<String, PackageKey> {
    let mut direct_deps = IndexMap::new();
    for importer_deps in input.direct_deps_by_importer.values() {
        for (alias, node_id) in importer_deps {
            direct_deps.entry(alias.clone()).or_insert_with(|| node_id.clone());
        }
    }
    direct_deps
}

/// Move the named workspace projects to the front of the direct-dep
/// order, keeping the relative order of everything else.
fn order_direct_deps_by_workspace(
    direct_deps: IndexMap<String, PackageKey>,
    workspace_packages: &IndexMap<String, PathBuf>,
) -> IndexMap<String, PackageKey> {
    let mut ordered = IndexMap::new();
    for name in workspace_packages.keys() {
        if let Some(node_id) = direct_deps.get(name) {
            ordered.insert(name.clone(), node_id.clone());
        }
    }
    for (alias, node_id) in direct_deps {
        ordered.entry(alias).or_insert(node_id);
    }
    ordered
}

/// The graph nodes every importer depends on directly, in importer
/// order and deduplicated through `visited`.
fn direct_nodes<'a>(
    input: &'a HoistInputs<'a>,
    visited: &mut HashSet<&'a PackageKey>,
) -> Vec<&'a PackageKey> {
    let mut direct_nodes = Vec::new();
    for importer_deps in input.direct_deps_by_importer.values() {
        for node_id in importer_deps.values() {
            let Some((graph_key, _)) = input.graph.get_key_value(node_id) else { continue };
            if visited.insert(graph_key) {
                direct_nodes.push(graph_key);
            }
        }
    }
    direct_nodes
}

/// Accumulates the hoist decisions as the sorted graph-walker entries
/// are visited. An alias is claimed by the first entry that places it,
/// so visit order is the precedence order.
struct HoistPass<'a> {
    input: &'a HoistInputs<'a>,
    /// Lowercased aliases already claimed. Seeded with every direct-dep
    /// name of the root importer (`"."`); workspace importers' deps
    /// don't seed it because they live in their own `node_modules` and
    /// don't collide with the root.
    hoisted_aliases: HashSet<String>,
    hoisted_dependencies: HoistedDependencies,
    hoisted_dependencies_by_node_id: HashMap<PackageKey, HashMap<String, HoistKind>>,
    hoisted_aliases_with_bins: Vec<(String, PackageKey)>,
    publicly_hoisted_aliases_with_bins: Vec<String>,
    /// The bin-alias vectors are emitted as `Vec`s to keep the consumer
    /// signature simple, so dedup goes through these sets. Private and
    /// public are separate so an alias can't collide across kinds.
    private_bins_seen: HashSet<String>,
    public_bins_seen: HashSet<String>,
    hoisted_workspace_aliases: Vec<(String, HoistKind, PathBuf)>,
}

impl<'a> HoistPass<'a> {
    fn new(input: &'a HoistInputs<'a>) -> Self {
        HoistPass {
            input,
            hoisted_aliases: input
                .direct_deps_by_importer
                .get(".")
                .map(|map| map.keys().map(|alias| alias.to_lowercase()).collect())
                .unwrap_or_default(),
            hoisted_dependencies: HoistedDependencies::new(),
            hoisted_dependencies_by_node_id: HashMap::new(),
            hoisted_aliases_with_bins: Vec::new(),
            publicly_hoisted_aliases_with_bins: Vec::new(),
            private_bins_seen: HashSet::new(),
            public_bins_seen: HashSet::new(),
            hoisted_workspace_aliases: Vec::new(),
        }
    }

    /// Place the named workspace projects, one deliberate divergence
    /// from v11: a placed workspace name claims its alias, so an
    /// equally-named transitive can't also hoist and clobber the link
    /// nondeterministically (v11's graph-miss `continue` skips the
    /// claim by accident).
    fn hoist_workspace_packages(&mut self) {
        for (name, dir) in self.input.hoisted_workspace_packages.into_iter().flatten() {
            let Some(hoist_kind) = self.hoist_kind(name) else { continue };
            if !self.hoisted_aliases.insert(name.to_lowercase()) {
                continue;
            }
            self.hoisted_workspace_aliases.push((name.clone(), hoist_kind, dir.clone()));
        }
    }

    /// Which hoist target the configured patterns put `alias` in, if
    /// any.
    fn hoist_kind(&self, alias: &str) -> Option<HoistKind> {
        if self.input.public_pattern.matches(alias) {
            Some(HoistKind::Public)
        } else if self.input.private_pattern.matches(alias) {
            Some(HoistKind::Private)
        } else {
            None
        }
    }

    fn place_child(&mut self, alias: &str, child_node_id: &PackageKey) {
        let Some(hoist_kind) = self.hoist_kind(alias) else { return };
        let alias_norm = alias.to_lowercase();
        if self.hoisted_aliases.contains(&alias_norm) {
            return;
        }
        // Record (childNodeId, alias) → kind unconditionally; the
        // symlink pass tolerates missing nodes via its own guard.
        self.hoisted_dependencies_by_node_id
            .entry(child_node_id.clone())
            .or_default()
            .insert(alias.to_owned(), hoist_kind);
        // From here on we need the node — bail if missing or skipped.
        // Note we do NOT claim the alias in that case, so a later
        // sibling with the same alias still gets a chance.
        let Some(node) = self.input.graph.get(child_node_id) else { return };
        if self.input.skipped.contains(child_node_id) {
            return;
        }
        if node.has_bin {
            self.record_bin(alias, child_node_id, hoist_kind);
        }
        self.hoisted_aliases.insert(alias_norm);
        self.hoisted_dependencies
            .entry(child_node_id.to_string())
            .or_default()
            .insert(alias.to_owned(), hoist_kind);
    }

    fn record_bin(&mut self, alias: &str, child_node_id: &PackageKey, hoist_kind: HoistKind) {
        match hoist_kind {
            HoistKind::Private => {
                if self.private_bins_seen.insert(alias.to_owned()) {
                    self.hoisted_aliases_with_bins.push((alias.to_owned(), child_node_id.clone()));
                }
            }
            HoistKind::Public => {
                if self.public_bins_seen.insert(alias.to_owned()) {
                    self.publicly_hoisted_aliases_with_bins.push(alias.to_owned());
                }
            }
        }
    }

    fn finish(self) -> HoistResult {
        HoistResult {
            hoisted_dependencies: self.hoisted_dependencies,
            hoisted_dependencies_by_node_id: self.hoisted_dependencies_by_node_id,
            hoisted_aliases_with_bins: self.hoisted_aliases_with_bins,
            publicly_hoisted_aliases_with_bins: self.publicly_hoisted_aliases_with_bins,
            hoisted_workspace_aliases: self.hoisted_workspace_aliases,
        }
    }
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

#[cfg(test)]
mod tests;
