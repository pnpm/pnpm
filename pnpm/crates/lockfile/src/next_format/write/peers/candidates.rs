use super::{PackageKey, ProviderFinder, SnapshotDepRef, children};
use crate::next_format::tarjan;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

pub(super) struct ProviderCandidates<'a> {
    component_of: HashMap<&'a PackageKey, usize>,
    references: Vec<Arc<HashSet<&'a SnapshotDepRef>>>,
}

impl<'a> ProviderCandidates<'a> {
    pub(super) fn of(finder: &ProviderFinder<'a, '_>, name: &str) -> Self {
        let keys: Vec<_> = finder.snapshots
            .keys()
            .filter(|key| finder.needs_from_outside(key, name))
            .collect();
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
        let mut references = Vec::with_capacity(components.components.len());
        for members in &components.components {
            references.push(merge_references(
                own_references(finder, &keys, members, name),
                descendant_components(&edges, members, &components, references.len()),
                &references,
            ));
        }
        let component_of = keys
            .into_iter()
            .enumerate()
            .map(|(index, key)| (key, components.component_of[&index]))
            .collect();
        ProviderCandidates { component_of, references }
    }

    pub(super) fn matching(
        &self,
        key: &PackageKey,
        matches: impl Fn(&SnapshotDepRef) -> bool,
    ) -> Option<&'a SnapshotDepRef> {
        self.references[self.component_of[key]]
            .iter()
            .copied()
            .find(|reference| matches(reference))
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

fn merge_references<'a>(
    mut own: HashSet<&'a SnapshotDepRef>,
    descendants: HashSet<usize>,
    previous: &[Arc<HashSet<&'a SnapshotDepRef>>],
) -> Arc<HashSet<&'a SnapshotDepRef>> {
    if own.is_empty() && descendants.len() == 1 {
        return Arc::clone(&previous[*descendants.iter().next().unwrap()]);
    }
    for descendant in descendants {
        own.extend(previous[descendant].iter().copied());
    }
    Arc::new(own)
}

#[cfg(test)]
mod tests;
