use super::{gvs_layout_cache, gvs_version_segment, local_directory_scope};
use crate::{AllowBuildPolicy, install_frozen_lockfile::find_own_runtime_node_major};
use indexmap::IndexMap;
use pnpm_deps_path::get_pkg_id_with_patch_hash;
use pnpm_graph_hasher::{
    DepsGraphNode, DepsStateCache, calc_graph_node_hash, engine_name,
    format_global_virtual_store_path,
};
use pnpm_lockfile::{
    LockfileResolution, PackageKey, PackageMetadata, PkgIdWithPatchHash, SnapshotEntry,
};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

/// Build the dependency graph from the lockfile's `snapshots` /
/// `packages` sections. Every entry in `snapshots` becomes a node whose
/// `full_pkg_id` is `<pkg_id_with_patch_hash>:<integrity>` (for tarball
/// / registry resolutions) and whose `children` are the
/// alias→snapshot-key edges pulled from the snapshot's combined
/// `dependencies` + `optionalDependencies`.
///
/// Resolved `link:` targets become leaf nodes whose identity is the
/// absolute target path. Modeling them as children makes the target
/// participate in every ancestor's recursive hash while keeping slots
/// shared between projects that resolve the link to the same directory.
///
/// Packages whose metadata is missing or whose resolution has no
/// `integrity` (directory / git) are emitted with the bare
/// `pkg_id_with_patch_hash` as their `full_pkg_id`. The frozen-
/// lockfile install path rejects those resolutions before reaching the
/// linker, so a stub `full_pkg_id` here is safe — the GVS hash for an
/// install that contains one of those snapshots is irrelevant because
/// the install will error out before consulting it.
pub(super) fn lockfile_to_dep_graph(
    snapshots: &HashMap<PackageKey, SnapshotEntry>,
    packages: Option<&HashMap<PackageKey, PackageMetadata>>,
    lockfile_dir: Option<&Path>,
) -> HashMap<String, DepsGraphNode<String>> {
    let mut graph = HashMap::with_capacity(snapshots.len());
    let mut link_target_nodes = HashSet::new();
    for (snapshot_key, snapshot) in snapshots {
        let children = crate::deps_graph::build_children_with(snapshot, |alias, dep_ref| {
            child_graph_key(alias, dep_ref, lockfile_dir)
        });
        link_target_nodes
            .extend(children.values().filter(|child_key| child_key.starts_with("link:")).cloned());
        graph.insert(
            snapshot_key.to_string(),
            DepsGraphNode { full_pkg_id: full_pkg_id_of(snapshot_key, packages), children },
        );
    }
    for link_target_node in link_target_nodes {
        graph.insert(
            link_target_node.clone(),
            DepsGraphNode { full_pkg_id: link_target_node, children: IndexMap::default() },
        );
    }
    graph
}
pub(super) fn child_graph_key(
    alias: &pnpm_lockfile::PkgName,
    dep_ref: &pnpm_lockfile::SnapshotDepRef,
    lockfile_dir: Option<&Path>,
) -> Option<String> {
    if let Some(snapshot_key) = dep_ref.resolve(alias) {
        return Some(snapshot_key.to_string());
    }
    let link_target = dep_ref.as_link_target()?;
    let resolved = pnpm_fs::lexical_normalize(&lockfile_dir?.join(link_target));
    Some(format!("link:{}", resolved.to_string_lossy()))
}
pub(super) fn full_pkg_id_of(
    snapshot_key: &PackageKey,
    packages: Option<&HashMap<PackageKey, PackageMetadata>>,
) -> String {
    let pkg_id_with_patch_hash =
        PkgIdWithPatchHash::from(get_pkg_id_with_patch_hash(&snapshot_key.to_string()).to_string());
    let resolution = packages
        .and_then(|packages| packages.get(&snapshot_key.without_peer()))
        .map(|meta| &meta.resolution);
    create_full_pkg_id(&pkg_id_with_patch_hash, resolution)
}
/// Length-prefixed so two adjacent fields cannot be confused with one
/// longer field carrying the same bytes.
pub(super) fn write_field(hasher: &mut sha2::Sha256, value: &str) {
    use sha2::Digest as _;
    hasher.update((value.len() as u64).to_le_bytes());
    hasher.update(value.as_bytes());
}
/// Hashes every snapshot's global-virtual-store slot suffix over one dep
/// graph, gating set and memo for the whole lockfile.
pub(super) struct GvsHasher<'h> {
    graph: HashMap<String, DepsGraphNode<String>>,
    /// The engine-agnostic gating set. `None` disables gating so every
    /// snapshot still hashes with its engine string.
    build_required_dep_paths: Option<HashSet<String>>,
    cache: DepsStateCache<String>,
    /// One conversion for the whole lockfile: the same string scopes
    /// every local directory snapshot in it.
    ///
    /// Lossy on purpose: the TypeScript CLI hashes the same slot from a
    /// JS string, and Node decodes a path as UTF-8 with replacement, so
    /// this is the identical input. Hashing the raw bytes instead would
    /// give the two stacks different slots for the same project.
    project_scope: Option<std::borrow::Cow<'h, str>>,
    engine: Option<&'h str>,
    packages: Option<&'h HashMap<PackageKey, PackageMetadata>>,
}
impl<'h> GvsHasher<'h> {
    pub(super) fn new(
        snapshots: &HashMap<PackageKey, SnapshotEntry>,
        packages: Option<&'h HashMap<PackageKey, PackageMetadata>>,
        engine: Option<&'h str>,
        allow_build_policy: Option<&AllowBuildPolicy>,
        lockfile_dir: Option<&'h Path>,
    ) -> Self {
        let graph = lockfile_to_dep_graph(snapshots, packages, lockfile_dir);
        let build_required_dep_paths =
            allow_build_policy.map(|policy| engine_gating_dep_paths(policy, snapshots, &graph));
        Self {
            graph,
            build_required_dep_paths,
            cache: HashMap::new(),
            project_scope: lockfile_dir.map(|dir| dir.to_string_lossy()),
            engine,
            packages,
        }
    }

