//! Adapter that converts the resolver's [`DependenciesGraph`] into a
//! [`Lockfile`].
//!
//! Ports upstream pnpm's
//! [`updateLockfile`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/updateLockfile.ts)
//! plus the snapshot-vs-packages split that
//! [`convertToLockfileFile`](https://github.com/pnpm/pnpm/blob/094aa6e57b/lockfile/fs/src/lockfileFormatConverters.ts)
//! applies on write — upstream's in-memory `LockfileObject` carries one
//! merged `PackageSnapshot` per depPath which the writer fans out into
//! the v9 `packages:` + `snapshots:` pair, while pacquet's
//! [`Lockfile`] already holds the two maps separately, so this adapter
//! emits them directly.

use std::collections::{HashMap, HashSet};

use pacquet_lockfile::{
    ComVer, ImporterDepVersion, Lockfile, LockfileSettings, LockfileVersion, PackageKey,
    PackageMetadata, PeerDependencyMeta, PkgName, PkgNameVerPeer, PkgVerPeer, ProjectSnapshot,
    ResolvedDependencyMap, ResolvedDependencySpec, SnapshotDepRef, SnapshotEntry,
};
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pacquet_resolving_deps_resolver::{
    DepPath, DependenciesGraph, DependenciesGraphNode, ResolveImporterResult,
};
use pacquet_resolving_resolver_base::ResolveResult;
use serde_json::Value;

/// Options threaded into [`dependencies_graph_to_lockfile`].
pub struct GraphToLockfileOptions<'a> {
    /// The on-disk `package.json` for the root importer. Used to source
    /// each direct dependency's specifier (the value the user wrote)
    /// and the importer-level dep-group classification
    /// (`dependencies` vs `devDependencies` vs `optionalDependencies`).
    pub manifest: &'a PackageManifest,
    /// Resolver output: graph keyed by depPath plus the importer's
    /// `alias → DepPath` map.
    pub resolved: &'a ResolveImporterResult,
    /// Round-tripped into the lockfile's top-level `settings:` block
    /// so a subsequent pnpm install can compare its own settings via
    /// `@pnpm/lockfile.settings-checker`'s `getOutdatedLockfileSetting`.
    pub auto_install_peers: bool,
    pub exclude_links_from_lockfile: bool,
    /// `overrides` recorded into the lockfile so a later install can
    /// detect drift. Mirrors upstream's `lockfile.overrides` field.
    pub overrides: Option<HashMap<String, String>>,
    /// `ignoredOptionalDependencies` recorded the same way.
    pub ignored_optional_dependencies: Option<Vec<String>>,
}

/// Build a [`Lockfile`] from the resolver's [`DependenciesGraph`] plus
/// the importer-side context needed to populate the `importers:` map.
///
/// The output reflects pnpm v9's wire shape:
///
/// - `importers["."]` carries the root project's `specifiers` and the
///   classified `dependencies` / `devDependencies` / `optionalDependencies`
///   maps keyed by the manifest's declared alias.
/// - `packages` carries one [`PackageMetadata`] entry per resolved
///   package version, keyed by the *peer-stripped* depPath (the
///   `pkgIdWithPatchHash`).
/// - `snapshots` carries one [`SnapshotEntry`] per *peer-suffixed*
///   depPath — peer variants of the same package each get their own
///   snapshot row.
pub fn dependencies_graph_to_lockfile(opts: GraphToLockfileOptions<'_>) -> Lockfile {
    let GraphToLockfileOptions {
        manifest,
        resolved,
        auto_install_peers,
        exclude_links_from_lockfile,
        overrides,
        ignored_optional_dependencies,
    } = opts;

    let importer = build_root_importer(manifest, resolved);

    let (packages, snapshots) = build_packages_and_snapshots(&resolved.peers_result.graph);

    let mut importers: HashMap<String, ProjectSnapshot> = HashMap::with_capacity(1);
    importers.insert(Lockfile::ROOT_IMPORTER_KEY.to_string(), importer);

    Lockfile {
        lockfile_version: LockfileVersion::<9>::try_from(ComVer::new(9, 0))
            .expect("lockfileVersion 9.0 is always compatible with MAJOR=9"),
        settings: Some(LockfileSettings { auto_install_peers, exclude_links_from_lockfile }),
        overrides: overrides.filter(|map| !map.is_empty()),
        ignored_optional_dependencies: ignored_optional_dependencies
            .filter(|list| !list.is_empty()),
        importers,
        packages: (!packages.is_empty()).then_some(packages),
        snapshots: (!snapshots.is_empty()).then_some(snapshots),
    }
}

