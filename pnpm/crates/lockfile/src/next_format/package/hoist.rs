//! What a package's snapshots share, written once in its entry.

use super::super::{
    NextRef,
    NextSnapshotEntry,
};
use crate::PkgName;
use std::collections::HashMap;

/// What `snapshots` share, and what each adds, when there are several.
pub(super) fn hoist(
    mut snapshots: Vec<NextSnapshotEntry>,
) -> (NextSnapshotEntry, Option<Vec<NextSnapshotEntry>>) {
    if snapshots.len() <= 1 {
        return (snapshots.pop().unwrap_or_default(), None);
    }
    let shared = {
        let (first, rest) = snapshots.split_first_mut().expect("several snapshots");
        NextSnapshotEntry {
            id: common_value(first.id.as_ref(), rest.iter().map(|snapshot| snapshot.id.as_ref())),
            dependencies: common_entries(first.dependencies.as_ref(), rest, |snapshot| {
                &snapshot.dependencies
            }),
            optional_dependencies: common_entries(
                first.optional_dependencies.as_ref(),
                rest,
                |snapshot| &snapshot.optional_dependencies,
            ),
            peer_dependencies: common_entries(first.peer_dependencies.as_ref(), rest, |snapshot| {
                &snapshot.peer_dependencies
            }),
            transitive_peer_dependencies: common_entries(
                first.transitive_peer_dependencies.as_ref(),
                rest,
                |snapshot| &snapshot.transitive_peer_dependencies,
            ),
            unbound_peer_dependencies: common_value(
                first.unbound_peer_dependencies.as_ref(),
                rest.iter().map(|snapshot| snapshot.unbound_peer_dependencies.as_ref()),
            ),
            optional: first.optional && rest.iter().all(|snapshot| snapshot.optional),
        }
    };
    for snapshot in &mut snapshots {
        snapshot.subtract(&shared);
    }
    (shared, Some(snapshots))
}

/// `first` when every other snapshot has the same value.
fn common_value<'a, Value: PartialEq + Clone + 'a>(
    first: Option<&Value>,
    others: impl Iterator<Item = Option<&'a Value>>,
) -> Option<Value> {
    let first = first?;
    let mut others = others;
    others
        .all(|other| other == Some(first))
        .then(|| first.clone())
}

/// The entries of `first` that every other snapshot has with the same value.
fn common_entries(
    first: Option<&HashMap<PkgName, NextRef>>,
    others: &[NextSnapshotEntry],
    map: impl Fn(&NextSnapshotEntry) -> &Option<HashMap<PkgName, NextRef>>,
) -> Option<HashMap<PkgName, NextRef>> {
    let common: HashMap<PkgName, NextRef> = first
        .into_iter()
        .flatten()
        .filter(|(name, reference)| {
            others
                .iter()
                .all(|other| {
                    map(other)
                        .as_ref()
                        .and_then(|entries| entries.get(*name))
                        == Some(*reference)
                })
        })
        .map(|(name, reference)| (name.clone(), reference.clone()))
        .collect();
    (!common.is_empty()).then_some(common)
}
