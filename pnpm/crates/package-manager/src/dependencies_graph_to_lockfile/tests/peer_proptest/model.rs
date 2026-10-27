//! The generated workspace a property runs on, and the conversions from
//! it to [`StubResolver`] tables and manifests, and from a resolution
//! back to a lockfile.

use std::collections::BTreeMap;

use pnpm_lockfile::Lockfile;
use pnpm_resolving_deps_resolver::ResolveWorkspaceResult;
use pnpm_resolving_resolver_base::{
    LatestQuery, ResolveFuture, ResolveLatestFuture, ResolveOptions, ResolveResult, Resolver,
    WantedDependency,
};
use proptest::{collection::vec, prelude::*};
use rustc_hash::FxHashMap as HashMap;

use super::super::{make_resolve_result as fake_result, single_importer_opts};

pub(super) struct StubResolver {
    table: HashMap<(String, String), ResolveResult>,
}

impl Resolver for StubResolver {
    fn resolve<'a>(
        &'a self,
        wanted: &'a WantedDependency,
        _opts: &'a ResolveOptions,
    ) -> ResolveFuture<'a> {
        let key = (
            wanted.alias.clone().unwrap_or_default(),
            wanted.bare_specifier.clone().unwrap_or_default(),
        );
        Box::pin(async move { Ok(self.table.get(&key).cloned()) })
    }

    fn resolve_latest<'a>(
        &'a self,
        _query: &'a LatestQuery,
        _opts: &'a ResolveOptions,
    ) -> ResolveLatestFuture<'a> {
        Box::pin(async { Ok(None) })
    }
}

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

#[derive(Debug, Clone, Copy)]
pub(super) struct Settings {
    pub(super) peers_suffix_max_length: usize,
    pub(super) auto_install_peers: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { peers_suffix_max_length: 1000, auto_install_peers: true }
    }
}

pub(super) fn settings() -> impl Strategy<Value = Settings> {
    (prop::sample::select(vec![1000usize, 20usize]), any::<bool>()).prop_map(
        |(peers_suffix_max_length, auto_install_peers)| Settings {
            peers_suffix_max_length,
            auto_install_peers,
        },
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

/// Fix each provider to one version so lockfile pins cannot select a different valid provider.
pub(super) fn fixed_provider_workspace() -> impl Strategy<Value = Workspace> {
    workspace()
        .prop_map(|mut workspace| {
            for target in 0..workspace.packages.len() {
                let major = MAJORS[target % MAJORS.len()];
                set_target_major(&mut workspace, target, major);
                for peer in workspace.packages
                    .iter_mut()
                    .flatten()
                    .flat_map(|version| &mut version.peers)
                    .filter(|peer| peer.target == target)
                {
                    peer.range = PeerRange::Major(major);
                }
            }
            workspace
        })
}

/// Pin the chosen dependency to one major in the initial workspace and flip it
/// to the other major across the moved workspace, keeping the dependency's own
/// manifest shape identical across both majors so only its version changes.
pub(super) fn move_dependency(
    workspace: &Workspace,
    importer: usize,
    edge: usize,
) -> (Workspace, Workspace) {
    let mut initial = workspace.clone();
    let importer_count = initial.importers.len();
    let (target, initial_major) = {
        let edges = &initial.importers[importer % importer_count];
        let picked = &edges[edge % edges.len()];
        (picked.target, picked.major)
    };
    initial.packages[target][1] = initial.packages[target][0].clone();
    set_target_major(&mut initial, target, initial_major);
    let mut moved = initial.clone();
    let moved_major = if initial_major == 1 { 2 } else { 1 };
    set_target_major(&mut moved, target, moved_major);
    (initial, moved)
}

fn set_target_major(workspace: &mut Workspace, target: usize, major: u8) {
    let importer_edges = workspace.importers.iter_mut().flatten();
    let package_deps = workspace.packages
        .iter_mut()
        .flatten()
        .flat_map(|version| &mut version.dependencies);
    for edge in importer_edges
        .chain(package_deps)
        .filter(|edge| edge.target == target)
    {
        edge.major = major;
    }
    for peer in workspace.packages
        .iter_mut()
        .flatten()
        .flat_map(|version| &mut version.peers)
        .filter(|peer| peer.target == target)
    {
        if let PeerRange::Major(peer_major) = &mut peer.range {
            *peer_major = major;
        }
    }
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
    StubResolver { table }
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

fn stringify_edges(
    edges: &BTreeMap<String, pnpm_resolving_deps_resolver::DepPath>,
) -> BTreeMap<String, String> {
    edges
        .iter()
        .map(|(alias, dep_path)| (alias.clone(), dep_path.to_string()))
        .collect()
}

/// Serialize the resolved graph through the production lockfile writer.
pub(super) fn lockfile_of(result: &ResolveWorkspaceResult, workspace: &Workspace) -> Lockfile {
    lockfile_with_settings(result, workspace, Settings::default())
}

pub(super) fn lockfile_with_settings(
    result: &ResolveWorkspaceResult,
    workspace: &Workspace,
    settings: Settings,
) -> Lockfile {
    let manifests = super::manifests(workspace);
    let mut options = single_importer_opts(
        &manifests[0].1,
        &result.peers.graph,
        BTreeMap::new(),
        settings.auto_install_peers,
        false,
        None,
        None,
    );
    options.settings.peers_suffix_max_length =
        Some(settings.peers_suffix_max_length.try_into().unwrap());
    options.importers = IMPORTER_IDS
        .iter()
        .zip(&manifests)
        .map(|(id, (_, manifest))| {
            (
                (*id).to_string(),
                crate::ImporterLockfileInput {
                    manifest,
                    direct_dependencies_by_alias: result.peers.direct_dependencies_by_importer[*id]
                        .clone(),
                },
            )
        })
        .collect();
    let lockfile = crate::dependencies_graph_to_lockfile(options).expect("write generated graph");
    let yaml = lockfile.to_yaml_string().expect("serialize generated lockfile");
    Lockfile::parse(&yaml, std::path::Path::new(Lockfile::FILE_NAME))
        .expect("parse generated lockfile")
        .expect("generated lockfile is not empty")
}
