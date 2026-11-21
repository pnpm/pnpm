//! A package's peer names and the providers its key's suffix records.

use crate::{
    HoistOpts,
    IndexSet,
    Lockfile,
    PkgName,
    PkgNameVerPeer,
    SnapshotEntry,
};
use pnpm_lockfile::PeerBindings;
use std::{
    collections::{
        BTreeSet,
        HashMap,
        HashSet,
    },
    fmt::Write as _,
};

/// How the hoister sees a package's peers.
pub(crate) struct PeerView {
    /// [`base_peer_names`] without the peers it holds as regular
    /// dependencies (see [`crate::repair`]). Empty when
    /// `auto_install_peers` is on, so the hoister moves freely.
    pub(crate) names: IndexSet<String>,
    /// See [`crate::HoisterTree::identity`]. A peer provider held as a
    /// regular dependency is a child the package's other peer variants
    /// lack, so it only shares a directory with variants that hold the
    /// same providers.
    pub(crate) identity: Option<String>,
}

pub(crate) fn peer_view(
    dep_key: &PkgNameVerPeer,
    snapshot: &SnapshotEntry,
    lockfile: &Lockfile,
    opts: &HoistOpts,
    regular: Option<&BTreeSet<String>>,
) -> PeerView {
    if opts.auto_install_peers {
        return PeerView { names: IndexSet::new(), identity: None };
    }
    let mut names = base_peer_names(dep_key, snapshot, lockfile);
    let regular = regular.filter(|regular| !regular.is_empty());
    names.retain(|name| !regular.is_some_and(|regular| regular.contains(name)));
    let identity =
        regular.map(|regular| identity_with_providers(dep_key, snapshot, regular, lockfile));
    PeerView { names, identity }
}

/// The package id followed by each of `regular`'s peers and the provider
/// it is bound to.
fn identity_with_providers(
    dep_key: &PkgNameVerPeer,
    snapshot: &SnapshotEntry,
    regular: &BTreeSet<String>,
    lockfile: &Lockfile,
) -> String {
    let mut identity = crate::pkg_id(dep_key);
    for peer in regular {
        let provider = provider_of(dep_key, snapshot, peer, lockfile).map(|key| key.to_string());
        write!(identity, "({peer}={})", provider.unwrap_or_default()).expect(
            "writing to a String cannot fail",
        );
    }
    identity
}

/// The peers a snapshot binds: each peer name with the key of its
/// provider.
pub(crate) type BoundPeers = Vec<(String, PkgNameVerPeer)>;

/// The provider of each of `names` that `key` binds.
pub(crate) fn bound_peers(
    key: &PkgNameVerPeer,
    snapshot: &SnapshotEntry,
    names: &IndexSet<String>,
    lockfile: &Lockfile,
) -> BoundPeers {
    names
        .iter()
        .filter_map(|peer| Some((peer.clone(), provider_of(key, snapshot, peer, lockfile)?)))
        .collect()
}

/// The key `peer` is bound to: a dependency of that name, else the
/// provider the key's peer suffix records.
pub(crate) fn provider_of(
    key: &PkgNameVerPeer,
    snapshot: &SnapshotEntry,
    peer: &str,
    lockfile: &Lockfile,
) -> Option<PkgNameVerPeer> {
    own_dependency_key(snapshot, peer)
        .or_else(|| {
            let recorded = PeerBindings::from_key(key)?
                .providers()
                .find(|provider| provider.name.to_string() == peer)?;
            Some(snapshot_key_of(recorded, snapshot, peer, lockfile))
        })
}

/// The snapshot key of the provider that a peer suffix records as
/// `recorded`.
///
/// With `dedupePeers` the suffix leaves out the provider's own peers, and
/// no snapshot has that key. A dependency that takes the peer records the
/// full key: the nearest ones are asked first, and the smallest key wins
/// among several. `recorded` is returned when none does.
fn snapshot_key_of(
    recorded: PkgNameVerPeer,
    snapshot: &SnapshotEntry,
    peer: &str,
    lockfile: &Lockfile,
) -> PkgNameVerPeer {
    full_provider_key(&recorded, snapshot, peer, lockfile).unwrap_or(recorded)
}

fn full_provider_key(
    recorded: &PkgNameVerPeer,
    snapshot: &SnapshotEntry,
    peer: &str,
    lockfile: &Lockfile,
) -> Option<PkgNameVerPeer> {
    let snapshots = lockfile.snapshots.as_ref()?;
    if snapshots.contains_key(recorded) {
        return None;
    }
    let search = ProviderSearch {
        alias: PkgName::parse(peer).ok()?,
        peer,
        package: recorded.without_peer(),
        snapshots,
    };
    let mut seen: HashSet<PkgNameVerPeer> = HashSet::new();
    let mut level = vec![snapshot];
    while !level.is_empty() {
        let (bound, carriers) = search.ask(&level, &mut seen);
        if bound.is_some() {
            return bound;
        }
        level = carriers;
    }
    None
}

/// What [`full_provider_key`] looks for.
struct ProviderSearch<'search> {
    alias: PkgName,
    peer: &'search str,
    /// The provider's key without its peers.
    package: PkgNameVerPeer,
    snapshots: &'search HashMap<PkgNameVerPeer, SnapshotEntry>,
}

