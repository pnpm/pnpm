//! Index of the optional peer ranges a tree view declares.

use super::{
    context::ComparablePeerRange,
    discovery::ViewGeneration,
};
use crate::resolved_tree::ResolvedTree;
use rustc_hash::{
    FxHashMap as HashMap,
    FxHashSet as HashSet,
};
use std::{
    cell::RefCell,
    sync::Arc,
};

type RangesByPeerName = HashMap<String, Vec<ComparablePeerRange>>;

/// Every optional peer range the packages of one tree view declare, by
/// peer name. Built on the first read under a [`ViewGeneration`].
#[derive(Debug, Default)]
pub(super) struct OptionalPeerRanges {
    table: RefCell<Option<(ViewGeneration, Arc<RangesByPeerName>)>>,
}

impl OptionalPeerRanges {
    /// Whether some optional peer range declared for `peer_name` rejects
    /// `version`.
    pub(super) fn rejects(
        &self,
        tree: &ResolvedTree,
        view_generation: ViewGeneration,
        (peer_name, version): (&str, &str),
    ) -> bool {
        self.table(tree, view_generation)
            .get(peer_name)
            .is_some_and(|ranges| {
                ranges
                    .iter()
                    .any(|range| !range.satisfies(version))
            })
    }

    fn table(&self, tree: &ResolvedTree, view_generation: ViewGeneration) -> Arc<RangesByPeerName> {
        let mut table = self.table.borrow_mut();
        if let Some((generation, ranges)) = table.as_ref()
            && *generation == view_generation
        {
            return Arc::clone(ranges);
        }
        let ranges = Arc::new(collect_optional_peer_ranges(tree));
        *table = Some((view_generation, Arc::clone(&ranges)));
        ranges
    }
}

fn collect_optional_peer_ranges(tree: &ResolvedTree) -> RangesByPeerName {
    let mut raw_ranges: HashMap<&str, HashSet<&str>> = HashMap::default();
    for pkg in tree.packages.values() {
        for (peer_name, peer) in pkg.peer_set.taken() {
            if peer.optional {
                raw_ranges
                    .entry(peer_name)
                    .or_default()
                    .insert(&peer.version);
            }
        }
    }
    raw_ranges
        .into_iter()
        .map(|(peer_name, ranges)| {
            let ranges = ranges
                .into_iter()
                .map(ComparablePeerRange::new)
                .collect();
            (peer_name.to_string(), ranges)
        })
        .collect()
}
