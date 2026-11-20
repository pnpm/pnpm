use super::{
    HashSet, QueryError, Range,
    graph::{KeyKind, MAX_MERGE_DEPTH, NodeId, YamlGraph, YamlNode},
};
use std::collections::HashMap;

const MAX_MERGE_STEPS: usize = 4_096;

pub(super) struct UsesScalar {
    pub(super) decoded: String,
    pub(super) span: Range<usize>,
}

pub(super) fn uses_scalars(text: &str) -> Result<Vec<UsesScalar>, QueryError> {
    let graph = YamlGraph::parse(text)?;
    let Some(root_id) = graph.root else {
        return Ok(Vec::new());
    };
    let mut scalars = Vec::new();
    let mut seen_step_sequences = HashSet::new();
    if let Some(jobs_id) = find_map_entry(&graph, root_id, "jobs") {
        add_jobs_scalars(&graph, &mut scalars, &mut seen_step_sequences, jobs_id);
    }
    if let Some(runs_id) = find_map_entry(&graph, root_id, "runs")
        && let Some(steps_id) = find_map_entry(&graph, runs_id, "steps")
    {
        add_step_scalars(&graph, &mut scalars, &mut seen_step_sequences, steps_id);
    }
    Ok(scalars)
}

fn add_jobs_scalars(
    graph: &YamlGraph,
    scalars: &mut Vec<UsesScalar>,
    seen_step_sequences: &mut HashSet<NodeId>,
    jobs_id: NodeId,
) {
    for job_id in collect_map_entries(graph, jobs_id) {
        if !matches!(graph.deref(job_id), Some((_, YamlNode::Mapping(_)))) {
            continue;
        }
        if let Some(scalar) = find_uses_scalar(graph, job_id) {
            scalars.push(scalar);
        }
        if let Some(steps_id) = find_map_entry(graph, job_id, "steps") {
            add_step_scalars(graph, scalars, seen_step_sequences, steps_id);
        }
    }
}

fn add_step_scalars(
    graph: &YamlGraph,
    scalars: &mut Vec<UsesScalar>,
    seen_step_sequences: &mut HashSet<NodeId>,
    steps_id: NodeId,
) {
    let Some((resolved_steps_id, YamlNode::Sequence(items))) = graph.deref(steps_id) else {
        return;
    };
    if !seen_step_sequences.insert(resolved_steps_id) {
        return;
    }
    for &step_id in items {
        if let Some(scalar) = find_uses_scalar(graph, step_id) {
            scalars.push(scalar);
        }
    }
}

fn find_uses_scalar(graph: &YamlGraph, node_id: NodeId) -> Option<UsesScalar> {
    let value_id = find_map_entry(graph, node_id, "uses")?;
    let (decoded, span) = graph.inline_string_at(value_id)?;
    Some(UsesScalar { decoded: decoded.to_owned(), span })
}

struct CycleTracker {
    confirmed: HashSet<NodeId>,
    active_index: HashMap<NodeId, usize>,
    cycle_masks: HashMap<NodeId, u64>,
    evict_stack: Vec<Vec<NodeId>>,
    steps: usize,
}

impl CycleTracker {
    fn new() -> Self {
        Self {
            confirmed: HashSet::new(),
            active_index: HashMap::new(),
            cycle_masks: HashMap::new(),
            evict_stack: Vec::new(),
            steps: 0,
        }
    }

    fn step_budget_exceeded(&mut self) -> bool {
        self.steps += 1;
        self.steps > MAX_MERGE_STEPS
    }

    fn check_cached(&self, resolved_id: NodeId) -> Option<u64> {
        if self.confirmed.contains(&resolved_id) {
            return Some(0);
        }
        if let Some(&ancestor_pos) = self.active_index.get(&resolved_id) {
            return Some(1_u64 << ancestor_pos);
        }
        self.cycle_masks.get(&resolved_id).copied()
    }

    fn enter(&mut self, resolved_id: NodeId) -> usize {
        let my_pos = self.active_index.len();
        self.active_index.insert(resolved_id, my_pos);
        self.evict_stack.push(Vec::new());
        my_pos
    }

