use super::{Arc, DepPath, HashMap, HashSet, SubtreePaths};

#[derive(Debug)]
struct LiveSubtree {
    _node: Arc<SubtreePaths>,
    refs: usize,
}

#[derive(Debug, Default)]
pub(in super::super::super) struct LivePaths {
    dep_path_counts: HashMap<DepPath, usize>,
    live_subtrees: HashMap<usize, LiveSubtree>,
    in_flight: HashSet<DepPath>,
    in_flight_visited: HashMap<usize, Arc<SubtreePaths>>,
}

impl LivePaths {
    pub(super) fn clear(&mut self) {
        self.dep_path_counts.clear();
        self.live_subtrees.clear();
        self.clear_in_flight();
    }

    pub(super) fn clear_in_flight(&mut self) {
        self.in_flight.clear();
        self.in_flight_visited.clear();
    }

    #[must_use]
    pub(in super::super::super) fn len(&self) -> usize {
        self.dep_path_counts.len() + self.in_flight.len()
    }

    #[cfg(test)]
    #[must_use]
    pub(in super::super::super) fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[cfg(test)]
    #[must_use]
    pub(in super::super::super) fn contains(&self, dep_path: &DepPath) -> bool {
        self.dep_path_counts.contains_key(dep_path) || self.in_flight.contains(dep_path)
    }

    pub(super) fn record_in_flight_dep_path(&mut self, dep_path: &DepPath, max: usize) -> bool {
        if self.dep_path_counts.contains_key(dep_path) || self.in_flight.contains(dep_path) {
            return true;
        }
        if self.len() >= max {
            return false;
        }
        self.in_flight.insert(dep_path.clone());
        true
    }

    pub(super) fn mark_in_flight_subtree(&mut self, subtree: &Arc<SubtreePaths>) {
        self.in_flight_visited.insert(Arc::as_ptr(subtree) as usize, Arc::clone(subtree));
    }

    pub(super) fn record_in_flight_subtree(
        &mut self,
        root: &Arc<SubtreePaths>,
        max: usize,
    ) -> bool {
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            let ptr = Arc::as_ptr(node) as usize;
            if self.live_subtrees.contains_key(&ptr) || self.in_flight_visited.contains_key(&ptr) {
                continue;
            }
            if !self.record_in_flight_dep_path(&node.own, max) {
                return false;
            }
            self.in_flight_visited.insert(ptr, Arc::clone(node));
            stack.extend(node.children.iter());
        }
        true
    }

    pub(super) fn retain_subtree(&mut self, root: &Arc<SubtreePaths>) {
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            let ptr = Arc::as_ptr(node) as usize;
            if let Some(live) = self.live_subtrees.get_mut(&ptr) {
                live.refs += 1;
                continue;
            }
            self.live_subtrees.insert(ptr, LiveSubtree { _node: Arc::clone(node), refs: 1 });
            *self.dep_path_counts.entry(node.own.clone()).or_default() += 1;
            stack.extend(node.children.iter());
        }
    }

    pub(super) fn release_subtree(&mut self, root: &Arc<SubtreePaths>) {
        let mut stack = vec![Arc::clone(root)];
        while let Some(node) = stack.pop() {
            let ptr = Arc::as_ptr(&node) as usize;
            let Some(live) = self.live_subtrees.get_mut(&ptr) else {
                debug_assert!(false, "releasing untracked live subtree");
                continue;
            };
            if live.refs > 1 {
                live.refs -= 1;
                continue;
            }
            self.live_subtrees.remove(&ptr);
            self.decrement_dep_path(&node.own);
            stack.extend(node.children.iter().cloned());
        }
    }

    fn decrement_dep_path(&mut self, dep_path: &DepPath) {
        let Some(count) = self.dep_path_counts.get_mut(dep_path) else {
            return;
        };
        if *count > 1 {
            *count -= 1;
        } else {
            self.dep_path_counts.remove(dep_path);
        }
    }
}
