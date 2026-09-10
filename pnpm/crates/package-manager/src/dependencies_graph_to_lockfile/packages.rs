use super::{
    DependenciesGraphToLockfileError,
    importers::{real_name, self_aliased_file_ver},
    optional_children_of,
};
use pnpm_lockfile::{
    BundledDependencies, LockfileFormError, LockfileFormOptions, LockfileResolution, PackageKey,
    PackageMetadata, PeerDependencyMeta, PkgName, PkgNameVerPeer, PkgVerPeer, RegistryOptions,
    SnapshotDepRef, SnapshotEntry, registry_server_type,
};
use pnpm_resolving_deps_resolver::{DepPath, DependenciesGraph, DependenciesGraphNode};
use rayon::prelude::*;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap};

pub(super) type PackagesAndSnapshots =
    (HashMap<PackageKey, PackageMetadata>, HashMap<PackageKey, SnapshotEntry>);
/// Walk the depPath-keyed [`DependenciesGraph`] and emit the matching
/// `(PackageMetadata, SnapshotEntry)` pair for each node — fanned out
/// across the two top-level maps the v9 lockfile splits.
///
/// Multiple snapshot entries (peer variants) share one packages entry,
/// so the loop dedupes by peer-stripped key.
///
/// `optional_overrides` carries the corrected `optional` flag per
/// depPath produced by [`compute_corrected_optional`](crate::dependencies_graph_to_lockfile::compute_corrected_optional); a missing
/// entry falls back to [`DependenciesGraphNode::optional`].
/// What [`build_packages_and_snapshots`] renders `packages:` entries
/// from, beyond the graph itself.
pub(super) struct PackageMetadataSources<'a> {
    pub(super) registry: &'a str,
    pub(super) registries_by_prefix: &'a HashMap<String, String>,
    pub(super) registry_options_by_url: &'a BTreeMap<String, RegistryOptions>,
    pub(super) lockfile_include_tarball_url: bool,
    pub(super) previous_packages: Option<&'a HashMap<PackageKey, PackageMetadata>>,
}
pub(super) fn build_packages_and_snapshots(
    graph: &DependenciesGraph,
    optional_overrides: &HashMap<DepPath, bool>,
    sources: &PackageMetadataSources<'_>,
) -> Result<PackagesAndSnapshots, DependenciesGraphToLockfileError> {
    // Every node's snapshot and metadata read only that node plus the
    // shared inputs, so the nodes fan out across the rayon pool; the
    // serial fold below walks them in the graph's iteration order, so
    // the first-of-a-key metadata insert and the first error stay the
    // ones the serial loop would have kept.
    let built: Vec<Result<Option<BuiltNode<'_>>, DependenciesGraphToLockfileError>> = graph
        .values()
        .collect::<Vec<_>>()
        .into_par_iter()
        .map(|node| build_node(node, graph, optional_overrides))
        .collect();

    let mut packages: HashMap<PackageKey, PackageMetadata> = HashMap::new();
    let mut snapshots: HashMap<PackageKey, SnapshotEntry> = HashMap::new();
    for built_node in built {
        let Some(BuiltNode { node, snapshot_key, snapshot }) = built_node? else { continue };
        insert_package_metadata(&mut packages, node, snapshot_key.without_peer(), sources)?;
        snapshots.insert(snapshot_key, snapshot);
    }

    Ok((packages, snapshots))
}
pub(super) struct BuiltNode<'graph> {
    node: &'graph pnpm_resolving_deps_resolver::DependenciesGraphNode,
    snapshot_key: PackageKey,
    snapshot: SnapshotEntry,
}
/// A node's snapshot row under its key; `None` for a workspace link, which
/// has no row of its own.
pub(super) fn build_node<'graph>(
    node: &'graph DependenciesGraphNode,
    graph: &DependenciesGraph,
    optional_overrides: &HashMap<DepPath, bool>,
) -> Result<Option<BuiltNode<'graph>>, DependenciesGraphToLockfileError> {
    let dep_path = node.dep_path.as_str();
    let snapshot_key = match dep_path.parse::<PackageKey>() {
        Ok(snapshot_key) => Ok(snapshot_key),
        // A workspace link is the one node with no row of its own —
        // it resolves as its own importer and pnpm writes it none
        // either.
        Err(_) if dep_path.starts_with("link:") => return Ok(None),
        Err(source) => Err(DependenciesGraphToLockfileError::UnkeyedDepPath {
            dep_path: dep_path.to_string(),
            source: Box::new(source),
        }),
    }?;
    let snapshot = build_snapshot_entry(node, graph, optional_overrides);
    Ok(Some(BuiltNode { node, snapshot_key, snapshot }))
}
/// Record a package's metadata under its peer-stripped key, once: the first
/// node of a key in graph order wins.
pub(super) fn insert_package_metadata(
    packages: &mut HashMap<PackageKey, PackageMetadata>,
    node: &DependenciesGraphNode,
    metadata_key: PackageKey,
    sources: &PackageMetadataSources<'_>,
) -> Result<(), DependenciesGraphToLockfileError> {
    let std::collections::hash_map::Entry::Vacant(entry) = packages.entry(metadata_key) else {
        return Ok(());
    };
    let (registry, include_tarball_url) = metadata_registry(entry.key(), sources);
    let mut metadata = build_package_metadata(
        node,
        entry.key(),
        LockfileFormOptions {
            registry,
            server_type: registry_server_type(sources.registry_options_by_url, registry),
            include_tarball_url,
        },
    )
    .map_err(DependenciesGraphToLockfileError::LockfileForm)?;
    carry_previous_deprecation(&mut metadata, entry.key(), sources);
    entry.insert(metadata);
    Ok(())
}
/// The registry a package's metadata is written against, and whether its
/// tarball URL has to be written out.
///
/// A registry-qualified key names its registry; that registry — not the
/// scope-routed default — decides whether the tarball URL is canonical and can
/// be dropped from the entry.
///
/// Fails closed on an alias that cannot be resolved: testing the URL for
/// canonicality against the *default* registry could drop a URL that only the
/// named registry can rebuild, leaving a `work:` entry that no install can
/// fetch. Keeping the URL is always recoverable, so an unknown alias forces it
/// to be written.
pub(super) fn metadata_registry<'a>(
    key: &PackageKey,
    sources: &PackageMetadataSources<'a>,
) -> (&'a str, bool) {
    let Some((registry_name, _)) = key.suffix.registry_qualified() else {
        return (sources.registry, sources.lockfile_include_tarball_url);
    };
    match sources.registries_by_prefix.get(registry_name) {
        Some(named_registry) => (named_registry.as_str(), sources.lockfile_include_tarball_url),
        None => (sources.registry, true),
    }
}
/// `deprecated` is the only registry-mutable field of a published version; an
/// unchanged resolution must not lose a recorded deprecation to a registry
/// serving it inconsistently (pnpm/pnpm#13846).
pub(super) fn carry_previous_deprecation(
    metadata: &mut PackageMetadata,
    key: &PackageKey,
    sources: &PackageMetadataSources<'_>,
) {
    if metadata.deprecated.is_none()
        && let Some(previous) = sources.previous_packages.and_then(|prev| prev.get(key))
        && previous.resolution == metadata.resolution
    {
        metadata.deprecated.clone_from(&previous.deprecated);
    }
}
/// Build the per-`(name, version)` [`PackageMetadata`] block for the
/// lockfile's `packages:` map. Pulls `engines` / `cpu` / `os` / `libc` /
/// `deprecated` / `hasBin` / `bundledDependencies` / `peerDependencies`
/// off the resolver's manifest fragment when present.
///
/// Covers the per-package half only — the per-snapshot fields
/// `dependencies` / `optionalDependencies` / `transitivePeerDependencies` /
/// `optional` / `patched` go on the snapshot below.
pub(super) fn build_package_metadata(
    node: &DependenciesGraphNode,
    metadata_key: &PackageKey,
    lockfile_form: LockfileFormOptions<'_>,
) -> Result<PackageMetadata, LockfileFormError> {
    let manifest = node.resolve_result.manifest.as_deref();
    let (peer_dependencies, peer_dependencies_meta) = build_peer_dep_blocks(node);
    let resolution_version = match metadata_key.suffix.registry_qualified() {
        Some((_, version)) => version.to_string(),
        None => metadata_key.suffix.version().to_string(),
    };
    let resolution = node.resolve_result.resolution.to_lockfile_form(
        &metadata_key.name.to_string(),
        &resolution_version,
        lockfile_form,
    )?;
    Ok(PackageMetadata {
        version: explicit_version(node, metadata_key, &resolution, manifest),
        resolution,
        engines: read_engines(manifest),
        cpu: read_string_list(manifest, "cpu"),
        os: read_string_list(manifest, "os"),
        libc: read_string_or_list(manifest, "libc"),
        deprecated: manifest
            .and_then(|manifest| manifest.get("deprecated"))
            .and_then(Value::as_str)
            .filter(|deprecated| !deprecated.is_empty())
            .map(ToString::to_string),
        has_bin: manifest_has_bin(manifest),
        prepare: None,
        bundled_dependencies: BundledDependencies::from_manifest(manifest),
        peer_dependencies,
        peer_dependencies_meta,
    })
}
/// The manifest's `engines`, without the ranges that constrain nothing.
pub(super) fn read_engines(manifest: Option<&Value>) -> Option<HashMap<String, String>> {
    manifest
        .and_then(|manifest| manifest.get("engines"))
        .and_then(|value| match value {
            Value::Object(map) => Some(
                map.iter()
                    .filter_map(|(name, value)| Some((name.clone(), value.as_str()?)))
                    .collect::<Vec<(String, &str)>>(),
            ),
            // Array-form `engines` (e.g. `["node >= 0.2.0"]`) records
            // index-keyed entries.
            Value::Array(items) => Some(
                items
                    .iter()
                    .enumerate()
                    .filter_map(|(index, value)| Some((index.to_string(), value.as_str()?)))
                    .collect(),
            ),
            _ => None,
        })
        .map(|entries| {
            entries
                .into_iter()
                .filter(|(_, range)| *range != "*")
                .map(|(name, range)| (name, range.to_string()))
                .collect::<HashMap<String, String>>()
        })
        .filter(|map| !map.is_empty())
}
/// Record `version` only for non-registry packages (depPath carries
/// a `:`), and only when the manifest declares one and the resolution
/// isn't a local directory. Registry packages omit it because their
/// version is already the depPath suffix.
/// A registry-qualified dep path carries a parseable semver of its own,
/// so the explicit version field written for other `:`-containing dep
/// paths would be redundant.
pub(super) fn explicit_version(
    node: &DependenciesGraphNode,
    metadata_key: &PackageKey,
    resolution: &LockfileResolution,
    manifest: Option<&Value>,
) -> Option<String> {
    (node.dep_path.as_str().contains(':')
        && metadata_key.suffix.registry_qualified().is_none()
        && !matches!(resolution, LockfileResolution::Directory(_)))
    .then(|| {
        manifest
            .and_then(|manifest| manifest.get("version"))
            .and_then(Value::as_str)
            .map(ToString::to_string)
    })
    .flatten()
}
/// Read a JSON array field off the resolver's manifest fragment and flatten it
/// into a `Vec<String>`. `None` when the field is missing or has no string
/// values — malformed metadata is silently dropped.
pub(super) fn read_string_list(manifest: Option<&Value>, key: &str) -> Option<Vec<String>> {
    match manifest?.get(key)? {
        Value::Array(items) => {
            let out: Vec<String> =
                items.iter().filter_map(Value::as_str).map(ToString::to_string).collect();
            (!out.is_empty()).then_some(out)
        }
        _ => None,
    }
}
pub(super) fn read_string_or_list(
    manifest: Option<&Value>,
    key: &str,
) -> Option<pnpm_lockfile::StringOrList> {
    match manifest?.get(key)? {
        Value::String(value) if !value.is_empty() => {
            Some(pnpm_lockfile::StringOrList::String(value.clone()))
        }
        Value::Array(_) => read_string_list(manifest, key).map(pnpm_lockfile::StringOrList::List),
        _ => None,
    }
}
/// `Some(true)` when the manifest declares executable files, recorded as
/// the `hasBin: true` signal; the field is dropped entirely when absent.
pub(crate) fn manifest_has_bin(manifest: Option<&Value>) -> Option<bool> {
    let manifest = manifest?;
    let has_bin = manifest.get("bin").is_some_and(|value| match value {
        Value::String(s) => !s.is_empty(),
        Value::Object(map) => !map.is_empty(),
        _ => false,
    });
    let has_bin_directory = manifest
        .get("directories")
        .and_then(Value::as_object)
        .and_then(|directories| directories.get("bin"))
        .is_some_and(|value| value.as_str().is_some_and(|path| !path.is_empty()));
    (has_bin || has_bin_directory).then_some(true)
}
/// Returned `Option`-pair from [`build_peer_dep_blocks`]: the
/// `peerDependencies` map (name → range) and the
/// `peerDependenciesMeta` map (name → `{ optional: true }`).
pub(super) type PeerDepBlocks =
    (Option<HashMap<String, String>>, Option<HashMap<String, PeerDependencyMeta>>);
