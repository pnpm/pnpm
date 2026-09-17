use super::{AuditGraph, DepClass, HashMap, Include, PackageKey};
use pnpm_workspace_task_scheduler::StronglyConnectedComponents;

/// A component stays live while it has a pending target or a live dependency.
pub(super) struct LiveGraph {
    component_of: HashMap<PackageKey, usize>,
    parents: Vec<Vec<usize>>,
    live_sources: Vec<usize>,
}

impl LiveGraph {
    pub(super) fn new(
        graph: &AuditGraph<'_>,
        include: Include,
        classes: &HashMap<PackageKey, DepClass>,
        targets: &[PackageKey],
    ) -> Self {
        let keys = classes
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        let index = keys
            .iter()
            .enumerate()
            .map(|(index, key)| (key.clone(), index))
            .collect::<HashMap<_, _>>();
        let adjacency = indexed_edges(graph, include, &keys, &index);
        let components = StronglyConnectedComponents::compute(&adjacency, &vec![false; keys.len()]);
        let component_of = keys
            .into_iter()
            .enumerate()
            .map(|(index, key)| (key, components.component_ids()[index]))
            .collect();
        let parents = component_parents(&adjacency, &components);
        let mut live = Self { component_of, live_sources: vec![0; parents.len()], parents };
        for key in targets {
            live.live_sources[live.component_of[key]] += 1;
        }
        live.count_live_dependencies();
        live
    }

    fn count_live_dependencies(&mut self) {
        let mut needed = vec![false; self.parents.len()];
        let mut stack = self.live_sources
            .iter()
            .enumerate()
            .filter(|(_, count)| **count > 0)
            .map(|(component, _)| component)
            .collect::<Vec<_>>();
        while let Some(component) = stack.pop() {
            if std::mem::replace(&mut needed[component], true) {
                continue;
            }
            stack.extend(self.parents[component].iter().copied());
        }
        for (component, parents) in self.parents.iter().enumerate() {
            if !needed[component] {
                continue;
            }
            for &parent in parents {
                self.live_sources[parent] += 1;
            }
        }
    }

    pub(super) fn contains(&self, key: &PackageKey) -> bool {
        self.live_sources[self.component_of[key]] > 0
    }

    pub(super) fn remove_target(&mut self, key: &PackageKey) {
        let mut stack = vec![self.component_of[key]];
        while let Some(component) = stack.pop() {
            self.live_sources[component] -= 1;
            if self.live_sources[component] == 0 {
                stack.extend(self.parents[component].iter().copied());
            }
        }
    }
}

fn indexed_edges(
    graph: &AuditGraph<'_>,
    include: Include,
    keys: &[PackageKey],
    index: &HashMap<PackageKey, usize>,
) -> Vec<Vec<usize>> {
    keys.iter()
        .map(|key| {
            graph
                .children(key, include.optional_dependencies)
                .into_iter()
                .map(|edge| index[&edge.key])
                .collect()
        })
        .collect()
}

fn component_parents(
    adjacency: &[Vec<usize>],
    components: &StronglyConnectedComponents,
) -> Vec<Vec<usize>> {
    let mut parents = vec![Vec::new(); components.component_count()];
    for (from, children) in adjacency.iter().enumerate() {
        let from = components.component_ids()[from];
        for &to in children {
            let to = components.component_ids()[to];
            if from != to {
                parents[to].push(from);
            }
        }
    }
    parents
}