/// Build the root importer's [`ProjectSnapshot`] from the on-disk
/// manifest's declared deps + the resolver's per-alias `DepPath` map.
///
/// Mirrors upstream's
/// [`addDirectDependenciesToLockfile`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/index.ts#L417-L484):
/// the manifest decides which dep group each alias lives under, and the
/// resolver decides the resolved version (peer-suffixed when peers are
/// involved, alias-prefixed when the alias and real name differ).
fn build_root_importer(
    manifest: &PackageManifest,
    resolved: &ResolveImporterResult,
) -> ProjectSnapshot {
    let direct = &resolved.peers_result.direct_dependencies_by_alias;
    let graph = &resolved.peers_result.graph;

    let mut dependencies: ResolvedDependencyMap = HashMap::new();
    let mut dev_dependencies: ResolvedDependencyMap = HashMap::new();
    let mut optional_dependencies: ResolvedDependencyMap = HashMap::new();
    let mut specifiers: HashMap<String, String> = HashMap::new();

    let alias_to_group = manifest_alias_to_group(manifest);

    for (alias, dep_path) in direct {
        let Some(node) = graph.get(dep_path) else { continue };
        let Ok(name_for_key) = PkgName::parse(alias.as_str()) else { continue };
        // Skip aliases the manifest doesn't declare. The resolver's
        // `direct_dependencies_by_alias` includes auto-installed peers
        // hoisted to the importer when `autoInstallPeers: true` is on,
        // but pnpm's `addDirectDependenciesToLockfile` iterates only
        // over `getAllDependenciesFromManifest(manifest)` — transitive
        // auto-installed peers never enter `importer.dependencies` /
        // `importer.specifiers`, only the snapshots graph below. Writing
        // them here would carry specifiers the manifest can't satisfy
        // through `satisfies_package_manifest` and force every later
        // install onto the fresh-resolve path.
        let Some(specifier) = read_manifest_specifier(manifest, alias) else { continue };
        let version = importer_dep_version(alias, node);
        let spec = ResolvedDependencySpec { specifier: specifier.clone(), version };
        specifiers.insert(alias.clone(), specifier);
        let group = alias_to_group.get(alias).copied().unwrap_or(DependencyGroup::Prod);
        match group {
            DependencyGroup::Dev => {
                dev_dependencies.insert(name_for_key, spec);
            }
            DependencyGroup::Optional => {
                optional_dependencies.insert(name_for_key, spec);
            }
            DependencyGroup::Prod | DependencyGroup::Peer => {
                dependencies.insert(name_for_key, spec);
            }
        }
    }

    ProjectSnapshot {
        specifiers: (!specifiers.is_empty()).then_some(specifiers),
        dependencies: (!dependencies.is_empty()).then_some(dependencies),
        dev_dependencies: (!dev_dependencies.is_empty()).then_some(dev_dependencies),
        optional_dependencies: (!optional_dependencies.is_empty()).then_some(optional_dependencies),
        dependencies_meta: None,
        publish_directory: None,
    }
}

/// Map each direct-dep alias to the manifest group it appears in.
/// `dependencies` wins over `devDependencies` wins over
/// `optionalDependencies` when an alias is duplicated across groups —
/// mirrors upstream's
/// [`getAliasToDependencyTypeMap`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/index.ts#L500-L511)
/// (first-write-wins over `DEPENDENCIES_FIELDS`).
fn manifest_alias_to_group(manifest: &PackageManifest) -> HashMap<String, DependencyGroup> {
    let mut out: HashMap<String, DependencyGroup> = HashMap::new();
    for group in [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional] {
        for (alias, _) in manifest.dependencies([group]) {
            out.entry(alias.to_string()).or_insert(group);
        }
    }
    out
}

