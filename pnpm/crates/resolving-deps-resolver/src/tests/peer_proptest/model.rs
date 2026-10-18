//! The generated workspace a property runs on, and the conversions from
//! it to [`StubResolver`] tables and manifests, and from a resolution
//! back to a lockfile.

use std::{collections::BTreeMap, str::FromStr};

use pnpm_lockfile::{
    ComVer, ImporterDepVersion, Lockfile, LockfileResolution, LockfileVersion, PackageKey,
    PackageMetadata, PkgName, PkgVerPeer, ProjectSnapshot, RegistryResolution,
    ResolvedDependencySpec, SnapshotDepRef, SnapshotEntry,
};
use proptest::{collection::vec, prelude::*};
use rustc_hash::FxHashMap as HashMap;

use super::super::{StubResolver, fake_result};
use crate::{DependenciesGraphNode, ResolveWorkspaceResult, lockfile_peer_blocks};

/// Every generated package is published at exactly these versions.
pub(super) const MAJORS: [u8; 2] = [1, 2];

pub(super) const IMPORTER_IDS: [&str; 3] = [".", "packages/a", "packages/b"];

#[derive(Debug, Clone)]
pub(super) struct Edge {
    pub(super) target: usize,
    pub(super) major: u8,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum PeerRange {
    Major(u8),
    Any,
}

impl PeerRange {
    fn render(self) -> String {
        match self {
            PeerRange::Major(major) => format!("^{major}.0.0"),
            PeerRange::Any => "*".to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub(super) struct Peer {
    pub(super) target: usize,
    pub(super) range: PeerRange,
    pub(super) optional: bool,
    /// Also list the peer under `dependencies`, as packages do to
    /// bring a default provider along.
    pub(super) also_dependency: bool,
}

#[derive(Debug, Clone)]
pub(super) struct PackageVersion {
    pub(super) dependencies: Vec<Edge>,
    pub(super) peers: Vec<Peer>,
}

#[derive(Debug, Clone)]
pub(super) struct Workspace {
    /// `packages[i][j]` is `p{i}` at version `MAJORS[j]`.
    pub(super) packages: Vec<Vec<PackageVersion>>,
    pub(super) importers: Vec<Vec<Edge>>,
}

pub(super) fn package_name(index: usize) -> String {
    format!("p{index}")
}

fn edge(package_count: usize) -> impl Strategy<Value = Edge> {
    (0..package_count, prop::sample::select(MAJORS.to_vec())).prop_map(|(target, major)| Edge {
        target,
        major,
    })
}

fn peer(package_count: usize) -> impl Strategy<Value = Peer> {
    let major = prop::sample::select(MAJORS.to_vec()).prop_map(PeerRange::Major);
    let unbounded = Just(PeerRange::Any);
    let range = prop_oneof![major, unbounded];
    (0..package_count, range, any::<bool>(), prop::bool::weighted(0.3)).prop_map(
        |(target, range, optional, also_dependency)| Peer {
            target,
            range,
            optional,
            also_dependency,
        },
    )
}

fn package_version(package_count: usize) -> impl Strategy<Value = PackageVersion> {
    (vec(edge(package_count), 0..=2), vec(peer(package_count), 0..=2)).prop_map(
        |(dependencies, peers)| PackageVersion { dependencies, peers },
    )
}

/// Two to five packages, each at both [`MAJORS`], depended on by one to
/// three importers. Self edges are dropped when the manifests are built,
/// so any target is fair game here and peer cycles arise naturally.
pub(super) fn workspace() -> impl Strategy<Value = Workspace> {
    (2usize..=5).prop_flat_map(|package_count| {
        let versions = vec(package_version(package_count), MAJORS.len());
        (vec(versions, package_count), vec(vec(edge(package_count), 1..=3), 1..=IMPORTER_IDS.len()))
            .prop_map(|(packages, importers)| Workspace { packages, importers })
    })
}

fn caret(major: u8) -> String {
    format!("^{major}.0.0")
}

fn exact(major: u8) -> String {
    format!("{major}.0.0")
}

/// `dependencies` and `peerDependencies` (with `peerDependenciesMeta`)
/// of `p{index}` at `version`.
fn package_manifest(index: usize, major: u8, version: &PackageVersion) -> serde_json::Value {
    let mut dependencies = serde_json::Map::new();
    let mut peers = serde_json::Map::new();
    let mut peers_meta = serde_json::Map::new();
    for dependency in version.dependencies
        .iter()
        .filter(|dep| dep.target != index)
    {
        dependencies.insert(package_name(dependency.target), caret(dependency.major).into());
    }
    for peer in version.peers
        .iter()
        .filter(|peer| peer.target != index)
    {
        let name = package_name(peer.target);
        peers.insert(name.clone(), peer.range.render().into());
        if peer.optional {
            peers_meta.insert(name.clone(), serde_json::json!({ "optional": true }));
        }
        if peer.also_dependency {
            let fallback = match peer.range {
                PeerRange::Major(major) => major,
                PeerRange::Any => MAJORS[MAJORS.len() - 1],
            };
            dependencies.insert(name, caret(fallback).into());
        }
    }
    let name = package_name(index);
    serde_json::json!({
        "name": name,
        "version": exact(major),
        "dependencies": dependencies,
        "peerDependencies": peers,
        "peerDependenciesMeta": peers_meta,
    })
}

/// The form a caret range takes once auto-installed peers merge it with
/// the other ranges that want the same peer.
fn desugared_caret(major: u8) -> String {
    format!(">={major}.0.0 <{}.0.0-0", major + 1)
}

/// Each package answers its caret range in both spellings, its exact
/// version, and `*` (the highest version).
pub(super) fn stub_resolver(workspace: &Workspace) -> StubResolver {
    let mut table = HashMap::default();
    for (index, versions) in workspace.packages.iter().enumerate() {
        let name = package_name(index);
        for (major, version) in MAJORS.into_iter().zip(versions) {
            let result = fake_result(&name, &exact(major), package_manifest(index, major, version));
            table.insert((name.clone(), caret(major)), result.clone());
            table.insert((name.clone(), exact(major)), result.clone());
            table.insert((name.clone(), desugared_caret(major)), result.clone());
            if major == MAJORS[MAJORS.len() - 1] {
                table.insert((name.clone(), "*".to_string()), result);
            }
        }
    }
    StubResolver { table, calls: std::sync::Mutex::new(Vec::new()) }
}

/// The `dependencies` of one importer, by alias.
pub(super) fn importer_dependencies(edges: &[Edge]) -> BTreeMap<String, String> {
    edges
        .iter()
        .map(|edge| (package_name(edge.target), caret(edge.major)))
        .collect()
}

/// What a resolution decided, in a form two resolutions compare by.
/// `direct` keeps only the aliases the importer manifests declare,
/// which are the ones the lockfile's `importers:` records.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Outcome {
    pub(super) direct: BTreeMap<String, BTreeMap<String, String>>,
    pub(super) graph: BTreeMap<String, BTreeMap<String, String>>,
}

pub(super) fn outcome_of(result: &ResolveWorkspaceResult, workspace: &Workspace) -> Outcome {
    let direct = IMPORTER_IDS
        .iter()
        .zip(&workspace.importers)
        .map(|(id, edges)| {
            let resolved = stringify_edges(&result.peers.direct_dependencies_by_importer[*id]);
            let declared = importer_dependencies(edges);
            let kept = resolved
                .into_iter()
                .filter(|(alias, _)| declared.contains_key(alias))
                .collect();
            ((*id).to_string(), kept)
        })
        .collect();
    let graph = result.peers.graph
        .values()
        .map(|node| (node.dep_path.to_string(), stringify_edges(&node.edges.children)))
        .collect();
    Outcome { direct, graph }
}

fn stringify_edges(edges: &BTreeMap<String, crate::DepPath>) -> BTreeMap<String, String> {
    edges
        .iter()
        .map(|(alias, dep_path)| (alias.clone(), dep_path.to_string()))
        .collect()
}

/// The version part of `name@version(peers)`. Generated names carry no
/// scope, so the first `@` ends the name.
fn version_of(dep_path: &str) -> PkgVerPeer {
    let (_, version) = dep_path.split_once('@').expect("dep path has a version");
    PkgVerPeer::from_str(version).expect("parse dep path version")
}

/// The `pnpm-lock.yaml` a later install would read back: the importers'
/// manifest specifiers, and one `packages:`/`snapshots:` pair per graph
/// node.
pub(super) fn lockfile_of(result: &ResolveWorkspaceResult, workspace: &Workspace) -> Lockfile {
    let mut importers = std::collections::HashMap::new();
    for (id, edges) in IMPORTER_IDS.iter().zip(&workspace.importers) {
        let resolved = &result.peers.direct_dependencies_by_importer[*id];
        let dependencies = importer_dependencies(edges)
            .into_iter()
            .map(|(alias, specifier)| {
                let version = ImporterDepVersion::Regular(version_of(resolved[&alias].as_str()));
                (PkgName::parse(alias).unwrap(), ResolvedDependencySpec { specifier, version })
            })
            .collect();
        importers.insert(
            (*id).to_string(),
            ProjectSnapshot { dependencies: Some(dependencies), ..ProjectSnapshot::default() },
        );
    }
    let mut packages = std::collections::HashMap::new();
    let mut snapshots = std::collections::HashMap::new();
    for node in result.peers.graph.values() {
        let key = PackageKey::from_str(node.dep_path.as_str()).expect("parse dep path");
        packages.insert(key.without_peer(), package_metadata(node));
        snapshots.insert(key, snapshot_entry(node));
    }
    Lockfile {
        lockfile_version: LockfileVersion::<9>::try_from(ComVer::new(9, 0)).unwrap(),
        settings: None,
        catalogs: None,
        overrides: None,
        package_extensions_checksum: None,
        pnpmfile_checksum: None,
        ignored_optional_dependencies: None,
        patched_dependencies: None,
        importers,
        packages: Some(packages),
        snapshots: Some(snapshots),
        time: None,
        extra: pnpm_lockfile::LockfileExtra::default(),
    }
}

fn package_metadata(node: &DependenciesGraphNode) -> PackageMetadata {
    let (peer_dependencies, peer_dependencies_meta) =
        lockfile_peer_blocks(node.edges.peer_set.declared());
    PackageMetadata {
        resolution: LockfileResolution::Registry(RegistryResolution {
            integrity: "sha512-gf6ZldcfCDyNXPRiW3lQjEP1Z9rrUM/4Cn7BZbv3SdTA82zxWRP8OmLwvGR974uuENhGCFgFdN11z3n1Ofpprg=="
                .parse()
                .unwrap(),
            revision: None,
        }),
        version: None,
        engines: None,
        cpu: None,
        os: None,
        libc: None,
        deprecated: None,
        has_bin: None,
        prepare: None,
        bundled_dependencies: None,
        peer_dependencies,
        peer_dependencies_meta,
    }
}

fn snapshot_entry(node: &DependenciesGraphNode) -> SnapshotEntry {
    let mut dependencies = std::collections::HashMap::new();
    let mut optional_dependencies = std::collections::HashMap::new();
    for (alias, child) in &node.edges.children {
        let target = if node.edges.optional_children.contains(alias) {
            &mut optional_dependencies
        } else {
            &mut dependencies
        };
        target.insert(
            PkgName::parse(alias.as_str()).unwrap(),
            SnapshotDepRef::Plain(version_of(child.as_str())),
        );
    }
    let mut transitive: Vec<String> = node.edges.transitive_peer_dependencies
        .iter()
        .cloned()
        .collect();
    transitive.sort();
    SnapshotEntry {
        dependencies: (!dependencies.is_empty()).then_some(dependencies),
        optional_dependencies: (!optional_dependencies.is_empty()).then_some(optional_dependencies),
        transitive_peer_dependencies: (!transitive.is_empty()).then_some(transitive),
        ..SnapshotEntry::default()
    }
}
