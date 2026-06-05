//! Port of upstream's hoist algorithm from
//! [`installing/linking/hoist/src/index.ts`](https://github.com/pnpm/pnpm/blob/94240bc046/installing/linking/hoist/src/index.ts).
//!
//! Hoisting decides which transitive dependencies should *also* surface
//! outside their isolated `<virtual_store>/<pkg>/node_modules/<pkg>`
//! shape — into the project's flat `node_modules/.pnpm/node_modules/`
//! (private hoist) or directly into `<project>/node_modules/` (public
//! hoist). Two patterns drive the decision: `hoistPattern` and
//! `publicHoistPattern`. The result is a `HoistedDependencies` map (keyed
//! by snapshot key) that the install pipeline persists to
//! `.modules.yaml` and uses to drive symlink creation + bin linking.
//!
//! This module ports `getHoistedDependencies` and `symlinkHoistedDependencies`.
//! Bin linking for hoisted aliases is handled at the call site
//! ([`crate::InstallFrozenLockfile::run`]) by re-using
//! [`crate::link_direct_dep_bins`] against the private and public
//! hoisted modules dirs — the hoist pass itself only computes the
//! alias-list inputs that pass needs.

use pacquet_config::matcher::Matcher;
use pacquet_lockfile::{PackageKey, PackageMetadata, PkgName, ProjectSnapshot, SnapshotEntry};
use pacquet_modules_yaml::HoistKind;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

/// On-disk shape persisted as `hoistedDependencies` in `.modules.yaml`.
/// Keys are snapshot keys (v9 dep paths), values map alias → public/private.
///
/// `BTreeMap` matches the `pacquet_modules_yaml::Modules.hoisted_dependencies`
/// field type so the map can be assigned in directly without a conversion.
pub type HoistedDependencies = BTreeMap<String, BTreeMap<String, HoistKind>>;

/// Per-snapshot graph view the hoist BFS walks. Built from
/// `lockfile.snapshots:` + `lockfile.packages:` via
/// [`build_hoist_graph`].
///
/// Mirrors the subset of upstream's
/// [`DependenciesGraphNode`](https://github.com/pnpm/pnpm/blob/94240bc046/installing/linking/hoist/src/index.ts#L14-L21)
/// the hoist pass actually consumes: `name` (for the symlink target's
/// last segment), `children` (for BFS recursion), and `has_bin` (for
/// the bin-link gating). The other fields upstream's struct carries
/// (`dir`, `optionalDependencies`, `depPath`) are derivable from the
/// snapshot key + virtual-store-dir at the call sites that need them,
/// so they aren't materialised here.
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
    pub children: HashMap<String, PackageKey>,
    /// Mirrors upstream's `node.hasBin`. `false` when the lockfile's
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
/// Parallelized via rayon: each snapshot's children-map build is
/// independent (no shared mutable state), and the children `HashMap`
/// allocations are the dominant cost on large lockfiles. Using
/// [`rayon::prelude::ParallelIterator::collect`] into a `HashMap`
/// fans the per-snapshot work across the rayon thread pool and
/// hands the result back as a single map.
#[must_use]
pub fn build_hoist_graph(
    snapshots: &HashMap<PackageKey, SnapshotEntry>,
    packages: &HashMap<PackageKey, PackageMetadata>,
) -> HashMap<PackageKey, HoistGraphNode> {
    use rayon::prelude::*;
    snapshots
        .par_iter()
        .filter_map(|(key, snapshot)| {
            let metadata_key = key.without_peer();
            let metadata = packages.get(&metadata_key)?;
            let dep_entries = snapshot
                .dependencies
                .iter()
                .flat_map(|m| m.iter())
                .chain(snapshot.optional_dependencies.iter().flat_map(|m| m.iter()));
            let children: HashMap<String, PackageKey> = dep_entries
                // `dep_ref.resolve` is `None` for `link:` deps —
                // workspace siblings that live outside the virtual
                // store. Mirrors upstream's `if (childDepPath)`
                // check in `getChildren`.
                .filter_map(|(alias, dep_ref)| Some((alias.to_string(), dep_ref.resolve(alias)?)))
                .collect();
            Some((
                key.clone(),
                HoistGraphNode {
                    name: key.name.clone(),
                    children,
                    has_bin: metadata.has_bin == Some(true),
                },
            ))
        })
        .collect()
}