/// Look up the user-written specifier for `alias` in the manifest's
/// `dependencies` / `devDependencies` / `optionalDependencies` /
/// `peerDependencies` maps in pnpm's
/// [`DEPENDENCIES_FIELDS`](https://github.com/pnpm/pnpm/blob/097983fbca/packages/types/src/misc.ts)
/// precedence order. Returns `None` for a peer-only entry that was
/// auto-installed but isn't recorded as a direct dep in the manifest —
/// such entries don't go into the importer's `specifiers` map.
fn read_manifest_specifier(manifest: &PackageManifest, alias: &str) -> Option<String> {
    for group in [
        DependencyGroup::Prod,
        DependencyGroup::Dev,
        DependencyGroup::Optional,
        DependencyGroup::Peer,
    ] {
        let group_key: &str = group.into();
        if let Some(map) = manifest.value().get(group_key).and_then(Value::as_object)
            && let Some(spec) = map.get(alias).and_then(Value::as_str)
        {
            return Some(spec.to_string());
        }
    }
    None
}

/// Build the version cell for an importer-level dependency, mirroring
/// pnpm's [`depPathToRef`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/depPathToRef.ts):
///
/// - When the resolved real name equals the manifest alias and the
///   depPath starts with `<name>@`, drop the prefix so the importer
///   carries just the version-with-peer string.
/// - Otherwise — npm-alias entries, where the alias and real name
///   differ — keep the full `<real>@<version-with-peer>` string so the
///   snapshot key the importer points at is unambiguous.
///
/// `link:` shows up only on the snapshot-level path (workspace siblings
/// are routed through [`crate::InstallWithFreshLockfile::workspace_packages`]
/// today), so this function doesn't emit [`ImporterDepVersion::Link`];
/// the workspace-importer port adds that arm.
fn importer_dep_version(alias: &str, node: &DependenciesGraphNode) -> ImporterDepVersion {
    let real_name = real_name(&node.resolve_result);
    let dep_path_str = node.dep_path.as_str();

    if let Some(real) = real_name.as_deref() {
        let prefix = format!("{real}@");
        if alias == real
            && let Some(ver) = dep_path_str.strip_prefix(&prefix)
            && let Ok(parsed) = ver.parse::<PkgVerPeer>()
        {
            return ImporterDepVersion::Regular(parsed);
        }
    }
    dep_path_str
        .parse::<PkgNameVerPeer>()
        .map(ImporterDepVersion::Alias)
        .expect("dep paths produced by the resolver always parse as PkgNameVerPeer")
}

/// `Some(real_name)` when the resolver produced a structured name; `None`
/// for resolvers that learn the name from the fetched manifest (git,
/// tarball, file). Matches the fallback path
/// [`pkg_name_version`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/resolvePeers.ts#L23-L37)
/// uses in the resolve-peers walker — and the same shape the
/// `name_ver` field carries upstream.
fn real_name(result: &ResolveResult) -> Option<String> {
    Some(result.name_ver.as_ref()?.name.to_string())
}

/// Walk the depPath-keyed [`DependenciesGraph`] and emit the matching
/// `(PackageMetadata, SnapshotEntry)` pair for each node — fanned out
/// across the two top-level maps the v9 lockfile splits.
///
/// Multiple snapshot entries (peer variants) share one packages entry,
/// so the loop dedupes by peer-stripped key.
fn build_packages_and_snapshots(
    graph: &DependenciesGraph,
) -> (HashMap<PackageKey, PackageMetadata>, HashMap<PackageKey, SnapshotEntry>) {
    let mut packages: HashMap<PackageKey, PackageMetadata> = HashMap::new();
    let mut snapshots: HashMap<PackageKey, SnapshotEntry> = HashMap::new();

    for node in graph.values() {
        let Ok(snapshot_key) = node.dep_path.as_str().parse::<PackageKey>() else { continue };
        let metadata_key = snapshot_key.without_peer();

        let snapshot = build_snapshot_entry(node, graph);
        snapshots.insert(snapshot_key, snapshot);

        packages.entry(metadata_key).or_insert_with(|| build_package_metadata(node));
    }

    (packages, snapshots)
}

