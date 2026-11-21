//! Peers the hoisted layout resolves to the wrong package.
//!
//! The hoister drops peer edges and expects a package to find each peer by
//! walking up `node_modules` by name, which holds for Yarn's model of
//! peers. A pnpm lockfile can bind a peer to a provider no ancestor holds
//! under the peer's name: a dependency installed under an alias
//! (`"webpack-5": "npm:webpack@5"` satisfies a `webpack` peer) next to
//! another package that has the peer's name, or a provider reached through
//! a dependent whose key no longer records it. `tree::aliased` links an
//! aliased provider whose name is free before the hoist runs.
//! [`crate::hoist`] checks the layout here and handles each peer found
//! missing as a regular dependency of that one snapshot, which places the
//! provider where the package finds it.

use crate::{
    HoistOpts,
    HoisterResult,
    Lockfile,
    PkgNameVerPeer,
    Rc,
    tree::peers::{
        PeerView,
        bound_peers,
        peer_view,
    },
};
use std::collections::HashMap;

/// Every `(snapshot, peer)` whose bound provider the layout does not
/// reach, peers already held as regular dependencies included: walking up
/// from a directory that serves the snapshot finds no directory of that
/// name, or one holding another package version.
pub(crate) fn misresolved_peers(
    result: &HoisterResult,
    lockfile: &Lockfile,
    opts: &HoistOpts,
) -> Vec<(PkgNameVerPeer, String)> {
    let mut check = Check { lockfile, opts, bound: HashMap::new(), misresolved: Vec::new() };
    let mut ancestors = vec![Rc::new(result.clone())];
    for child in children(result) {
        check.walk(&child, &mut ancestors);
    }
    check.misresolved
}

/// The peers one snapshot binds: the peer name, and the package id of its
/// provider.
type BoundPeerIds = Rc<Vec<(String, String)>>;

struct Check<'check> {
    lockfile: &'check Lockfile,
    opts: &'check HoistOpts,
    /// [`BoundPeerIds`] by snapshot key, computed once per key.
    bound: HashMap<String, Option<(PkgNameVerPeer, BoundPeerIds)>>,
    misresolved: Vec<(PkgNameVerPeer, String)>,
}

impl Check<'_> {
    fn walk(&mut self, dir: &Rc<HoisterResult>, ancestors: &mut Vec<Rc<HoisterResult>>) {
        let references: Vec<String> = dir.references
            .borrow()
            .iter()
            .cloned()
            .collect();
        for reference in references {
            let Some((key, peers)) = self.bound_peers(&reference) else { continue };
            for (peer, package_id) in peers.iter() {
                if !reaches(dir, ancestors, peer, package_id) {
                    self.misresolved.push((key.clone(), peer.clone()));
                }
            }
        }
        ancestors.push(Rc::clone(dir));
        for child in children(dir) {
            self.walk(&child, ancestors);
        }
        ancestors.pop();
    }

    fn bound_peers(&mut self, reference: &str) -> Option<(PkgNameVerPeer, BoundPeerIds)> {
        if let Some(known) = self.bound.get(reference) {
            return known.clone();
        }
        let bound = reference
            .parse::<PkgNameVerPeer>()
            .ok()
            .and_then(|key| {
                let snapshot = self.lockfile.snapshots.as_ref()?.get(&key)?;
                let PeerView { names, .. } =
                    peer_view(&key, snapshot, self.lockfile, self.opts, None);
                let peers: Vec<(String, String)> = bound_peers(&key, snapshot, &names)
                    .into_iter()
                    .map(|(peer, provider)| (peer, crate::pkg_id(&provider)))
                    .collect();
                Some((key, Rc::new(peers)))
            });
        self.bound.insert(reference.to_string(), bound.clone());
        bound
    }
}

fn children(dir: &HoisterResult) -> Vec<Rc<HoisterResult>> {
    dir.dependencies
        .borrow()
        .iter()
        .map(|dep| Rc::clone(&dep.0))
        .collect()
}

/// Whether `name` resolves from `dir` to a directory holding the package
/// `package_id`: `dir`'s own `node_modules`, then each ancestor's, nearest
/// first.
fn reaches(
    dir: &Rc<HoisterResult>,
    ancestors: &[Rc<HoisterResult>],
    name: &str,
    package_id: &str,
) -> bool {
    let found = std::iter::once(dir)
        .chain(ancestors.iter().rev())
        .find_map(|parent| {
            parent.dependencies
                .borrow()
                .iter()
                .find(|dep| dep.0.name == name)
                .map(|dep| Rc::clone(&dep.0))
        });
    found.is_some_and(|found| {
        found.references
            .borrow()
            .iter()
            .map(|reference| reference.parse::<PkgNameVerPeer>())
            .filter_map(Result::ok)
            .any(|key| crate::pkg_id(&key) == package_id)
    })
}
