use super::{
    Component, HashSet, QueryError, Route,
    graph::{MAX_MERGE_DEPTH, NodeId, YamlGraph, YamlNode},
};
use std::collections::HashMap;

pub(super) fn uses_routes(text: &str) -> Result<Vec<Route<'static>>, QueryError> {
    let graph = YamlGraph::parse(text)?;
    let Some(root_id) = graph.root else {
        return Ok(Vec::new());
    };
    let mut routes = Vec::new();
    if let Some((jobs_id, jobs_prefix)) = find_map_entry(&graph, root_id, &[], "jobs") {
        add_jobs_routes(&graph, &mut routes, jobs_id, &jobs_prefix);
    }
    if let Some((runs_id, runs_prefix)) = find_map_entry(&graph, root_id, &[], "runs")
        && let Some((steps_id, steps_prefix)) =
            find_map_entry(&graph, runs_id, &runs_prefix, "steps")
    {
        add_step_routes(&graph, &mut routes, steps_id, &steps_prefix);
    }
    Ok(routes)
}

fn add_jobs_routes(
    graph: &YamlGraph,
    routes: &mut Vec<Route<'static>>,
    jobs_id: NodeId,
    jobs_prefix: &[Component<'static>],
) {
    for (job_id, job_prefix) in collect_map_entries(graph, jobs_id, jobs_prefix) {
        if !matches!(graph.deref(job_id), Some((_, YamlNode::Mapping(_)))) {
            continue;
        }
        if let Some(route) = find_string_route(graph, job_id, &job_prefix, "uses") {
            routes.push(route);
        }
        if let Some((steps_id, steps_prefix)) = find_map_entry(graph, job_id, &job_prefix, "steps")
        {
            add_step_routes(graph, routes, steps_id, &steps_prefix);
        }
    }
}

fn add_step_routes(
    graph: &YamlGraph,
    routes: &mut Vec<Route<'static>>,
    steps_id: NodeId,
    steps_prefix: &[Component<'static>],
) {
    let Some((_, YamlNode::Sequence(items))) = graph.deref(steps_id) else {
        return;
    };
    for (index, &step_id) in items.iter().enumerate() {
        let mut step_prefix = steps_prefix.to_vec();
        step_prefix.push(index.into());
        if let Some(route) = find_string_route(graph, step_id, &step_prefix, "uses") {
            routes.push(route);
        }
    }
}

fn find_string_route(
    graph: &YamlGraph,
    node_id: NodeId,
    prefix: &[Component<'static>],
    key: &'static str,
) -> Option<Route<'static>> {
    let (value_id, path) = find_map_entry(graph, node_id, prefix, key)?;
    graph.string_at(value_id).map(|_| Route::from(path))
}

fn find_map_entry(
    graph: &YamlGraph,
    node_id: NodeId,
    prefix: &[Component<'static>],
    key: &'static str,
) -> Option<(NodeId, Vec<Component<'static>>)> {
    let mut lookup = MapLookup { graph, key, visited_depth: HashMap::new() };
    lookup.find(node_id, prefix, 0)
}

struct MapLookup<'graph> {
    graph: &'graph YamlGraph,
    key: &'static str,
    visited_depth: HashMap<NodeId, usize>,
}

impl MapLookup<'_> {
    fn find(
        &mut self,
        node_id: NodeId,
        prefix: &[Component<'static>],
        depth: usize,
    ) -> Option<(NodeId, Vec<Component<'static>>)> {
        let (resolved_id, YamlNode::Mapping(entries)) = self.graph.deref(node_id)? else {
            return None;
        };
        if self.visited_depth
            .get(&resolved_id)
            .is_some_and(|&previous| previous <= depth)
        {
            return None;
        }
        self.visited_depth.insert(resolved_id, depth);

        let mut merge_target = None;
        for &(key_id, value_id) in entries {
            let Some(entry_key) = key_id.and_then(|id| self.graph.string_at(id)) else {
                continue;
            };
            if entry_key == self.key {
                let mut path = prefix.to_vec();
                path.push(self.key.into());
                return Some((value_id, path));
            }
            if entry_key == "<<" && merge_target.is_none() {
                merge_target = Some(value_id);
            }
        }
        if depth >= MAX_MERGE_DEPTH {
            return None;
        }
        self.find_merged(merge_target?, prefix, depth + 1)
    }

    fn find_merged(
        &mut self,
        merge_id: NodeId,
        prefix: &[Component<'static>],
        depth: usize,
    ) -> Option<(NodeId, Vec<Component<'static>>)> {
        match self.graph.deref(merge_id)?.1 {
            YamlNode::Mapping(_) => {
                let mut merge_prefix = prefix.to_vec();
                merge_prefix.push("<<".into());
                self.find(merge_id, &merge_prefix, depth)
            }
            YamlNode::Sequence(items) => {
                for (index, &item_id) in items.iter().enumerate() {
                    let mut item_prefix = prefix.to_vec();
                    item_prefix.extend(["<<".into(), index.into()]);
                    if let Some(found) = self.find(item_id, &item_prefix, depth) {
                        return Some(found);
                    }
                }
                None
            }
            YamlNode::Scalar(_) | YamlNode::Alias(_) => None,
        }
    }
}

fn collect_map_entries(
    graph: &YamlGraph,
    node_id: NodeId,
    prefix: &[Component<'static>],
) -> Vec<(NodeId, Vec<Component<'static>>)> {
    let mut collector = MapCollector {
        graph,
        seen_keys: HashSet::new(),
        visited_depth: HashMap::new(),
        entries: Vec::new(),
    };
    collector.collect(node_id, prefix, 0);
    collector.entries
}

struct MapCollector<'graph> {
    graph: &'graph YamlGraph,
    seen_keys: HashSet<String>,
    visited_depth: HashMap<NodeId, usize>,
    entries: Vec<(NodeId, Vec<Component<'static>>)>,
}

impl MapCollector<'_> {
    fn collect(&mut self, node_id: NodeId, prefix: &[Component<'static>], depth: usize) {
        let Some((resolved_id, YamlNode::Mapping(entries))) = self.graph.deref(node_id) else {
            return;
        };
        if self.visited_depth
            .get(&resolved_id)
            .is_some_and(|&previous| previous <= depth)
        {
            return;
        }
        self.visited_depth.insert(resolved_id, depth);

        let merge_target = self.collect_direct_entries(entries, prefix);
        if depth < MAX_MERGE_DEPTH
            && let Some(merge_id) = merge_target
        {
            self.collect_merged(merge_id, prefix, depth + 1);
        }
    }

    fn collect_direct_entries(
        &mut self,
        entries: &[(Option<NodeId>, NodeId)],
        prefix: &[Component<'static>],
    ) -> Option<NodeId> {
        let mut merge_target = None;
        for &(key_id, value_id) in entries {
            let Some(key_name) = key_id.and_then(|id| self.graph.string_at(id)) else {
                continue;
            };
            if key_name == "<<" {
                merge_target.get_or_insert(value_id);
            } else if self.seen_keys.insert(key_name.to_owned()) {
                let mut entry_prefix = prefix.to_vec();
                entry_prefix.push(key_name.to_owned().into());
                self.entries.push((value_id, entry_prefix));
            }
        }
        merge_target
    }

    fn collect_merged(&mut self, merge_id: NodeId, prefix: &[Component<'static>], depth: usize) {
        let Some((_, merged_node)) = self.graph.deref(merge_id) else {
            return;
        };
        match merged_node {
            YamlNode::Mapping(_) => {
                let mut merge_prefix = prefix.to_vec();
                merge_prefix.push("<<".into());
                self.collect(merge_id, &merge_prefix, depth);
            }
            YamlNode::Sequence(items) => {
                for (index, &item_id) in items.iter().enumerate() {
                    let mut item_prefix = prefix.to_vec();
                    item_prefix.extend(["<<".into(), index.into()]);
                    self.collect(item_id, &item_prefix, depth);
                }
            }
            YamlNode::Scalar(_) | YamlNode::Alias(_) => {}
        }
    }
}