    fn exit_complete(&mut self, resolved_id: NodeId, my_pos: usize, dep_mask: u64) -> u64 {
        self.evict_frame(resolved_id);
        let external_mask = dep_mask & ((1_u64 << my_pos) - 1);
        if external_mask == 0 {
            self.confirmed.insert(resolved_id);
            return 0;
        }
        let deepest_dep = 63 - external_mask.leading_zeros() as usize;
        self.cycle_masks.insert(resolved_id, external_mask);
        self.evict_stack[deepest_dep].push(resolved_id);
        external_mask
    }

    fn evict_frame(&mut self, resolved_id: NodeId) {
        self.active_index.remove(&resolved_id);
        if let Some(evicted) = self.evict_stack.pop() {
            for cached_id in evicted {
                self.cycle_masks.remove(&cached_id);
            }
        }
    }
}

fn find_map_entry(graph: &YamlGraph, node_id: NodeId, key: &'static str) -> Option<NodeId> {
    let mut lookup = MapLookup { graph, key, tracker: CycleTracker::new() };
    match lookup.find(node_id, 0) {
        LookupOutcome::Found(value_id) => Some(value_id),
        LookupOutcome::Missing { .. } | LookupOutcome::Cutoff => None,
    }
}

enum LookupOutcome {
    Found(NodeId),
    Missing { dep_mask: u64 },
    Cutoff,
}

enum DirectScan {
    Found(NodeId),
    Merge(Option<NodeId>),
}

struct MapLookup<'graph> {
    graph: &'graph YamlGraph,
    key: &'static str,
    tracker: CycleTracker,
}

impl MapLookup<'_> {
    fn find(&mut self, node_id: NodeId, depth: usize) -> LookupOutcome {
        if self.tracker.step_budget_exceeded() {
            return LookupOutcome::Cutoff;
        }
        let Some((resolved_id, YamlNode::Mapping(entries))) = self.graph.deref(node_id) else {
            return LookupOutcome::Missing { dep_mask: 0 };
        };
        if let Some(dep_mask) = self.tracker.check_cached(resolved_id) {
            return LookupOutcome::Missing { dep_mask };
        }
        let merge_entry = match self.scan_direct(entries) {
            DirectScan::Found(value_id) => return LookupOutcome::Found(value_id),
            DirectScan::Merge(merge_entry) => merge_entry,
        };
        let Some(merge_id) = merge_entry else {
            self.tracker.confirmed.insert(resolved_id);
            return LookupOutcome::Missing { dep_mask: 0 };
        };
        if depth >= MAX_MERGE_DEPTH {
            return LookupOutcome::Cutoff;
        }
        self.descend_merge(resolved_id, merge_id, depth + 1)
    }

    fn descend_merge(
        &mut self,
        resolved_id: NodeId,
        merge_id: NodeId,
        depth: usize,
    ) -> LookupOutcome {
        let my_pos = self.tracker.enter(resolved_id);
        match self.find_merged(merge_id, depth) {
            LookupOutcome::Missing { dep_mask } => {
                let external = self.tracker.exit_complete(resolved_id, my_pos, dep_mask);
                LookupOutcome::Missing { dep_mask: external }
            }
            outcome => {
                self.tracker.evict_frame(resolved_id);
                outcome
            }
        }
    }

    fn scan_direct(&self, entries: &[(NodeId, NodeId)]) -> DirectScan {
        let mut merge_entry = None;
        for &(key_id, value_id) in entries {
            match self.graph.key_at(key_id) {
                Some(KeyKind::String(decoded)) if decoded == self.key => {
                    return DirectScan::Found(value_id);
                }
                Some(KeyKind::Merge) if merge_entry.is_none() => merge_entry = Some(value_id),
                _ => {}
            }
        }
        DirectScan::Merge(merge_entry)
    }

    fn find_merged(&mut self, merge_id: NodeId, depth: usize) -> LookupOutcome {
        match self.graph.deref(merge_id).map(|(_, node)| node) {
            Some(YamlNode::Mapping(_)) => self.find(merge_id, depth),
            Some(YamlNode::Sequence(items)) => self.find_merged_sequence(items, depth),
            _ => LookupOutcome::Missing { dep_mask: 0 },
        }
    }

    fn find_merged_sequence(&mut self, items: &[NodeId], depth: usize) -> LookupOutcome {
        let mut combined_mask = 0;
        for &item_id in items {
            match self.find(item_id, depth) {
                outcome @ (LookupOutcome::Found(_) | LookupOutcome::Cutoff) => return outcome,
                LookupOutcome::Missing { dep_mask } => combined_mask |= dep_mask,
            }
        }
        LookupOutcome::Missing { dep_mask: combined_mask }
    }
}

