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
    collections::BTreeSet,
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
    let identity = regular.map(|regular| identity_with_providers(dep_key, snapshot, regular));
    PeerView { names, identity }
}

/// The package id followed by each of `regular`'s peers and the provider
/// it is bound to.
fn identity_with_providers(
    dep_key: &PkgNameVerPeer,
    snapshot: &SnapshotEntry,
    regular: &BTreeSet<String>,
) -> String {
    let mut identity = crate::pkg_id(dep_key);
    for peer in regular {
        let provider = provider_of(dep_key, snapshot, peer).map(|key| key.to_string());
        write!(identity, "({peer}={})", provider.unwrap_or_default()).expect(
            "writing to a String cannot fail",
        );
    }
    identity
}

/// The key `peer` is bound to: a dependency of that name, else the
/// provider the key's peer suffix records.
pub(crate) fn provider_of(
    key: &PkgNameVerPeer,
    snapshot: &SnapshotEntry,
    peer: &str,
) -> Option<PkgNameVerPeer> {
    let own = [&snapshot.dependencies, &snapshot.optional_dependencies]
        .into_iter()
        .flatten()
        .flatten()
        .find(|(alias, _)| alias.to_string() == peer)
        .and_then(|(alias, dep_ref)| dep_ref.resolve(alias));
    own.or_else(|| {
        PeerBindings::from_key(key)?
            .providers()
            .find(|provider| provider.name.to_string() == peer)
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
        .filter(|provider| {
            let name = provider.name.to_string();
            peers.contains(&name) && !has_edge(&name)
        })
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
    let bound: std::collections::HashSet<PkgNameVerPeer> = bindings
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
    let alias = PkgName::parse(name).ok()?;
    [&snapshot.dependencies, &snapshot.optional_dependencies]
        .into_iter()
        .flatten()
        .find_map(|deps| deps.get(&alias))
        .and_then(|dep_ref| dep_ref.resolve(&alias))
}
