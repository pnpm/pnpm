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
    HoisterResult,
    PkgNameVerPeer,
    Rc,
    tree::peers::BoundPeersByKey,
};
use rustc_hash::FxHashMap;
use std::cell::OnceCell;

/// Every `(snapshot, peer)` whose bound provider the layout does not
/// reach: walking up from a directory that serves the snapshot finds no
/// directory of that name, or one holding another package version.
/// `bound_peers` holds the peers each snapshot of the hoisted tree binds.
pub(crate) fn misresolved_peers(
    result: &HoisterResult,
    bound_peers: &BoundPeersByKey,
) -> Vec<(PkgNameVerPeer, String)> {
    if bound_peers.values().all(|bound| bound.is_empty()) {
        return Vec::new();
    }
    let mut check =
        Check { bound_peers, provider_ids: FxHashMap::default(), misresolved: Vec::new() };
    let mut ancestors = vec![Level::of(Rc::new(result.clone()))];
    for child in children(result) {
        check.walk(&child, &mut ancestors);
    }
    check.misresolved
}

/// The peers one snapshot binds: the peer name, and the package id of its
/// provider.
type BoundPeerIds = Rc<Vec<(String, String)>>;

struct Check<'check> {
    bound_peers: &'check BoundPeersByKey,
    /// [`BoundPeerIds`] by snapshot key, computed once per key. `None` for a
    /// key that binds no peer.
    provider_ids: FxHashMap<String, Option<BoundPeerIds>>,
    misresolved: Vec<(PkgNameVerPeer, String)>,
}

/// A directory on the path being checked.
struct Level {
    dir: Rc<HoisterResult>,
    /// Its children by name, built when one is first looked up.
    by_name: OnceCell<FxHashMap<String, Rc<HoisterResult>>>,
}

impl Level {
    fn of(dir: Rc<HoisterResult>) -> Self {
        Level { dir, by_name: OnceCell::new() }
    }

    fn child(&self, name: &str) -> Option<&Rc<HoisterResult>> {
        let by_name = self.by_name.get_or_init(|| {
            let mut by_name = FxHashMap::default();
            for dep in self.dir.dependencies.borrow().iter() {
                by_name.entry(dep.0.name.clone()).or_insert_with(|| Rc::clone(&dep.0));
            }
            by_name
        });
        by_name.get(name)
    }
}

impl Check<'_> {
    fn walk(&mut self, dir: &Rc<HoisterResult>, ancestors: &mut Vec<Level>) {
        let level = Level::of(Rc::clone(dir));
        let references: Vec<String> = dir.references
            .borrow()
            .iter()
            .cloned()
            .collect();
        for reference in &references {
            self.check_reference(reference, &level, ancestors);
        }
        ancestors.push(level);
        for child in children(dir) {
            self.walk(&child, ancestors);
        }
        ancestors.pop();
    }

    /// Record each peer of `reference` that does not resolve from `level`.
    fn check_reference(&mut self, reference: &str, level: &Level, ancestors: &[Level]) {
        let Some(peers) = self.provider_ids(reference) else { return };
        let missed = peers
            .iter()
            .filter(|(peer, package_id)| !reaches(level, ancestors, peer, package_id));
        for (peer, _) in missed {
            if let Ok(key) = reference.parse::<PkgNameVerPeer>() {
                self.misresolved.push((key, peer.clone()));
            }
        }
    }

    fn provider_ids(&mut self, reference: &str) -> Option<BoundPeerIds> {
        if let Some(known) = self.provider_ids.get(reference) {
            return known.clone();
        }
        let ids = self.bound_peers
            .get(reference)
            .filter(|bound| !bound.is_empty())
            .map(|bound| {
                let ids = bound
                    .iter()
                    .map(|(peer, provider)| (peer.clone(), crate::pkg_id(provider)))
                    .collect();
                Rc::new(ids)
            });
        self.provider_ids.insert(reference.to_string(), ids.clone());
        ids
    }
}

fn children(dir: &HoisterResult) -> Vec<Rc<HoisterResult>> {
    dir.dependencies
        .borrow()
        .iter()
        .map(|dep| Rc::clone(&dep.0))
        .collect()
}

/// Whether `name` resolves from `level` to a directory holding the package
/// `package_id`: the directory's own `node_modules`, then each ancestor's,
/// nearest first.
fn reaches(level: &Level, ancestors: &[Level], name: &str, package_id: &str) -> bool {
    let found = std::iter::once(level)
        .chain(ancestors.iter().rev())
        .find_map(|level| level.child(name));
    found.is_some_and(|found| {
        found.references
            .borrow()
            .iter()
            .map(|reference| reference.parse::<PkgNameVerPeer>())
            .filter_map(Result::ok)
            .any(|key| crate::pkg_id(&key) == package_id)
    })
}