fn collect_map_entries(graph: &YamlGraph, node_id: NodeId) -> Vec<NodeId> {
    let mut collector = MapCollector {
        graph,
        seen_keys: HashSet::new(),
        tracker: CycleTracker::new(),
        entries: Vec::new(),
    };
    let _ = collector.collect(node_id, 0);
    collector.entries
}

enum CollectOutcome {
    Complete { dep_mask: u64 },
    Cutoff,
}

struct MapCollector<'graph> {
    graph: &'graph YamlGraph,
    seen_keys: HashSet<String>,
    tracker: CycleTracker,
    entries: Vec<NodeId>,
}

impl MapCollector<'_> {
    fn collect(&mut self, node_id: NodeId, depth: usize) -> CollectOutcome {
        if self.tracker.step_budget_exceeded() {
            return CollectOutcome::Cutoff;
        }
        let Some((resolved_id, YamlNode::Mapping(entries))) = self.graph.deref(node_id) else {
            return CollectOutcome::Complete { dep_mask: 0 };
        };
        if let Some(dep_mask) = self.tracker.check_cached(resolved_id) {
            return CollectOutcome::Complete { dep_mask };
        }
        let Some(merge_id) = self.collect_direct_entries(entries) else {
            self.tracker.confirmed.insert(resolved_id);
            return CollectOutcome::Complete { dep_mask: 0 };
        };
        if depth >= MAX_MERGE_DEPTH {
            return CollectOutcome::Cutoff;
        }
        self.descend_merge(resolved_id, merge_id, depth + 1)
    }

    fn descend_merge(
        &mut self,
        resolved_id: NodeId,
        merge_id: NodeId,
        depth: usize,
    ) -> CollectOutcome {
        let my_pos = self.tracker.enter(resolved_id);
        match self.collect_merged(merge_id, depth) {
            CollectOutcome::Complete { dep_mask } => {
                let external = self.tracker.exit_complete(resolved_id, my_pos, dep_mask);
                CollectOutcome::Complete { dep_mask: external }
            }
            CollectOutcome::Cutoff => {
                self.tracker.evict_frame(resolved_id);
                CollectOutcome::Cutoff
            }
        }
    }

    fn collect_direct_entries(&mut self, entries: &[(NodeId, NodeId)]) -> Option<NodeId> {
        let mut merge_entry = None;
        for &(key_id, value_id) in entries {
            match self.graph.key_at(key_id) {
                Some(KeyKind::Merge) if merge_entry.is_none() => merge_entry = Some(value_id),
                Some(KeyKind::String(decoded)) if self.seen_keys.insert(decoded.to_owned()) => {
                    self.entries.push(value_id);
                }
                _ => {}
            }
        }
        merge_entry
    }

    fn collect_merged(&mut self, merge_id: NodeId, depth: usize) -> CollectOutcome {
        match self.graph.deref(merge_id).map(|(_, node)| node) {
            Some(YamlNode::Mapping(_)) => self.collect(merge_id, depth),
            Some(YamlNode::Sequence(items)) => self.collect_merged_sequence(items, depth),
            _ => CollectOutcome::Complete { dep_mask: 0 },
        }
    }

    fn collect_merged_sequence(&mut self, items: &[NodeId], depth: usize) -> CollectOutcome {
        let mut combined_mask = 0;
        for &item_id in items {
            match self.collect(item_id, depth) {
                CollectOutcome::Cutoff => return CollectOutcome::Cutoff,
                CollectOutcome::Complete { dep_mask } => combined_mask |= dep_mask,
            }
        }
        CollectOutcome::Complete { dep_mask: combined_mask }
    }
}