    /// Per-snapshot engine resolution: a snapshot that declares its own
    /// `engines.runtime` carries the desugared
    /// `dependencies.node: 'runtime:<version>'` pin, which has to drive
    /// the engine portion of *its* hash rather than the install-wide
    /// fallback. Precedence: own pin first, install-wide fallback
    /// second. Default host platform / arch (`None`, `None`) matches
    /// whatever the caller used to format the fallback `engine` so the
    /// two strings remain comparable across snapshots in one install.
    /// Every snapshot's suffix, walked in lockfile key order rather
    /// than `HashMap` order: [`calc_graph_node_hash`] memoizes into
    /// `cache`, and for a snapshot inside a dependency cycle the digest
    /// that lands there depends on which snapshot the walk reached it
    /// from.
    pub(super) fn suffixes(
        &mut self,
        snapshots: &HashMap<PackageKey, SnapshotEntry>,
    ) -> HashMap<PackageKey, String> {
        let mut gvs_suffixes = HashMap::with_capacity(snapshots.len());
        for (snapshot_key, snapshot) in crate::deps_graph::in_lockfile_order(snapshots) {
            gvs_suffixes.insert(snapshot_key.clone(), self.suffix(snapshot_key, snapshot));
        }
        gvs_suffixes
    }

    /// Digest of everything [`Self::suffixes`] would read, so a cached
    /// map can be filed under it.
    ///
    /// Taken from the dep graph this hasher already built, plus the
    /// per-snapshot values `suffix` reads that the graph does not
    /// carry. Hashing the graph rather than the lockfile it came from
    /// is what keeps this honest: the suffixes and the key are then two
    /// functions of the same values, and no re-read can put them out of
    /// step.
    ///
    /// Cheap next to what it guards. On a 1355-node lockfile the graph
    /// build is ~2 ms and the suffix loop it lets us skip is ~8 ms.
    pub(super) fn fingerprint(&self, snapshots: &HashMap<PackageKey, SnapshotEntry>) -> String {
        use sha2::Digest as _;
        let mut hasher = sha2::Sha256::new();
        write_field(&mut hasher, gvs_layout_cache::CACHE_FORMAT_VERSION);
        // `None` and `Some("")` are different payloads downstream, so
        // they must not collapse here.
        match self.engine {
            Some(engine) => {
                hasher.update([1_u8]);
                write_field(&mut hasher, engine);
            }
            None => hasher.update([0_u8]),
        }
        write_field(&mut hasher, self.project_scope.as_deref().unwrap_or(""));
        self.write_gating_set(&mut hasher);
        self.write_graph(&mut hasher);
        self.write_snapshot_extras(&mut hasher, snapshots);
        format!("{:x}", hasher.finalize())
    }

