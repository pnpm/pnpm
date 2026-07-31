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

use rustc_hash::FxHashSet as HashSet;
use serde_json::Value;
use std::sync::Arc;

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

/// The `dependencies` entries of `manifest` that its own
/// `peerDependencies` shadow, and that the peer edge therefore supplies
/// instead. Under `auto_install_peers` every shadowed name is dropped
/// (the peer is auto-installed at the importer, so it always resolves);
/// otherwise only the names already resolvable from
/// `parent_pkg_aliases`, since dropping a peer nothing in scope
/// provides would leave it unsatisfied.
pub(crate) fn peer_shadowed_dependencies(
    manifest: Option<&Value>,
    parent_pkg_aliases: &ParentPkgAliases,
    auto_install_peers: bool,
) -> HashSet<String> {
    let Some(manifest) = manifest else { return HashSet::default() };
    let object = |key| manifest.get(key).and_then(Value::as_object);
    let (Some(peers), Some(deps)) = (object("peerDependencies"), object("dependencies")) else {
        return HashSet::default();
    };
    deps.keys()
        .filter(|name| peers.contains_key(*name))
        .filter(|name| auto_install_peers || parent_pkg_aliases.contains(name))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests;
