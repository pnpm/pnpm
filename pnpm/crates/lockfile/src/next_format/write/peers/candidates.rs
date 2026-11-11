use super::{PackageKey, ProviderConstraint, ProviderFinder, SnapshotDepRef, children};
use crate::next_format::tarjan;
use std::collections::{HashMap, HashSet, VecDeque};

const MAX_CACHED_BINDINGS: usize = 8;

pub(super) struct ProviderCandidates<'a> {
    component_of: HashMap<&'a PackageKey, usize>,
    components: Vec<Component<'a>>,
    cached: VecDeque<MatchingProviders<'a>>,
}

struct Component<'a> {
    references: HashSet<&'a SnapshotDepRef>,
    descendants: HashSet<usize>,
}

struct MatchingProviders<'a> {
    constraint: ProviderConstraint,
    providers: Vec<Option<&'a SnapshotDepRef>>,
}

impl<'a> ProviderCandidates<'a> {
    pub(super) fn of(finder: &ProviderFinder<'a, '_>, name: &str) -> Self {
        let keys: Vec<_> = finder.keys_needing_peer(name).collect();
        let positions: HashMap<_, _> = keys
            .iter()
            .enumerate()
            .map(|(index, key)| (*key, index))
            .collect();
        let edges: Vec<Vec<usize>> = keys
            .iter()
            .map(|key| {
                finder
                    .children_needing_peer(key, name)
                    .map(|child| positions[child])
                    .collect()
            })
            .collect();
        let components = tarjan::components(keys.len(), |index| edges[index].clone(), |_| true);
        let mut grouped = Vec::with_capacity(components.components.len());
        for members in &components.components {
            grouped.push(Component {
                references: own_references(finder, &keys, members, name),
                descendants: descendant_components(&edges, members, &components, grouped.len()),
            });
        }
        let component_of = keys
            .into_iter()
            .enumerate()
            .map(|(index, key)| (key, components.component_of[&index]))
            .collect();
        ProviderCandidates { component_of, components: grouped, cached: VecDeque::new() }
    }

    pub(super) fn matching(
        &mut self,
        key: &PackageKey,
        constraint: &ProviderConstraint,
    ) -> Option<&'a SnapshotDepRef> {
        let cached = if let Some(index) = self.cached
            .iter()
            .position(|entry| entry.constraint == *constraint)
        {
            self.cached.remove(index).unwrap()
        } else {
            if self.cached.len() == MAX_CACHED_BINDINGS {
                self.cached.pop_front();
            }
            MatchingProviders {
                constraint: constraint.clone(),
                providers: self.matching_providers(constraint),
            }
        };
        let found = cached.providers[self.component_of[key]];
        self.cached.push_back(cached);
        found
    }

    fn matching_providers(
        &self,
        constraint: &ProviderConstraint,
    ) -> Vec<Option<&'a SnapshotDepRef>> {
        let mut providers = Vec::with_capacity(self.components.len());
        for component in &self.components {
            let found = component.references
                .iter()
                .copied()
                .find(|reference| constraint.matches(reference))
                .or_else(|| {
                    component.descendants
                        .iter()
                        .find_map(|&index| providers[index])
                });
            providers.push(found);
        }
        providers
    }
}

fn own_references<'a>(
    finder: &ProviderFinder<'a, '_>,
    keys: &[&'a PackageKey],
    members: &[usize],
    name: &str,
) -> HashSet<&'a SnapshotDepRef> {
    members
        .iter()
        .flat_map(|&index| {
            children(&finder.snapshots[keys[index]])
                .filter_map(move |(alias, reference)| {
                    (finder.own_peers[keys[index]].contains(alias) && alias.to_string() == name)
                        .then_some(reference)
                })
        })
        .collect()
}

fn descendant_components(
    edges: &[Vec<usize>],
    members: &[usize],
    components: &tarjan::Components,
    ready: usize,
) -> HashSet<usize> {
    members
        .iter()
        .flat_map(|&index| &edges[index])
        .map(|child| components.component_of[child])
        .filter(|&component| component < ready)
        .collect()
}

#[cfg(test)]
mod tests;