/// Split the resolver's `peer_dependencies` into the
/// `peerDependencies` (name → range) and `peerDependenciesMeta`
/// (name → `{ optional: true }`) blocks written onto `packages:`.
pub(super) fn build_peer_dep_blocks(node: &DependenciesGraphNode) -> PeerDepBlocks {
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
/// Build the per-snapshot [`SnapshotEntry`] for this depPath: the
/// `dependencies` / `optionalDependencies` partition follows the
/// node's own `optionalDependencies` set and peer-optional flag;
/// `transitivePeerDependencies` is sorted; `optional` is sourced from
/// [`compute_corrected_optional`](crate::dependencies_graph_to_lockfile::compute_corrected_optional), which re-derives the flag from the
/// importer graph because the resolver's per-node fold misses
/// transitive descendants on revisits — see
/// <https://github.com/pnpm/pnpm/issues/11916>.
/// `BuildModules` consults this flag to decide whether a build
/// failure is fatal or should be reported via
/// `pnpm:skipped-optional-dependency`.
pub(super) fn build_snapshot_entry(
    node: &DependenciesGraphNode,
    graph: &DependenciesGraph,
    optional_overrides: &HashMap<DepPath, bool>,
) -> SnapshotEntry {
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

    let optional = optional_overrides.get(&node.dep_path).copied().unwrap_or(node.optional);

    SnapshotEntry {
        id: None,
        dependencies: (!dependencies.is_empty()).then_some(dependencies),
        optional_dependencies: (!optional_dependencies.is_empty()).then_some(optional_dependencies),
        transitive_peer_dependencies: (!transitive.is_empty()).then_some(transitive),
        patched: None,
        optional,
    }
}
/// Build the `<alias>: <ref>` value the snapshot writes per child edge.
/// Mirrors importer-side [`importer_dep_version`](crate::dependencies_graph_to_lockfile::importers::importer_dep_version): the `link:` branch
/// emits [`SnapshotDepRef::Link`] for workspace siblings, plain /
/// alias otherwise.
pub(super) fn snapshot_dep_ref(
    alias: &str,
    child_dep_path: &DepPath,
    graph: &DependenciesGraph,
) -> Option<SnapshotDepRef> {
    let dep_path_str = child_dep_path.as_str();
    if let Some(target) = dep_path_str.strip_prefix("link:") {
        return Some(SnapshotDepRef::Link(target.to_string()));
    }
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
    let key = dep_path_str.parse::<PkgNameVerPeer>().ok()?;
    if let Some(ver) = self_aliased_file_ver(alias, &key) {
        return Some(SnapshotDepRef::Plain(ver.clone()));
    }
    Some(SnapshotDepRef::Alias(key))
}
