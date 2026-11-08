//! The peers declared below a snapshot that nothing binds, which today's
//! `transitivePeerDependencies` lists beside the bound ones.

use super::{Graph, Sid};
use crate::{
    PackageMetadata, PkgName,
    next_format::{RestoreContext, tarjan},
};
use std::collections::{HashMap, HashSet};

/// Peer names interned to one integer each, so sets of them union cheaply.
type Name = u32;

impl Graph<'_> {
    /// For every snapshot, the peers it and the packages below it declare
    /// and nothing binds: the snapshot's own unbound peers, and those of its
    /// dependency children and everything below them. A dependency cycle
    /// shares one set.
    ///
    /// Today's `transitivePeerDependencies` of a snapshot lists what comes up
    /// from its children, less its own peers. See [`Self::unbound_below`].
    pub(super) fn unbound_peers(&self, context: RestoreContext<'_>) -> UnboundPeers {
        let mut names = Names::default();
        let own: Vec<Vec<Name>> = (0..self.keys.len())
            .map(|sid| {
                self.own_unbound_peers(sid, context)
                    .into_iter()
                    .map(|name| names.intern(name))
                    .collect()
            })
            .collect();
        let order = tarjan::components(
            self.keys.len(),
            |sid| {
                self.children[sid]
                    .iter()
                    .map(|&(_, child)| child)
                    .collect()
            },
            |_| true,
        );
        let mut below: Vec<HashSet<Name>> = vec![HashSet::new(); self.keys.len()];
        for component in &order.components {
            let set = self.component_set(component, &own, &below, &order);
            for &member in component {
                below[member].clone_from(&set);
            }
        }
        UnboundPeers { names, below }
    }

    /// The unbound peers of one dependency cycle's members and of everything
    /// below them, with the sets of the components below already final.
    fn component_set(
        &self,
        component: &[Sid],
        own: &[Vec<Name>],
        below: &[HashSet<Name>],
        order: &tarjan::Components,
    ) -> HashSet<Name> {
        let mut set: HashSet<Name> = HashSet::new();
        for &member in component {
            set.extend(own[member].iter().copied());
            let outside = self.children[member]
                .iter()
                .map(|&(_, child)| child)
                .filter(|child| {
                    order.component_of.get(child) != order.component_of.get(&member)
                });
            for child in outside {
                set.extend(below[child].iter().copied());
            }
        }
        set
    }

    /// The unbound peers below `sid`, less the peers `sid` declares itself: a
    /// package's own peers are listed by its dependents, not by the package.
    /// `sid`'s own set holds its own unbound peers and its children's, and
    /// the former are all declared, so the difference is the children's.
    pub(super) fn unbound_below(
        &self,
        sid: Sid,
        unbound: &UnboundPeers,
        context: RestoreContext<'_>,
    ) -> HashSet<String> {
        let declared = self
            .metadata(sid, context)
            .and_then(|metadata| metadata.peer_dependencies.as_ref());
        unbound.below[sid]
            .iter()
            .map(|&name| unbound.names.text(name))
            .filter(|name| !declared.is_some_and(|declared| declared.contains_key(*name)))
            .map(ToString::to_string)
            .collect()
    }

    fn metadata<'c>(&self, sid: Sid, context: RestoreContext<'c>) -> Option<&'c PackageMetadata> {
        context.packages.and_then(|packages| packages.get(self.keys[sid]))
    }

    /// The peers the package declares that the snapshot neither binds nor
    /// installs as a dependency of its own, optional ones included.
    fn own_unbound_peers(&self, sid: Sid, context: RestoreContext<'_>) -> Vec<String> {
        let Some(declared) = self
            .metadata(sid, context)
            .and_then(|metadata| metadata.peer_dependencies.as_ref())
        else {
            return Vec::new();
        };
        let entry = self.entries[sid];
        let satisfied = |name: &str| {
            let Ok(name) = PkgName::parse(name) else { return false };
            [&entry.peer_dependencies, &entry.dependencies, &entry.optional_dependencies]
                .into_iter()
                .flatten()
                .any(|map| map.contains_key(&name))
        };
        declared
            .keys()
            .filter(|name| !satisfied(name))
            .cloned()
            .collect()
    }
}

/// Every snapshot's unbound peers, by interned name.
pub(super) struct UnboundPeers {
    names: Names,
    below: Vec<HashSet<Name>>,
}

#[derive(Default)]
struct Names {
    ids: HashMap<String, Name>,
    texts: Vec<String>,
}

impl Names {
    fn intern(&mut self, name: String) -> Name {
        if let Some(&id) = self.ids.get(&name) {
            return id;
        }
        let id = Name::try_from(self.texts.len()).expect("fewer peer names than u32::MAX");
        self.ids.insert(name.clone(), id);
        self.texts.push(name);
        id
    }

    fn text(&self, name: Name) -> &str {
        &self.texts[name as usize]
    }
}
