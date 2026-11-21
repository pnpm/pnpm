//! Peers bound to a package that is held under another name.
//!
//! A package finds a peer by walking up `node_modules` for the peer's
//! name. pnpm binds a peer by package, so the provider can be a dependency
//! installed under an alias: `"webpack-5": "npm:webpack@5"` satisfies a
//! `webpack` peer. The node that holds the alias gets a second edge to the
//! provider under the peer's name. The dependent then finds it by name,
//! and the provider's own peers resolve where the aliased copy resolves
//! them.

use super::peers::BoundPeers;
use crate::{
    HoisterDependencyKind,
    HoisterTree,
    IndexMap,
    IndexSet,
    PkgNameVerPeer,
    RcByPtr,
    pkg_id,
};
use std::{
    collections::{
        HashMap,
        HashSet,
    },
    rc::Rc,
};

/// The node whose children are being linked.
pub(super) struct Holder<'holder> {
    /// The name the node is installed under.
    pub(super) name: &'holder str,
    /// `None` for a project.
    pub(super) key: Option<&'holder PkgNameVerPeer>,
    pub(super) peer_names: &'holder IndexSet<String>,
}

/// The edges `holder` lacks: each peer name one of its `children` binds to
/// a package `holder` holds under another name, with the key it is held
/// by. A name `holder` already uses, for a child or for itself, is left
/// out, as is one it takes as a peer.
pub(super) fn aliased_peer_providers(
    holder: &Holder<'_>,
    children: &IndexSet<RcByPtr<HoisterTree>>,
    bound_peers: &HashMap<String, Rc<BoundPeers>>,
) -> Vec<(String, PkgNameVerPeer)> {
    let wanted = unheld_peers(holder, children, bound_peers);
    if wanted.is_empty() {
        return Vec::new();
    }
    let held = held_keys(holder, children);
    wanted
        .into_iter()
        .filter_map(|(peer, provider)| {
            let candidates = held.get(&pkg_id(provider))?;
            let key = candidates
                .iter()
                .find(|key| *key == provider)
                .or_else(|| candidates.first())?;
            Some((peer.clone(), key.clone()))
        })
        .collect()
}

/// The first provider bound to each peer name that walking up from a
/// regular child of `holder` does not find in `holder`.
fn unheld_peers<'peers>(
    holder: &Holder<'_>,
    children: &IndexSet<RcByPtr<HoisterTree>>,
    bound_peers: &'peers HashMap<String, Rc<BoundPeers>>,
) -> Vec<(&'peers String, &'peers PkgNameVerPeer)> {
    let bound: Vec<&(String, PkgNameVerPeer)> = regular_children(holder, children)
        .filter(|child| !child.peer_names.is_empty())
        .filter_map(|child| bound_peers.get(&child.reference))
        .flat_map(|bound| bound.iter())
        .collect();
    if bound.is_empty() {
        return Vec::new();
    }
    let child_names: HashSet<&str> = children
        .iter()
        .map(|child| child.0.name.as_str())
        .collect();
    let mut wanted: IndexMap<&String, &PkgNameVerPeer> = IndexMap::new();
    for (peer, provider) in bound {
        let is_held = holder.name == peer
            || holder.peer_names.contains(peer)
            || child_names.contains(peer.as_str());
        if !is_held {
            wanted.entry(peer).or_insert(provider);
        }
    }
    wanted.into_iter().collect()
}

/// The keys `holder` holds by package id: its own, then those of its
/// regular children in order.
fn held_keys(
    holder: &Holder<'_>,
    children: &IndexSet<RcByPtr<HoisterTree>>,
) -> HashMap<String, Vec<PkgNameVerPeer>> {
    let own = holder.key.cloned();
    let of_children = regular_children(holder, children)
        .map(|child| child.reference.parse::<PkgNameVerPeer>())
        .filter_map(Result::ok);
    let mut held: HashMap<String, Vec<PkgNameVerPeer>> = HashMap::new();
    for key in own.into_iter().chain(of_children) {
        held.entry(pkg_id(&key))
            .or_default()
            .push(key);
    }
    held
}

/// The packages `holder` installs in its own `node_modules`: a child it
/// takes as a peer is found above it.
fn regular_children<'children>(
    holder: &Holder<'_>,
    children: &'children IndexSet<RcByPtr<HoisterTree>>,
) -> impl Iterator<Item = &'children HoisterTree> {
    children
        .iter()
        .map(|child| &*child.0)
        .filter(|child| {
            child.dependency_kind == HoisterDependencyKind::Regular
                && !holder.peer_names.contains(&child.name)
        })
}