    /// The set that decides which snapshots carry the engine string.
    ///
    /// Tagged, because `None` is not an empty set: `calc_graph_node_hash`
    /// reads `None` as "gating off" and puts the engine in every
    /// snapshot's hash, while an empty set puts it in none.
    fn write_gating_set(&self, hasher: &mut sha2::Sha256) {
        use sha2::Digest as _;
        let Some(paths) = self.build_required_dep_paths.as_ref() else {
            return hasher.update([0_u8]);
        };
        hasher.update([1_u8]);
        let mut sorted: Vec<&str> = paths.iter().map(String::as_str).collect();
        sorted.sort_unstable();
        write_field(hasher, &sorted.join("\u{0}"));
    }

    /// The dep graph, in an order a `HashMap` cannot vary.
    fn write_graph(&self, hasher: &mut sha2::Sha256) {
        use sha2::Digest as _;
        let mut node_keys: Vec<&String> = self.graph.keys().collect();
        node_keys.sort_unstable();
        hasher.update((node_keys.len() as u64).to_le_bytes());
        for node_key in node_keys {
            let node = &self.graph[node_key];
            write_field(hasher, node_key);
            write_field(hasher, &node.full_pkg_id);
            hasher.update((node.children.len() as u64).to_le_bytes());
            for (alias, child_key) in &node.children {
                write_field(hasher, alias);
                write_field(hasher, child_key);
            }
        }
    }

    /// What [`Self::suffix`] reads that the graph does not carry: a
    /// snapshot's own `engines.runtime` pin, and the version segment
    /// its metadata contributes.
    fn write_snapshot_extras(
        &self,
        hasher: &mut sha2::Sha256,
        snapshots: &HashMap<PackageKey, SnapshotEntry>,
    ) {
        for (snapshot_key, snapshot) in crate::deps_graph::in_lockfile_order(snapshots) {
            let metadata_key = snapshot_key.without_peer();
            let metadata = self.packages.and_then(|packages| packages.get(&metadata_key));
            write_field(hasher, &snapshot_key.to_string());
            write_field(
                hasher,
                &find_own_runtime_node_major(snapshot)
                    .map(|major| engine_name(major, None, None))
                    .unwrap_or_default(),
            );
            write_field(hasher, &gvs_version_segment(metadata, &metadata_key.suffix));
        }
    }

    fn suffix(&mut self, snapshot_key: &PackageKey, snapshot: &SnapshotEntry) -> String {
        let own_engine =
            find_own_runtime_node_major(snapshot).map(|major| engine_name(major, None, None));
        let metadata_key = snapshot_key.without_peer();
        let metadata = self.packages.and_then(|packages| packages.get(&metadata_key));
        let hex_digest = calc_graph_node_hash(
            &self.graph,
            &mut self.cache,
            &snapshot_key.to_string(),
            own_engine.as_deref().or(self.engine),
            self.build_required_dep_paths.as_ref(),
            local_directory_scope(metadata, &metadata_key.suffix, self.project_scope.as_deref()),
        );
        format_global_virtual_store_path(
            &metadata_key.name.to_string(),
            &gvs_version_segment(metadata, &metadata_key.suffix),
            &hex_digest,
        )
    }
}
pub(super) fn engine_gating_dep_paths(
    policy: &AllowBuildPolicy,
    snapshots: &HashMap<PackageKey, SnapshotEntry>,
    graph: &HashMap<String, DepsGraphNode<String>>,
) -> HashSet<String> {
    let built_dep_paths = snapshots
        .keys()
        .filter(|key| policy.check(&key.without_peer().to_string()) == Some(true))
        .map(ToString::to_string)
        .collect();
    pnpm_graph_hasher::build_required_dep_paths(graph, &built_dep_paths)
}
/// `variations` (cross-platform variant) resolutions don't exist in
/// pacquet's lockfile model yet — when they're added, this helper
/// will need a `selectPlatformVariant` branch to pick the right
/// integrity.
pub(super) fn create_full_pkg_id(
    pkg_id_with_patch_hash: &PkgIdWithPatchHash,
    resolution: Option<&LockfileResolution>,
) -> String {
    match resolution.and_then(LockfileResolution::integrity) {
        Some(integrity) => format!("{pkg_id_with_patch_hash}:{integrity}"),
        // Directory / git / missing-metadata fall through to the bare
        // id. The install path rejects these resolutions before the
        // hash is consulted (see
        // [`crate::InstallPackageBySnapshotError::UnsupportedResolution`]),
        // so the value never actually drives a slot path on disk.
        None => pkg_id_with_patch_hash.to_string(),
    }
}
