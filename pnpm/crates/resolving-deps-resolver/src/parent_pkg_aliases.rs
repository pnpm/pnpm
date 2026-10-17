//! The dependency names visible in a resolving parent's scope.
//!
//! pnpm's `options.parentPkgAliases` (`resolveDependencies.ts`) is the
//! union of every alias resolved at the levels *above* the one being
//! resolved: the importer's wanted dependencies, then each level's
//! resolved children as the walk descends. A package whose
//! `peerDependencies` name is already in that scope drops the
//! same-named entry from its own `dependencies`, so the peer edge —
//! satisfied from an ancestor — supplies the package instead of a
//! nested copy.
//!
//! Upstream rebuilds the whole map per level (`{ ...parentPkgAliases,
//! ...currentParentPkgAliases }`). Here the levels are chained instead,
//! so extending a scope costs one level's aliases rather than a copy of
//! everything above it; only membership is ever asked of it.

use crate::resolved_tree::PeerDep;
use rustc_hash::FxHashSet as HashSet;
use serde_json::Value;
use std::{collections::BTreeMap, sync::Arc};

/// One level's aliases, chained onto the levels above it.
#[derive(Debug, Default)]
pub struct ParentPkgAliases {
    names: HashSet<String>,
    parent: Option<Arc<ParentPkgAliases>>,
}

impl ParentPkgAliases {
    /// The scope an importer's own direct dependencies resolve in: the
    /// aliases it declares. pnpm seeds `parentPkgAliases` from the
    /// importer's wanted dependencies and keeps adding to it — resolved
    /// direct deps, then every hoisted peer — as the hoist rounds run.
    #[must_use]
    pub fn root(names: HashSet<String>) -> Arc<Self> {
        Arc::new(ParentPkgAliases { names, parent: None })
    }

    /// The scope the children of `self`'s level resolve in.
    #[must_use]
    pub fn extend(self: &Arc<Self>, names: HashSet<String>) -> Arc<Self> {
        Arc::new(ParentPkgAliases { names, parent: Some(Arc::clone(self)) })
    }

    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        let mut level = Some(self);
        while let Some(current) = level {
            if current.names.contains(name) {
                return true;
            }
            level = current.parent.as_deref();
        }
        false
    }
}

/// [`peer_shadowed_dependency_names`] in the scope `parent_pkg_aliases`.
pub(crate) fn peer_shadowed_dependencies(
    manifest: Option<&Value>,
    parent_pkg_aliases: &ParentPkgAliases,
    auto_install_peers: bool,
) -> HashSet<String> {
    peer_shadowed_dependency_names(
        manifest,
        |name| parent_pkg_aliases.contains(name),
        auto_install_peers,
    )
    .map(str::to_string)
    .collect()
}

/// The `dependencies` entries of `manifest` that its own
/// `peerDependencies` shadow, and that the peer edge therefore supplies
/// instead. Under `auto_install_peers` every non-optional shadowed name
/// is dropped (the peer is auto-installed at the importer, so it always
/// resolves). Otherwise, and for optional peers, which are never
/// auto-installed, only the names `in_scope` are dropped, since dropping
/// a peer nothing in scope provides would leave it unsatisfied.
pub fn peer_shadowed_dependency_names(
    manifest: Option<&Value>,
    in_scope: impl Fn(&str) -> bool,
    auto_install_peers: bool,
) -> impl Iterator<Item = &str> {
    dependencies_named_as_peers(manifest)
        .filter(move |(name, is_optional_peer)| {
            (auto_install_peers && !is_optional_peer) || in_scope(name)
        })
        .map(|(name, _)| name.as_str())
}

/// The names `manifest` resolves as its own dependencies, which it
/// therefore records as no peer: the `dependencies` entries that are not
/// `peer_shadowed`, every `optionalDependencies` entry, and the package's
/// own name. Only `dependencies` are shadowed, so an entry that is also an
/// optional dependency still supplies the name itself.
pub fn own_dependency_names(
    manifest: &Value,
    peer_shadowed: impl Fn(&str) -> bool,
) -> impl Iterator<Item = &str> {
    let names = |key| {
        manifest
            .get(key)
            .and_then(Value::as_object)
            .into_iter()
            .flat_map(|map| map.keys().map(String::as_str))
    };
    names("dependencies")
        .filter(move |name| !peer_shadowed(name))
        .chain(names("optionalDependencies"))
        .chain(manifest.get("name").and_then(Value::as_str))
}

/// Every peer `manifest` declares: its `peerDependencies`, plus each
/// `optional` `peerDependenciesMeta` entry without one as an optional
/// `"*"` peer. The names it resolves itself in every scope, its
/// `optionalDependencies` and its own name, are left out. A
/// `dependencies` entry of the same name stays, since whether it is
/// supplied as a peer depends on the scope
/// ([`peer_shadowed_dependency_names`]).
#[must_use]
pub fn declared_peer_dependencies(manifest: &Value) -> BTreeMap<String, PeerDep> {
    let own: HashSet<&str> = own_dependency_names(manifest, |_| true).collect();
    let object = |key| manifest.get(key).and_then(Value::as_object);
    let mut peers: BTreeMap<String, PeerDep> = object("peerDependencies")
        .into_iter()
        .flatten()
        .filter(|(name, _)| !own.contains(name.as_str()))
        .filter_map(|(name, range)| {
            let version = range.as_str()?.to_string();
            Some((name.clone(), PeerDep { version, optional: false }))
        })
        .collect();
    for (name, info) in object("peerDependenciesMeta").into_iter().flatten() {
        if own.contains(name.as_str())
            || info.get("optional").and_then(Value::as_bool) != Some(true)
        {
            continue;
        }
        peers
            .entry(name.clone())
            .and_modify(|entry| entry.optional = true)
            .or_insert_with(|| PeerDep { version: "*".to_string(), optional: true });
    }
    peers
}

/// The `dependencies` entries of `manifest` that its own
/// `peerDependencies` also name, each with whether that peer is
/// optional. These are the only names
/// [`peer_shadowed_dependencies`] can drop.
pub(crate) fn dependencies_named_as_peers(
    manifest: Option<&Value>,
) -> impl Iterator<Item = (&String, bool)> {
    let object = |key| manifest?.get(key).and_then(Value::as_object);
    let peers = object("peerDependencies");
    let peers_meta = object("peerDependenciesMeta");
    let deps = if peers.is_some() { object("dependencies") } else { None };
    deps.into_iter()
        .flat_map(|deps| deps.keys())
        .filter(move |name| peers.is_some_and(|peers| peers.contains_key(*name)))
        .map(move |name| {
            let is_optional_peer = peers_meta
                .and_then(|meta| meta.get(name))
                .and_then(|entry| entry.get("optional"))
                .and_then(Value::as_bool)
                == Some(true);
            (name, is_optional_peer)
        })
}

#[cfg(test)]
mod tests;
