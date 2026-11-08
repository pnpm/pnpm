//! The scope a snapshot's peers are bound in, and the providers it binds.

use super::{Graph, Provider, Referrer, Sid, child_named};
use crate::{PkgName, next_format::NextRef};

impl<'a> Graph<'a> {
    /// The first referrer of every snapshot, in a walk from the importers in
    /// order. A snapshot no importer reaches starts a walk of its own.
    pub(super) fn parents(&self) -> Vec<Option<Referrer<'a>>> {
        let mut walk = Walk {
            graph: self,
            parent: vec![None; self.keys.len()],
            visited: vec![false; self.keys.len()],
            queue: Vec::new(),
        };
        let mut ids: Vec<&'a str> = self.importer_children
            .keys()
            .copied()
            .collect();
        ids.sort_unstable();
        for id in ids {
            for &(_, child) in &self.importer_children[id] {
                walk.reach(child, Referrer::Importer(id));
            }
            walk.drain();
        }
        for sid in self.unreached_in_key_order(&walk.parent) {
            walk.queue.push(sid);
            walk.drain();
        }
        walk.parent
    }

    /// The snapshots no importer reaches. The sids follow the map the
    /// snapshots were read into, so they are ordered by key, the same in every
    /// process.
    fn unreached_in_key_order(&self, parent: &[Option<Referrer<'a>>]) -> Vec<Sid> {
        let mut unreached: Vec<Sid> = (0..self.keys.len())
            .filter(|&sid| parent[sid].is_none())
            .collect();
        unreached.sort_by_cached_key(|&sid| {
            (self.keys[sid].to_string(), sid - self.by_package[self.keys[sid]].0)
        });
        unreached
    }

    pub(super) fn provider(&self, sid: Sid, name: &PkgName, reference: &NextRef) -> Provider {
        if let Some(path) = reference.as_link_target() {
            return Provider::Link(path.to_string());
        }
        let Some(key) = reference.resolve(name) else {
            return Provider::Missing(reference.version().to_string());
        };
        let Some(&(start, count)) = self.by_package.get(&key.without_peer()) else {
            return Provider::Missing(format!("{}@{}", key.name, key.suffix.without_peer()));
        };
        if count == 1 {
            return Provider::Snapshot(start);
        }
        if let Some(peers) = reference.peers()
            && let Some(found) = self.snapshot_with_bindings(start, count, peers)
        {
            return Provider::Snapshot(found);
        }
        match self.linked_above(sid, name, start, count) {
            Some(found) => Provider::Snapshot(found),
            None => {
                Provider::Snapshot(self.snapshot_in_scope(start, count, Referrer::Snapshot(sid)))
            }
        }
    }

    /// The snapshot among `start..start + count` linked as `name` in the scope
    /// `sid` was reached in: under its parent, its parent's parent, and so on
    /// up to an importer.
    pub(super) fn linked_above(
        &self,
        sid: Sid,
        name: &PkgName,
        start: Sid,
        count: usize,
    ) -> Option<Sid> {
        let is_candidate = |found: Sid| (start..start + count).contains(&found);
        let mut referrer = self.parent[sid];
        let mut steps = 0;
        while let Some(current) = referrer {
            let linked = match current {
                Referrer::Snapshot(parent) => child_named(&self.children[parent], name)
                    .or_else(|| (self.keys[parent].name == *name).then_some(parent)),
                Referrer::Importer(id) => child_named(&self.importer_children[id], name),
            };
            if let Some(found) = linked.filter(|&found| is_candidate(found)) {
                return Some(found);
            }
            referrer = match current {
                Referrer::Snapshot(parent) => self.parent[parent],
                Referrer::Importer(_) => None,
            };
            steps += 1;
            if steps > self.keys.len() {
                return None;
            }
        }
        None
    }
}

/// A walk from the importers that records the first referrer of every
/// snapshot.
struct Walk<'g, 'a> {
    graph: &'g Graph<'a>,
    parent: Vec<Option<Referrer<'a>>>,
    visited: Vec<bool>,
    queue: Vec<Sid>,
}

impl<'a> Walk<'_, 'a> {
    fn reach(&mut self, sid: Sid, referrer: Referrer<'a>) {
        self.parent[sid].get_or_insert(referrer);
        self.queue.push(sid);
    }

    fn drain(&mut self) {
        let graph = self.graph;
        while let Some(sid) = self.queue.pop() {
            if std::mem::replace(&mut self.visited[sid], true) {
                continue;
            }
            for &(_, child) in &graph.children[sid] {
                if !self.visited[child] {
                    self.reach(child, Referrer::Snapshot(sid));
                }
            }
        }
    }
}
