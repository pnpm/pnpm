use std::collections::{
    HashMap,
    HashSet,
};

use super::all_dep_refs;
use crate::{
    PkgNameVerPeer,
    SnapshotEntry,
};

const MAX_VISITS: usize = 65_536;

/// Finds the providers a suffix can name without their nested groups. The
/// resolver cuts only a provider that is still being walked, so a cut
/// provider lies on a dependency cycle.
pub(super) struct CycleProbe<'a> {
    snapshots: &'a HashMap<PkgNameVerPeer, SnapshotEntry>,
    consumer: PkgNameVerPeer,
    remaining_visits: usize,
}

impl<'a> CycleProbe<'a> {
    pub(super) fn new(
        snapshots: &'a HashMap<PkgNameVerPeer, SnapshotEntry>,
        consumer: &PkgNameVerPeer,
    ) -> Self {
        CycleProbe { snapshots, consumer: consumer.without_peer(), remaining_visits: MAX_VISITS }
    }

    /// `false` only when every dependency path from `provider` was followed
    /// and none led back to the provider's package or to the consumer's.
    /// Once the visit budget is spent, any provider with a dependency may be
    /// cut, and a provider without one still cannot be.
    pub(super) fn may_be_cut(&mut self, provider: &PkgNameVerPeer) -> bool {
        if self.remaining_visits == 0 {
            return self.has_children_or_is_unknown(provider);
        }
        let Some(pending) = self.children(provider) else {
            return true;
        };
        self.reaches_cut_point(&provider.without_peer(), pending)
    }

    fn reaches_cut_point(
        &mut self,
        provider_package: &PkgNameVerPeer,
        mut pending: Vec<PkgNameVerPeer>,
    ) -> bool {
        let mut visited = HashSet::new();
        while let Some(current) = pending.pop() {
            let package = current.without_peer();
            if &package == provider_package || package == self.consumer {
                return true;
            }
            if visited.contains(&current) {
                continue;
            }
            if self.remaining_visits == 0 {
                return true;
            }
            self.remaining_visits -= 1;
            let Some(children) = self.children(&current) else {
                return true;
            };
            pending.extend(children);
            visited.insert(current);
        }
        false
    }

    fn has_children_or_is_unknown(&self, key: &PkgNameVerPeer) -> bool {
        self.snapshots
            .get(key)
            .is_none_or(|snapshot| {
                all_dep_refs(snapshot).any(|(alias, dep_ref)| dep_ref.resolve(alias).is_some())
            })
    }

    fn children(&self, key: &PkgNameVerPeer) -> Option<Vec<PkgNameVerPeer>> {
        let snapshot = self.snapshots.get(key)?;
        Some(
            all_dep_refs(snapshot)
                .filter_map(|(alias, dep_ref)| dep_ref.resolve(alias))
                .collect(),
        )
    }
}