/// Build the per-`(name, version)` [`PackageMetadata`] block for the
/// lockfile's `packages:` map. Pulls `engines` / `cpu` / `os` / `libc` /
/// `deprecated` / `hasBin` / `bundledDependencies` / `peerDependencies`
/// off the resolver's manifest fragment when present.
///
/// Mirrors the field-by-field copy in
/// [`toLockfileDependency`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/updateLockfile.ts#L49-L156)
/// for the per-package half (excludes the per-snapshot fields
/// `dependencies` / `optionalDependencies` / `transitivePeerDependencies` /
/// `optional` / `patched`, which go on the snapshot below).
fn build_package_metadata(node: &DependenciesGraphNode) -> PackageMetadata {
    let manifest = node.resolve_result.manifest.as_deref();

    let engines = manifest
        .and_then(|m| m.get("engines"))
        .and_then(Value::as_object)
        .map(|map| {
            map.iter()
                .filter_map(|(name, value)| {
                    let range = value.as_str()?;
                    if range == "*" {
                        return None;
                    }
                    Some((name.clone(), range.to_string()))
                })
                .collect::<HashMap<String, String>>()
        })
        .filter(|map| !map.is_empty());

    let cpu = read_string_list(manifest, "cpu");
    let os = read_string_list(manifest, "os");
    let libc = read_string_list(manifest, "libc");

    let deprecated =
        manifest.and_then(|m| m.get("deprecated")).and_then(Value::as_str).map(ToString::to_string);

    let has_bin = manifest_has_bin(manifest);

    let bundled_dependencies = read_string_list(manifest, "bundledDependencies")
        .or_else(|| read_string_list(manifest, "bundleDependencies"));

    let (peer_dependencies, peer_dependencies_meta) = build_peer_dep_blocks(node);

    PackageMetadata {
        resolution: node.resolve_result.resolution.clone(),
        engines,
        cpu,
        os,
        libc,
        deprecated,
        has_bin,
        prepare: None,
        bundled_dependencies,
        peer_dependencies,
        peer_dependencies_meta,
    }
}

/// Read a JSON array field off the resolver's manifest fragment and
/// flatten it into a `Vec<String>`. `None` when the field is missing or
/// not an array of strings — matches upstream's silent drop for
/// malformed metadata.
fn read_string_list(manifest: Option<&Value>, key: &str) -> Option<Vec<String>> {
    let arr = manifest?.get(key)?.as_array()?;
    let out: Vec<String> = arr.iter().filter_map(Value::as_str).map(ToString::to_string).collect();
    (!out.is_empty()).then_some(out)
}

/// `Some(true)` when the manifest declares a `bin` entry (string or
/// non-empty object map). Pacquet records the same `hasBin: true`
/// signal pnpm writes; the field is dropped entirely when absent.
fn manifest_has_bin(manifest: Option<&Value>) -> Option<bool> {
    let value = manifest?.get("bin")?;
    let present = match value {
        Value::String(s) => !s.is_empty(),
        Value::Object(map) => !map.is_empty(),
        _ => false,
    };
    present.then_some(true)
}

/// Returned `Option`-pair from [`build_peer_dep_blocks`]: the
/// `peerDependencies` map (name → range) and the
/// `peerDependenciesMeta` map (name → `{ optional: true }`).
type PeerDepBlocks = (Option<HashMap<String, String>>, Option<HashMap<String, PeerDependencyMeta>>);

/// Split the resolver's `peer_dependencies` into the
/// `peerDependencies` (name → range) and `peerDependenciesMeta`
/// (name → `{ optional: true }`) blocks pnpm writes onto `packages:`.
fn build_peer_dep_blocks(node: &DependenciesGraphNode) -> PeerDepBlocks {
    if node.peer_dependencies.is_empty() {
        return (None, None);
    }
    let mut peers: HashMap<String, String> = HashMap::new();
    let mut peers_meta: HashMap<String, PeerDependencyMeta> = HashMap::new();
    for (name, peer) in &node.peer_dependencies {
        peers.insert(name.clone(), peer.version.clone());
        if peer.optional {
            peers_meta.insert(name.clone(), PeerDependencyMeta { optional: true });
        }
    }
    let peers_meta = (!peers_meta.is_empty()).then_some(peers_meta);
    (Some(peers), peers_meta)
}