impl<'search> ProviderSearch<'search> {
    /// The smallest full key a dependency of `level` records, and the
    /// dependencies that take the peer transitively, in key order.
    fn ask(
        &self,
        level: &[&'search SnapshotEntry],
        seen: &mut HashSet<PkgNameVerPeer>,
    ) -> (Option<PkgNameVerPeer>, Vec<&'search SnapshotEntry>) {
        let mut bound: Vec<PkgNameVerPeer> = Vec::new();
        let mut carriers: Vec<(PkgNameVerPeer, &SnapshotEntry)> = Vec::new();
        let dependencies =
            level.iter().flat_map(|snapshot| dependencies_of(snapshot, self.snapshots));
        for (key, dependency) in dependencies {
            if let Some(full) = self.bound_in(dependency) {
                bound.push(full);
            } else if takes_transitively(dependency, self.peer) && seen.insert(key.clone()) {
                carriers.push((key, dependency));
            }
        }
        carriers.sort_by_cached_key(|(key, _)| key.to_string());
        let carriers = carriers
            .into_iter()
            .map(|(_, dependency)| dependency)
            .collect();
        (bound.into_iter().min_by_key(ToString::to_string), carriers)
    }

    fn bound_in(&self, dependency: &SnapshotEntry) -> Option<PkgNameVerPeer> {
        dependency_key(dependency, &self.alias).filter(|full| full.without_peer() == self.package)
    }
}

fn takes_transitively(snapshot: &SnapshotEntry, peer: &str) -> bool {
    snapshot.transitive_peer_dependencies
        .iter()
        .flatten()
        .any(|name| name == peer)
}

/// The snapshot's dependencies that have a snapshot of their own.
fn dependencies_of<'lockfile>(
    snapshot: &'lockfile SnapshotEntry,
    snapshots: &'lockfile HashMap<PkgNameVerPeer, SnapshotEntry>,
) -> impl Iterator<Item = (PkgNameVerPeer, &'lockfile SnapshotEntry)> {
    [&snapshot.dependencies, &snapshot.optional_dependencies]
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|(alias, dep_ref)| {
            let key = dep_ref.resolve(alias)?;
            let dependency = snapshots.get(&key)?;
            Some((key, dependency))
        })
}

/// The peer providers the key's suffix records that `has_edge` does not
/// cover: the node needs an edge to each, as a peer edge when the name is
/// still a peer and as a regular dependency otherwise. A hashed suffix
/// names no provider.
pub(super) fn unlinked_peer_providers(
    dep_key: &PkgNameVerPeer,
    snapshot: &SnapshotEntry,
    lockfile: &Lockfile,
    has_edge: impl Fn(&str) -> bool,
) -> Vec<PkgNameVerPeer> {
    let Some(bindings) = PeerBindings::from_key(dep_key) else { return Vec::new() };
    let peers = base_peer_names(dep_key, snapshot, lockfile);
    bindings
        .providers()
        .map(|provider| (provider.name.to_string(), provider))
        .filter(|(name, _)| peers.contains(name) && !has_edge(name))
        .map(|(name, provider)| snapshot_key_of(provider, snapshot, &name, lockfile))
        .collect()
}

/// The package's peer names, as pnpm v11's `resolvePeerNames` lists
/// them: `peerDependencies` (from the `packages:` map) then
/// `transitivePeerDependencies` (from the `snapshots:` map), without the
/// declared peers the package's own dependencies provide.
fn base_peer_names(
    dep_key: &PkgNameVerPeer,
    snapshot: &SnapshotEntry,
    lockfile: &Lockfile,
) -> IndexSet<String> {
    let declared = declared_peer_names(dep_key, lockfile);
    let own = own_dependencies_declared_as_peers(dep_key, snapshot, &declared);
    let transitive = snapshot.transitive_peer_dependencies
        .iter()
        .flatten()
        .cloned();
    declared
        .into_iter()
        .chain(transitive)
        .filter(|name| !own.contains(name))
        .collect()
}

/// The declared peer names, sorted as the lockfile writes them.
fn declared_peer_names(dep_key: &PkgNameVerPeer, lockfile: &Lockfile) -> Vec<String> {
    let meta = lockfile.packages
        .as_ref()
        .and_then(|packages| packages.get(&dep_key.without_peer()));
    let mut names: Vec<String> = meta
        .and_then(|meta| meta.peer_dependencies.as_ref())
        .into_iter()
        .flat_map(|peers| peers.keys())
        .cloned()
        .collect();
    names.sort();
    names
}

/// Port of pnpm v11's `getOwnDependenciesDeclaredAsPeers`: the declared
/// peers the package's own dependency provides. The lockfile records every
/// declared peer, but the key's peer suffix binds only the peers resolved
/// from outside the package. A hashed suffix names no provider, so every
/// declared peer stays a peer.
fn own_dependencies_declared_as_peers(
    dep_key: &PkgNameVerPeer,
    snapshot: &SnapshotEntry,
    declared: &[String],
) -> Vec<String> {
    let bindings = PeerBindings::from_key(dep_key);
    if declared.is_empty() || matches!(bindings, Some(PeerBindings::Hashed)) {
        return Vec::new();
    }
    let bound: HashSet<PkgNameVerPeer> = bindings
        .iter()
        .flat_map(PeerBindings::providers)
        .map(|key| key.without_peer())
        .collect();
    declared
        .iter()
        .filter(|name| {
            own_dependency_key(snapshot, name)
                .is_some_and(|target| !bound.contains(&target.without_peer()))
        })
        .cloned()
        .collect()
}

/// The snapshot key `name` resolves to among the package's own
/// dependencies, regular before optional.
fn own_dependency_key(snapshot: &SnapshotEntry, name: &str) -> Option<PkgNameVerPeer> {
    dependency_key(snapshot, &PkgName::parse(name).ok()?)
}

fn dependency_key(snapshot: &SnapshotEntry, alias: &PkgName) -> Option<PkgNameVerPeer> {
    [&snapshot.dependencies, &snapshot.optional_dependencies]
        .into_iter()
        .flatten()
        .find_map(|deps| deps.get(alias))
        .and_then(|dep_ref| dep_ref.resolve(alias))
}