/// Per-importer direct-dependency map. Mirrors upstream's
/// [`DirectDependenciesByImporterId`](https://github.com/pnpm/pnpm/blob/94240bc046/installing/linking/hoist/src/index.ts#L23-L25).
///
/// Outer key is the importer id (`"."` for the root project; workspace
/// projects extend this in [#431]). Inner map is alias → snapshot key,
/// preserving npm-alias semantics — the alias is the directory name
/// linked under the project's `node_modules`, and the snapshot key
/// resolves where the link points.
///
/// [#431]: https://github.com/pnpm/pacquet/issues/431
pub type DirectDepsByImporter = HashMap<String, HashMap<String, PackageKey>>;

/// Build a [`DirectDepsByImporter`] from the lockfile's `importers:`
/// section, restricted to the supplied dependency groups. Mirrors the
/// `directDependenciesByImporterId` upstream computes inside
/// [`deps-restorer`](https://github.com/pnpm/pnpm/blob/94240bc046/installing/deps-restorer/src/index.ts).
///
/// The CLI passes `[Prod, Dev, Optional]` today, matching upstream's
/// dep-groups-included-in-install. Peer is filtered upfront because
/// upstream doesn't include peer-only entries in the direct-deps map
/// either (peers materialize through their host).
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
/// [`pacquet_lockfile::ImporterDepVersion::as_regular`] inside the
/// loop.
///
/// [#443]: https://github.com/pnpm/pacquet/pull/443
#[expect(
    clippy::needless_pass_by_value,
    reason = "dependency_groups is cloned per importer; the owned `impl IntoIterator + Clone` avoids a parenthesized `&(… + …)` borrow"
)]
pub fn build_direct_deps_by_importer<'a, Iter>(
    importers: Iter,
    dependency_groups: impl IntoIterator<Item = pacquet_package_manifest::DependencyGroup> + Clone,
) -> DirectDepsByImporter
where
    Iter: IntoIterator<Item = (&'a String, &'a ProjectSnapshot)>,
{
    let mut result: DirectDepsByImporter = HashMap::new();
    for (importer_id, project_snapshot) in importers {
        let mut deps: HashMap<String, PackageKey> = HashMap::new();
        for group in dependency_groups.clone() {
            if matches!(group, pacquet_package_manifest::DependencyGroup::Peer) {
                continue;
            }
            let Some(map) = project_snapshot.get_map_by_group(group) else { continue };
            for (name, spec) in map {
                // Skip `link:` workspace siblings — they don't live
                // in the snapshot graph and aren't candidates for the
                // private/public hoist (upstream handles them via the
                // separate `hoistedWorkspacePackages` shape, which is
                // out of scope for this issue per <https://github.com/pnpm/pacquet/issues/431>). For aliased
                // deps, [`ImporterDepVersion::resolved_key`] returns
                // the alias's own (name, suffix), matching the
                // snapshot key under which the package lives.
                let Some(key) = spec.version.resolved_key(name) else { continue };
                // First-wins per alias: same precedence as
                // `SymlinkDirectDependencies` (Prod beats Dev beats
                // Optional with the CLI's group order).
                deps.entry(name.to_string()).or_insert(key);
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
    /// skipped (typically: skipped optional deps). The hoist BFS still
    /// walks into them so the children of a skipped optional dep can
    /// be considered for hoisting — but the skipped dep itself is
    /// kept out of `hoistedDependencies` (mirrors upstream's
    /// `opts.skipped.has(node.depPath)` branch).
    pub skipped: &'a HashSet<PackageKey>,
    /// Boolean matcher built from `Config.hoist_pattern`. Mirrors
    /// upstream's `createMatcher(privateHoistPattern)`.
    pub private_pattern: Matcher,
    /// Boolean matcher built from `Config.public_hoist_pattern`.
    /// Public matches override private matches at the per-alias
    /// decision below — same precedence as upstream's
    /// `createGetAliasHoistType`.
    pub public_pattern: Matcher,
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
    /// privately. The install pipeline feeds this into
    /// `link_direct_dep_bins` against the private hoisted modules dir
    /// to write shims into `<vs>/node_modules/.bin`.
    pub hoisted_aliases_with_bins: Vec<String>,
    /// Aliases whose target package declares a bin and were hoisted
    /// publicly. Public-hoist bins land alongside the project's
    /// direct-dep bins in `<root>/node_modules/.bin`. Mirrors
    /// upstream's [`hoist.ts:55-65` comment](https://github.com/pnpm/pnpm/blob/94240bc046/installing/linking/hoist/src/index.ts#L55-L65)
    /// — "the bins of the publicly hoisted modules will be linked
    /// together with the bins of the project's direct dependencies".
    /// In pacquet's pipeline ordering, `SymlinkDirectDependencies`
    /// runs *before* `hoist`, so the install pipeline does an
    /// additional `link_direct_dep_bins` pass over this list after
    /// the hoist symlinks land.
    pub publicly_hoisted_aliases_with_bins: Vec<String>,
}

/// Walk the dep graph BFS and decide which aliases should be hoisted.
///
/// Mirrors upstream's
/// [`getHoistedDependencies`](https://github.com/pnpm/pnpm/blob/94240bc046/installing/linking/hoist/src/index.ts#L73-L114)
/// composed with
/// [`hoistGraph`](https://github.com/pnpm/pnpm/blob/94240bc046/installing/linking/hoist/src/index.ts#L207-L267).
///
/// Algorithm:
///
/// 1. Seed BFS from each importer's direct deps. Track depth (root's
///    direct deps are depth 0; their children depth 1; etc.).
/// 2. Sort all collected nodes by `(depth, nodeId)` so resolution
///    is deterministic and matches upstream's `sort` step.
/// 3. For each child alias of every visited node, ask the matchers
///    whether to hoist (public wins over private). If the alias was
///    already taken — case-insensitively, and seeded with the root
///    importer's direct-dep names — skip.
/// 4. Record the (node, alias) → kind in
///    `hoisted_dependencies_by_node_id`. If the target snapshot
///    is in the graph and not in `skipped`, also record its
///    `(snapshot_key, alias) → kind` in `hoisted_dependencies` and
///    claim the alias.
///
/// Returns `None` when the graph is empty (mirroring upstream's early
/// return; spares the symlink pass any work).
#[must_use]
pub fn get_hoisted_dependencies<'a>(input: &'a HoistInputs<'a>) -> Option<HoistResult> {
    if input.graph.is_empty() {
        return None;
    }

    // Seed the visited set + work queue from every importer's direct
    // deps. Each (alias, node) pair becomes both a depth-0 visit
    // entry and a starting point for BFS recursion.
    //
    // Upstream's `graphWalker` also tracks `directDeps` separately so
    // the depth=-1 importer-deps node can be sorted ahead of the
    // depth=0 transitives. Pacquet folds the same shape into the
    // `BfsEntry`s with depth `-1` for the importer pseudo-node and
    // depth `0` for the importer's direct deps.
    let mut visited: HashSet<&'a PackageKey> = HashSet::new();
    let mut entries: Vec<BfsEntry<'a>> = Vec::new();

    // Importer pseudo-nodes (depth -1) — one per importer, carrying
    // its direct-deps map as `children`. Upstream stuffs all
    // importers' deps into a single depth=-1 node; pacquet keeps them
    // separate so the per-importer `nodeId` (the importer id string)
    // sorts deterministically against itself in the per-depth sort.
    for (importer_id, direct_deps) in input.direct_deps_by_importer {
        entries.push(BfsEntry {
            depth: -1,
            // The pseudo-node's nodeId is the importer id. Upstream
            // uses an empty string `'' as T` for a single-importer
            // shape; the per-importer breakdown here gives a stable
            // (depth, importer_id) sort. Cloning the few-byte
            // importer ids is cheap; what matters perf-wise is that
            // `children` is now borrowed.
            sort_key: importer_id.clone(),
            children: direct_deps,
        });
    }

    // BFS — walk children of each direct dep at depth 0, then their
    // children at depth 1, etc. Each visited node contributes a
    // depth-N entry. The work queue holds borrowed `&PackageKey`
    // pointing into the input `direct_deps_by_importer` / the
    // graph's child maps; nothing is cloned.
    let mut queue: VecDeque<(&'a PackageKey, i32)> = VecDeque::new();
    for direct_deps in input.direct_deps_by_importer.values() {
        for node_id in direct_deps.values() {
            // `HashSet::get_or_insert_with` would let us own the
            // visited entry as `&PackageKey` from the graph itself
            // when the key matches; the simpler `contains_key + insert`
            // path here trades a redundant lookup for explicit code.
            let Some((graph_key, _)) = input.graph.get_key_value(node_id) else { continue };
            if visited.insert(graph_key) {
                queue.push_back((graph_key, 0));
            }
        }
    }
    while let Some((node_id, depth)) = queue.pop_front() {
        let node = &input.graph[node_id];
        entries.push(BfsEntry {
            depth,
            // Stringifying the node id matches the previous
            // implementation's lex-on-formatted-key tiebreaker so
            // sort output stays byte-identical. `PackageKey` itself
            // doesn't impl `Ord` (lockfile-crate types deliberately
            // don't carry semantic ordering), and switching to
            // component-wise lex would diverge from the old order
            // for scoped names. The dominant per-node cost is the
            // children HashMap, which is now borrowed; this single
            // String per node is the cheap part.
            sort_key: node_id.to_string(),
            children: &node.children,
        });
        for child_id in node.children.values() {
            // Same get_key_value trick: pull a `&'a PackageKey` out
            // of the graph's keyspace so the visited set can hold a
            // reference instead of owning a clone.
            let Some((graph_key, _)) = input.graph.get_key_value(child_id) else { continue };
            if visited.insert(graph_key) {
                queue.push_back((graph_key, depth + 1));
            }
        }
    }

    // Sort by `(depth, sort_key)` — matches upstream's
    // `sort by depth then lexCompare(nodeId)`.
    entries.sort_by(|a, b| a.depth.cmp(&b.depth).then_with(|| a.sort_key.cmp(&b.sort_key)));

    // Seed `hoisted_aliases` with every direct-dep name of the root
    // importer (`"."`). Upstream does the same — names already
    // present at the root must not be re-hoisted under different
    // versions. Workspace importers' deps don't seed this set
    // because they live in their own `node_modules` and don't
    // collide with the root.
    let mut hoisted_aliases: HashSet<String> = input
        .direct_deps_by_importer
        .get(".")
        .map(|map| map.keys().map(|k| k.to_lowercase()).collect())
        .unwrap_or_default();

    let mut hoisted_dependencies: HoistedDependencies = BTreeMap::new();
    let mut hoisted_dependencies_by_node_id: HashMap<PackageKey, HashMap<String, HoistKind>> =
        HashMap::new();
    let mut hoisted_aliases_with_bins: Vec<String> = Vec::new();
    let mut publicly_hoisted_aliases_with_bins: Vec<String> = Vec::new();
    // Dedup the bin-alias vecs — upstream uses a Set; pacquet emits
    // `Vec`s to keep the consumer signature simple but de-dups via
    // these sets first. Separate sets for private vs public so an
    // alias hoisted both privately (impossible — public always wins)
    // doesn't collide; in practice each alias lands in exactly one
    // kind.
    let mut private_bins_seen: HashSet<String> = HashSet::new();
    let mut public_bins_seen: HashSet<String> = HashSet::new();

    for entry in &entries {
        // Within a single entry's children there are no alias
        // collisions (children is a `HashMap<alias, _>`), so the
        // matcher's per-alias decision is independent of iteration
        // order. The on-disk output goes through `BTreeMap`
        // serialization which sorts at write time; sorting again
        // here would cost ~entries × log(avg-fanout) extra and
        // produce no observable difference. Iterate the HashMap
        // directly.
        for (alias, child_node_id) in entry.children {
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
            // Record (childNodeId, alias) → kind unconditionally —
            // upstream does too, even when the target snapshot is
            // missing or skipped. The symlink pass tolerates
            // missing nodes via its own guard.
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
                            hoisted_aliases_with_bins.push(alias.clone());
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
            // Snapshot key as String — matches the
            // `BTreeMap<String, _>` shape of `.modules.yaml`'s
            // `hoistedDependencies`.
            hoisted_dependencies
                .entry(child_node_id.to_string())
                .or_default()
                .insert(alias.clone(), hoist_kind);
        }
    }

    Some(HoistResult {
        hoisted_dependencies,
        hoisted_dependencies_by_node_id,
        hoisted_aliases_with_bins,
        publicly_hoisted_aliases_with_bins,
    })
}

/// Internal BFS row. `children` borrows from the input graph (or the
/// importer's direct-deps map for the depth=-1 pseudo-nodes) so the
/// BFS allocates one `Vec<BfsEntry>` plus the `visited`/`queue`
/// collections — no per-node `HashMap` clones, which was the dominant
/// allocation hotspot pre-optimization. `sort_key` is still a
/// `String` because `PackageKey` doesn't carry an `Ord` impl that
/// would match the old `to_string()` lex order; that single
/// allocation per node is cheap relative to what was a `HashMap` clone.
struct BfsEntry<'a> {
    depth: i32,
    sort_key: String,
    children: &'a HashMap<String, PackageKey>,
}

/// Create the hoist symlinks. Mirrors upstream's
/// [`symlinkHoistedDependencies`](https://github.com/pnpm/pnpm/blob/94240bc046/installing/linking/hoist/src/index.ts#L269-L307).
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
/// Existing symlinks are left in place — `EEXIST` from the underlying
/// [`pacquet_fs::symlink_dir`] is swallowed, mirroring upstream's
/// `symlinkDir({ overwrite: false })`. The "overwrite a
/// virtual-store-resident link" branch upstream additionally does
/// (via `resolveLinkTarget` + `isSubdir`) is intentionally not
/// ported — see the `external_symlink_introspection` `known_failure`
/// reason.
///
/// Two-phase to amortize directory creation:
///
/// 1. Walk the input once to collect every `(target, dest)` symlink
///    pair plus the set of scope-dir parents (`<root>/@scope`)
///    needed by scoped aliases.
/// 2. `create_dir_all` the two hoisted-modules roots and each
///    distinct scope dir — once per dir, not per symlink. The
///    previous version called `create_dir_all` inside
///    [`crate::symlink_package()`] on every symlink, so a 1k-alias install
///    paid 1k redundant stats on the same handful of parents.
/// 3. `par_iter` the pair list and issue `symlinkat()` syscalls in
///    parallel via rayon. Each pair is now a single syscall — no
///    parent-dir prep — so the only contention is the kernel's
///    inode lock on the parent directory, which is dominated by
///    the syscall latency itself on macOS APFS / Linux ext4.
pub fn symlink_hoisted_dependencies(
    hoisted_by_node_id: &HashMap<PackageKey, HashMap<String, HoistKind>>,
    graph: &HashMap<PackageKey, HoistGraphNode>,
    layout: &crate::VirtualStoreLayout,
    private_hoisted_modules_dir: &std::path::Path,
    public_hoisted_modules_dir: &std::path::Path,
    skipped: &std::collections::HashSet<PackageKey>,
) -> Result<(), crate::SymlinkPackageError> {
    use rayon::prelude::*;
    use std::{
        collections::HashSet,
        io::ErrorKind,
        path::{Path, PathBuf},
        sync::Arc,
    };

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
        // fail as a junction (Windows). Mirrors the guard
        // `get_hoisted_dependencies` already applies before
        // recording the *parent's* hoisting decisions —
        // `hoisted_dependencies_by_node_id` records the
        // (target, alias) pair unconditionally upstream of that
        // guard (matching pnpm's structure), so the filter has to
        // run here too. Covers slice 4 (`fetch_failed`), slice 5
        // (`optional_excluded`), and the slice 1 installability
        // path uniformly.
        if skipped.contains(node_id) {
            continue;
        }
        let Some(node) = graph.get(node_id) else { continue };
        let dep_dir =
            Arc::new(layout.slot_dir(node_id).join("node_modules").join(name_to_dir(&node.name)));
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
            match pacquet_fs::symlink_dir(dep_dir.as_path(), &dest) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == ErrorKind::AlreadyExists => Ok(()),
                Err(error) => Err(crate::SymlinkPackageError::SymlinkDir {
                    symlink_target: dep_dir.as_path().to_path_buf(),
                    symlink_path: dest,
                    error,
                }),
            }
        },
    )
}

/// Render a [`PkgName`] into its on-disk segment under `node_modules`.
/// Scoped names land at `@scope/name`, unscoped at `name`. Mirrors the
/// helper used by the bin linker (kept private to this module to avoid
/// cross-crate dependency churn for one path-join helper).
fn name_to_dir(name: &PkgName) -> std::path::PathBuf {
    match &name.scope {
        Some(scope) => std::path::PathBuf::from(format!("@{scope}")).join(&name.bare),
        None => std::path::PathBuf::from(&name.bare),
    }
}

#[cfg(test)]
mod tests;
