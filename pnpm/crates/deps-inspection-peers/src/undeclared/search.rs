use super::{Provider, Providers, UndeclaredPeerInspector};
use pnpm_lockfile::PkgNameVerPeer;
use std::collections::{HashMap, HashSet};

pub(super) struct ProviderRequirement<'a> {
    pub(super) peer: &'a str,
    pub(super) provider: &'a Provider,
}

#[derive(Default)]
struct ProviderSearch {
    pending: Vec<PkgNameVerPeer>,
    visited: HashSet<PkgNameVerPeer>,
    dependents: HashMap<PkgNameVerPeer, Vec<PkgNameVerPeer>>,
    unprovided: HashSet<PkgNameVerPeer>,
}

impl ProviderSearch {
    fn include_parent(
        &mut self,
        parent: &PkgNameVerPeer,
        child: &PkgNameVerPeer,
        declared: Option<&[Provider]>,
        provider: &Provider,
    ) {
        match declared {
            Some(declared)
                if !declared
                    .iter()
                    .any(|declared| declared.is_same_package(provider)) =>
            {
                self.unprovided.insert(child.clone());
            }
            Some(_) => {}
            None => {
                self.dependents
                    .entry(parent.clone())
                    .or_default()
                    .push(child.clone());
                self.pending.push(parent.clone());
            }
        }
    }

    fn finish(mut self) -> HashSet<PkgNameVerPeer> {
        let mut pending: Vec<_> = self.unprovided
            .iter()
            .cloned()
            .collect();
        while let Some(key) = pending.pop() {
            for dependent in self.dependents
                .get(&key)
                .into_iter()
                .flatten()
            {
                if self.unprovided.insert(dependent.clone()) {
                    pending.push(dependent.clone());
                }
            }
        }
        self.unprovided
    }
}

pub(super) struct DeclarationGraph<'a> {
    pub(super) roots: HashSet<PkgNameVerPeer>,
    pub(super) providers: &'a Providers,
    pub(super) parents: HashMap<PkgNameVerPeer, Vec<PkgNameVerPeer>>,
}

fn scope_provides(scope: &Providers, peer: &str, provider: &Provider) -> bool {
    scope
        .get(peer)
        .is_some_and(|declared| {
            declared
                .iter()
                .any(|declared| declared.is_same_package(provider))
        })
}

/// Search only ancestors of the targets, stopping each path at a provider declaration.
pub(super) fn unprovided_targets<'a>(
    inspector: &UndeclaredPeerInspector<'_>,
    graph: &DeclarationGraph<'_>,
    requirement: &ProviderRequirement<'_>,
    targets: impl Iterator<Item = &'a PkgNameVerPeer>,
) -> HashSet<PkgNameVerPeer> {
    let mut search =
        ProviderSearch { pending: targets.cloned().collect(), ..ProviderSearch::default() };
    while let Some(key) = search.pending.pop() {
        if !search.visited.insert(key.clone()) {
            continue;
        }
        #[cfg(test)]
        inspector.visited_states.set(inspector.visited_states.get() + 1);
        if graph.roots.contains(&key)
            && !scope_provides(graph.providers, requirement.peer, requirement.provider)
        {
            search.unprovided.insert(key.clone());
        }
        for parent in graph.parents
            .get(&key)
            .into_iter()
            .flatten()
        {
            search.include_parent(
                parent,
                &key,
                inspector.nodes[parent].children.get(requirement.peer).map(Vec::as_slice),
                requirement.provider,
            );
        }
    }
    search.finish()
}
