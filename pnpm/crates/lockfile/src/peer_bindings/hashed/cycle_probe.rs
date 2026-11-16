use std::collections::{HashMap, HashSet};

use super::all_dep_refs;
use crate::{PkgNameVerPeer, SnapshotEntry};

const MAX_VISITS: usize = 16_384;

/// Finds the providers a consumer's suffix can name without their nested
/// groups. The resolver cuts only a provider that is still being walked, and
/// such a provider depends on the consumer.
pub(super) struct CycleProbe<'a> {
    snapshots: &'a HashMap<PkgNameVerPeer, SnapshotEntry>,
    consumer: PkgNameVerPeer,
    independent: HashSet<PkgNameVerPeer>,
    remaining_visits: usize,
}

impl<'a> CycleProbe<'a> {
    pub(super) fn new(
        snapshots: &'a HashMap<PkgNameVerPeer, SnapshotEntry>,
        consumer: &PkgNameVerPeer,
    ) -> Self {
        CycleProbe {
            snapshots,
            consumer: consumer.without_peer(),
            independent: HashSet::new(),
            remaining_visits: MAX_VISITS,
        }
    }

    /// `false` only when every dependency path from `provider` was followed
    /// and none reached the consumer.
    pub(super) fn may_depend_on_consumer(&mut self, provider: &PkgNameVerPeer) -> bool {
        let mut visited = HashSet::new();
        let mut pending = vec![provider.clone()];
        while let Some(current) = pending.pop() {
            if self.independent.contains(&current) || visited.contains(&current) {
                continue;
            }
            if current.without_peer() == self.consumer || self.remaining_visits == 0 {
                return true;
            }
            let Some(snapshot) = self.snapshots.get(&current) else {
                return true;
            };
            self.remaining_visits -= 1;
            pending.extend(
                all_dep_refs(snapshot).filter_map(|(alias, dep_ref)| dep_ref.resolve(alias)),
            );
            visited.insert(current);
        }
        self.independent.extend(visited);
        false
    }
}
