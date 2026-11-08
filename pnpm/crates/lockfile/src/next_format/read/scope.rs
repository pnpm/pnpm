//! The scope a variant's peers are bound in, and the providers it binds.

use super::{Graph, Provider, Referrer, Vid, child_named, has_peer_groups};
use crate::{PkgName, SnapshotDepRef};

impl<'a> Graph<'a> {
    /// The first referrer of every variant, in a walk from the importers in
    /// order. A variant no importer reaches starts a walk of its own.
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
        for vid in self.unreached_in_key_order(&walk.parent) {
            walk.queue.push(vid);
            walk.drain();
        }
        walk.parent
    }

    /// The variants no importer reaches. The vids follow the map the
    /// snapshots were read into, so they are ordered by key, the same in every
    /// process.
    fn unreached_in_key_order(&self, parent: &[Option<Referrer<'a>>]) -> Vec<Vid> {
        let mut unreached: Vec<Vid> = (0..self.keys.len())
            .filter(|&vid| parent[vid].is_none())
            .collect();
        unreached.sort_by_cached_key(|&vid| {
            (self.keys[vid].to_string(), vid - self.variants[self.keys[vid]].0)
        });
        unreached
    }

    pub(super) fn provider(
        &self,
        vid: Vid,
        name: &PkgName,
        reference: &SnapshotDepRef,
    ) -> Provider {
        if let Some(path) = reference.as_link_target() {
            return Provider::Link(path.to_string());
        }
        let Some(key) = reference.resolve(name) else {
            return Provider::Missing(reference.to_string());
        };
        let explicit = has_peer_groups(&key.suffix);
        let package = if explicit {
            self.variants.get(&key.without_peer_bindings())
        } else {
            self.variants.get(&key)
        };
        let Some(&(start, count)) = package else {
            return Provider::Missing(format!("{}@{}", key.name, key.suffix.without_peer()));
        };
        if count == 1 {
            return Provider::Variant(start);
        }
        if explicit && let Some(found) = self.variant_with_providers(start, count, &key) {
            return Provider::Variant(found);
        }
        match self.linked_above(vid, name, start, count) {
            Some(found) => Provider::Variant(found),
            None => Provider::Variant(self.variant_in_scope(start, count, Referrer::Snapshot(vid))),
        }
    }

    /// The variant among `start..start + count` linked as `name` in the scope
    /// `vid` was reached in: under its parent, its parent's parent, and so on
    /// up to an importer.
    pub(super) fn linked_above(
        &self,
        vid: Vid,
        name: &PkgName,
        start: Vid,
        count: usize,
    ) -> Option<Vid> {
        let is_candidate = |found: Vid| (start..start + count).contains(&found);
        let mut referrer = self.parent[vid];
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
/// variant.
struct Walk<'g, 'a> {
    graph: &'g Graph<'a>,
    parent: Vec<Option<Referrer<'a>>>,
    visited: Vec<bool>,
    queue: Vec<Vid>,
}

impl<'a> Walk<'_, 'a> {
    fn reach(&mut self, vid: Vid, referrer: Referrer<'a>) {
        self.parent[vid].get_or_insert(referrer);
        self.queue.push(vid);
    }

    fn drain(&mut self) {
        let graph = self.graph;
        while let Some(vid) = self.queue.pop() {
            if std::mem::replace(&mut self.visited[vid], true) {
                continue;
            }
            for &(_, child) in &graph.children[vid] {
                if !self.visited[child] {
                    self.reach(child, Referrer::Snapshot(vid));
                }
            }
        }
    }
}