/// Build the per-snapshot [`SnapshotEntry`] for this depPath. Mirrors
/// the per-snapshot half of upstream's
/// [`toLockfileDependency`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/updateLockfile.ts#L49-L156):
/// the `dependencies` / `optionalDependencies` partition follows the
/// node's own `optionalDependencies` set and peer-optional flag;
/// `transitivePeerDependencies` is sorted; `optional` is copied from
/// the resolver's [`DependenciesGraphNode::optional`] (AND-folded
/// across every visit so a snapshot is marked `optional: true` only
/// when every path from any importer to it goes through an
/// `optionalDependencies` edge). `BuildModules` consults this flag to
/// decide whether a build failure is fatal or should be reported via
/// `pnpm:skipped-optional-dependency`.
fn build_snapshot_entry(node: &DependenciesGraphNode, graph: &DependenciesGraph) -> SnapshotEntry {
    let optional_children = optional_children_of(node);

    let mut dependencies: HashMap<PkgName, SnapshotDepRef> = HashMap::new();
    let mut optional_dependencies: HashMap<PkgName, SnapshotDepRef> = HashMap::new();
    for (alias, child_dep_path) in &node.children {
        let Ok(alias_name) = PkgName::parse(alias.as_str()) else { continue };
        let Some(child_ref) = snapshot_dep_ref(alias, child_dep_path, graph) else { continue };
        if optional_children.contains(alias.as_str()) {
            optional_dependencies.insert(alias_name, child_ref);
        } else {
            dependencies.insert(alias_name, child_ref);
        }
    }

    let transitive: Vec<String> = {
        let mut list: Vec<String> = node.transitive_peer_dependencies.iter().cloned().collect();
        list.sort();
        list
    };

    SnapshotEntry {
        id: None,
        dependencies: (!dependencies.is_empty()).then_some(dependencies),
        optional_dependencies: (!optional_dependencies.is_empty()).then_some(optional_dependencies),
        transitive_peer_dependencies: (!transitive.is_empty()).then_some(transitive),
        patched: None,
        optional: node.optional,
    }
}

/// Aliases this node treats as optional — its manifest's
/// `optionalDependencies` entries plus the names of peers marked
/// optional by `peerDependenciesMeta`. Mirrors upstream's `partition`
/// over `(child) => depNode.optionalDependencies.has(child.alias) ||
/// depNode.peerDependencies[child.alias]?.optional === true` in
/// [`updateLockfile`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/src/updateLockfile.ts#L28-L31).
fn optional_children_of(node: &DependenciesGraphNode) -> HashSet<String> {
    let mut out: HashSet<String> = HashSet::new();
    if let Some(manifest) = node.resolve_result.manifest.as_ref()
        && let Some(map) = manifest.get("optionalDependencies").and_then(Value::as_object)
    {
        for name in map.keys() {
            out.insert(name.clone());
        }
    }
    for (name, peer) in &node.peer_dependencies {
        if peer.optional {
            out.insert(name.clone());
        }
    }
    out
}

/// Build the `<alias>: <ref>` value the snapshot writes per child edge.
/// `link:` snapshots aren't produced here — workspace siblings live
/// outside the dep graph today. The plain / alias discrimination
/// mirrors importer-side [`importer_dep_version`].
fn snapshot_dep_ref(
    alias: &str,
    child_dep_path: &DepPath,
    graph: &DependenciesGraph,
) -> Option<SnapshotDepRef> {
    let dep_path_str = child_dep_path.as_str();
    let real_name = graph.get(child_dep_path).and_then(|n| real_name(&n.resolve_result));
    if let Some(real) = real_name.as_deref() {
        let prefix = format!("{real}@");
        if alias == real
            && let Some(ver) = dep_path_str.strip_prefix(&prefix)
            && let Ok(parsed) = ver.parse::<PkgVerPeer>()
        {
            return Some(SnapshotDepRef::Plain(parsed));
        }
    }
    dep_path_str.parse::<PkgNameVerPeer>().ok().map(SnapshotDepRef::Alias)
}

#[cfg(test)]
mod tests;
