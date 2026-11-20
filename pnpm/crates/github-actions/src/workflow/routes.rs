use super::{
    Component, HashSet, QueryError, Route,
    graph::{KeyKind, MAX_MERGE_DEPTH, NodeId, YamlGraph, YamlNode},
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
    let mut lookup =
        MapLookup { graph, key, confirmed_missing: HashSet::new(), active_index: HashMap::new() };
    match lookup.find(node_id, prefix, 0) {
        LookupOutcome::Found(value_id, path) => Some((value_id, path)),
        LookupOutcome::Missing { .. } | LookupOutcome::Cutoff => None,
    }
}

enum LookupOutcome {
    Found(NodeId, Vec<Component<'static>>),
    Missing { lowlink: Option<usize> },
    Cutoff,
}

enum DirectScan {
    Found(NodeId, Vec<Component<'static>>),
    Merge(Option<(String, NodeId)>),
}

struct MapLookup<'graph> {
    graph: &'graph YamlGraph,
    key: &'static str,
    confirmed_missing: HashSet<NodeId>,
    active_index: HashMap<NodeId, usize>,
}

impl MapLookup<'_> {
    fn find(
        &mut self,
        node_id: NodeId,
        prefix: &[Component<'static>],
        depth: usize,
    ) -> LookupOutcome {
        let Some((resolved_id, YamlNode::Mapping(entries))) = self.graph.deref(node_id) else {
            return LookupOutcome::Missing { lowlink: None };
        };
        if self.confirmed_missing.contains(&resolved_id) {
            return LookupOutcome::Missing { lowlink: None };
        }
        if let Some(&ancestor_pos) = self.active_index.get(&resolved_id) {
            return LookupOutcome::Missing { lowlink: Some(ancestor_pos) };
        }
        let merge_entry = match self.scan_direct(entries, prefix) {
            DirectScan::Found(value_id, path) => return LookupOutcome::Found(value_id, path),
            DirectScan::Merge(merge_entry) => merge_entry,
        };
        let Some((merge_route_key, merge_id)) = merge_entry else {
            self.confirmed_missing.insert(resolved_id);
            return LookupOutcome::Missing { lowlink: None };
        };
        if depth >= MAX_MERGE_DEPTH {
            return LookupOutcome::Cutoff;
        }
        self.descend_lookup_merge(resolved_id, merge_id, &merge_route_key, prefix, depth + 1)
    }

    fn descend_lookup_merge(
        &mut self,
        resolved_id: NodeId,
        merge_id: NodeId,
        merge_route_key: &str,
        prefix: &[Component<'static>],
        depth: usize,
    ) -> LookupOutcome {
        let my_pos = self.active_index.len();
        self.active_index.insert(resolved_id, my_pos);
        let outcome = self.find_merged(merge_id, merge_route_key, prefix, depth);
        self.active_index.remove(&resolved_id);
        match outcome {
            LookupOutcome::Missing { lowlink } if lowlink.is_none_or(|pos| pos >= my_pos) => {
                self.confirmed_missing.insert(resolved_id);
                LookupOutcome::Missing { lowlink: None }
            }
            other => other,
        }
    }

    fn scan_direct(
        &self,
        entries: &[(NodeId, NodeId)],
        prefix: &[Component<'static>],
    ) -> DirectScan {
        let mut merge_entry = None;
        for &(key_id, value_id) in entries {
            match self.graph.key_at(key_id) {
                Some(KeyKind::String { decoded, route_key }) if decoded == self.key => {
                    let mut path = prefix.to_vec();
                    path.push(route_key.to_owned().into());
                    return DirectScan::Found(value_id, path);
                }
                Some(KeyKind::Merge { route_key }) if merge_entry.is_none() => {
                    merge_entry = Some((route_key.to_owned(), value_id));
                }
                _ => {}
            }
        }
        DirectScan::Merge(merge_entry)
    }

    fn find_merged(
        &mut self,
        merge_id: NodeId,
        merge_route_key: &str,
        prefix: &[Component<'static>],
        depth: usize,
    ) -> LookupOutcome {
        let Some((_, merged_node)) = self.graph.deref(merge_id) else {
            return LookupOutcome::Missing { lowlink: None };
        };
        match merged_node {
            YamlNode::Mapping(_) => {
                let mut merge_prefix = prefix.to_vec();
                merge_prefix.push(merge_route_key.to_owned().into());
                self.find(merge_id, &merge_prefix, depth)
            }
            YamlNode::Sequence(items) => {
                self.find_merged_sequence(items, merge_route_key, prefix, depth)
            }
            _ => LookupOutcome::Missing { lowlink: None },
        }
    }

    fn find_merged_sequence(
        &mut self,
        items: &[NodeId],
        merge_route_key: &str,
        prefix: &[Component<'static>],
        depth: usize,
    ) -> LookupOutcome {
        let mut min_lowlink = None;
        for (index, &item_id) in items.iter().enumerate() {
            let mut item_prefix = prefix.to_vec();
            item_prefix.extend([merge_route_key.to_owned().into(), index.into()]);
            match self.find(item_id, &item_prefix, depth) {
                outcome @ (LookupOutcome::Found(..) | LookupOutcome::Cutoff) => return outcome,
                LookupOutcome::Missing { lowlink } => {
                    min_lowlink = merge_lowlink(min_lowlink, lowlink);
                }
            }
        }
        LookupOutcome::Missing { lowlink: min_lowlink }
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
        completed: HashSet::new(),
        active_index: HashMap::new(),
        entries: Vec::new(),
    };
    let _ = collector.collect(node_id, prefix, 0);
    collector.entries
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CollectOutcome {
    Complete { lowlink: Option<usize> },
    Cutoff,
}

struct MapCollector<'graph> {
    graph: &'graph YamlGraph,
    seen_keys: HashSet<String>,
    completed: HashSet<NodeId>,
    active_index: HashMap<NodeId, usize>,
    entries: Vec<(NodeId, Vec<Component<'static>>)>,
}

impl MapCollector<'_> {
    fn collect(
        &mut self,
        node_id: NodeId,
        prefix: &[Component<'static>],
        depth: usize,
    ) -> CollectOutcome {
        let Some((resolved_id, YamlNode::Mapping(entries))) = self.graph.deref(node_id) else {
            return CollectOutcome::Complete { lowlink: None };
        };
        if self.completed.contains(&resolved_id) {
            return CollectOutcome::Complete { lowlink: None };
        }
        if let Some(&ancestor_pos) = self.active_index.get(&resolved_id) {
            return CollectOutcome::Complete { lowlink: Some(ancestor_pos) };
        }
        let Some((merge_route_key, merge_id)) = self.collect_direct_entries(entries, prefix) else {
            self.completed.insert(resolved_id);
            return CollectOutcome::Complete { lowlink: None };
        };
        if depth >= MAX_MERGE_DEPTH {
            return CollectOutcome::Cutoff;
        }
        self.descend_collect_merge(resolved_id, merge_id, &merge_route_key, prefix, depth + 1)
    }

    fn descend_collect_merge(
        &mut self,
        resolved_id: NodeId,
        merge_id: NodeId,
        merge_route_key: &str,
        prefix: &[Component<'static>],
        depth: usize,
    ) -> CollectOutcome {
        let my_pos = self.active_index.len();
        self.active_index.insert(resolved_id, my_pos);
        let outcome = self.collect_merged(merge_id, merge_route_key, prefix, depth);
        self.active_index.remove(&resolved_id);
        match outcome {
            CollectOutcome::Complete { lowlink } if lowlink.is_none_or(|pos| pos >= my_pos) => {
                self.completed.insert(resolved_id);
                CollectOutcome::Complete { lowlink: None }
            }
            other => other,
        }
    }

    fn collect_direct_entries(
        &mut self,
        entries: &[(NodeId, NodeId)],
        prefix: &[Component<'static>],
    ) -> Option<(String, NodeId)> {
        let mut merge_entry = None;
        for &(key_id, value_id) in entries {
            match self.graph.key_at(key_id) {
                Some(KeyKind::Merge { route_key }) if merge_entry.is_none() => {
                    merge_entry = Some((route_key.to_owned(), value_id));
                }
                Some(KeyKind::String { decoded, route_key })
                    if self.seen_keys.insert(decoded.to_owned()) =>
                {
                    let mut entry_prefix = prefix.to_vec();
                    entry_prefix.push(route_key.to_owned().into());
                    self.entries.push((value_id, entry_prefix));
                }
                _ => {}
            }
        }
        merge_entry
    }

    fn collect_merged(
        &mut self,
        merge_id: NodeId,
        merge_route_key: &str,
        prefix: &[Component<'static>],
        depth: usize,
    ) -> CollectOutcome {
        let Some((_, merged_node)) = self.graph.deref(merge_id) else {
            return CollectOutcome::Complete { lowlink: None };
        };
        match merged_node {
            YamlNode::Mapping(_) => {
                let mut merge_prefix = prefix.to_vec();
                merge_prefix.push(merge_route_key.to_owned().into());
                self.collect(merge_id, &merge_prefix, depth)
            }
            YamlNode::Sequence(items) => {
                self.collect_merged_sequence(items, merge_route_key, prefix, depth)
            }
            _ => CollectOutcome::Complete { lowlink: None },
        }
    }

    fn collect_merged_sequence(
        &mut self,
        items: &[NodeId],
        merge_route_key: &str,
        prefix: &[Component<'static>],
        depth: usize,
    ) -> CollectOutcome {
        let mut min_lowlink = None;
        for (index, &item_id) in items.iter().enumerate() {
            let mut item_prefix = prefix.to_vec();
            item_prefix.extend([merge_route_key.to_owned().into(), index.into()]);
            match self.collect(item_id, &item_prefix, depth) {
                CollectOutcome::Cutoff => return CollectOutcome::Cutoff,
                CollectOutcome::Complete { lowlink } => {
                    min_lowlink = merge_lowlink(min_lowlink, lowlink);
                }
            }
        }
        CollectOutcome::Complete { lowlink: min_lowlink }
    }
}

fn merge_lowlink(current: Option<usize>, next: Option<usize>) -> Option<usize> {
    match (current, next) {
        (Some(left), Some(right)) => Some(left.min(right)),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    }
}
