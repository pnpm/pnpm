//! Matching a reference to one snapshot of the package it names.

use super::{Graph, Referrer, Sid, compare_names};
use crate::{
    ImporterDepVersion, PkgName, PkgNameVerPeer,
    next_format::{NamedBindings, NextImporter, NextRef, names_binding},
};

impl<'a> Graph<'a> {
    pub(super) fn choose_children(&self, sid: Sid) -> Vec<(&'a PkgName, Sid)> {
        let entry = self.entries[sid];
        let mut children: Vec<(&'a PkgName, Sid)> = entry.dependencies
            .iter()
            .chain(entry.optional_dependencies.iter())
            .flatten()
            .filter_map(|(alias, reference)| {
                let key = reference.resolve(alias)?;
                Some((alias, self.choose(&key, reference.peers(), Referrer::Snapshot(sid))?))
            })
            .collect();
        children.sort_unstable_by(|left, right| compare_names(left.0, right.0));
        children
    }

    pub(super) fn choose_importer_children(
        &self,
        id: &'a str,
        importer: &'a NextImporter,
    ) -> Vec<(&'a PkgName, Sid)> {
        let mut children: Vec<(&'a PkgName, Sid)> = importer
            .resolved_dependencies()
            .filter_map(|(alias, dep)| {
                let key = dep.version.resolved_key(alias)?;
                Some((alias, self.choose(&key, dep.peers.as_ref(), Referrer::Importer(id))?))
            })
            .collect();
        children.sort_unstable_by(|left, right| compare_names(left.0, right.0));
        children
    }

    /// The snapshot `key` names from `referrer`, or `None` when the lockfile
    /// has no snapshot of the package. A lockfile can omit one, for a skipped
    /// optional dependency, and the reference then stays as it is.
    pub(super) fn choose(
        &self,
        key: &PkgNameVerPeer,
        peers: Option<&NamedBindings>,
        referrer: Referrer<'a>,
    ) -> Option<Sid> {
        let &(start, count) = self.by_package.get(&key.without_peer())?;
        if count == 1 {
            return Some(start);
        }
        if let Some(peers) = peers
            && let Some(sid) = self.snapshot_with_bindings(start, count, peers)
        {
            return Some(sid);
        }
        Some(self.snapshot_in_scope(start, count, referrer))
    }

    /// The snapshot that binds every named peer, with the fewest extra bindings.
    pub(super) fn snapshot_with_bindings(
        &self,
        start: Sid,
        count: usize,
        peers: &NamedBindings,
    ) -> Option<Sid> {
        (start..start + count)
            .filter(|&sid| {
                peers
                    .iter()
                    .all(|(name, wanted)| {
                        self.entries[sid]
                            .bindings()
                            .any(|(bound, reference)| {
                                bound.to_string() == *name && names_binding(wanted, reference)
                            })
                    })
            })
            .min_by_key(|&sid| self.entries[sid].bindings().count())
    }

    /// The snapshot none of whose bindings disagrees with the referrer's
    /// scope, agreeing on the most names, then binding the fewest names the
    /// scope lacks. The first snapshot when every one disagrees.
    ///
    /// A name the scope lacks is neutral: an optional peer bound from
    /// elsewhere in the graph is in no referrer's scope, and would otherwise
    /// keep every reference to the package from being selected by scope.
    pub(super) fn snapshot_in_scope(
        &self,
        start: Sid,
        count: usize,
        referrer: Referrer<'a>,
    ) -> Sid {
        (start..start + count)
            .filter_map(|sid| Some((sid, self.agreement(sid, referrer)?)))
            .min_by_key(|&(sid, agreement)| {
                (std::cmp::Reverse(agreement.agreeing), agreement.absent, sid)
            })
            .map_or(start, |(sid, _)| sid)
    }

    /// How the bindings of `sid` stand to the referrer's scope, or `None`
    /// when one disagrees.
    pub(super) fn agreement(&self, sid: Sid, referrer: Referrer<'a>) -> Option<Agreement> {
        let mut agreement = Agreement::default();
        for (name, reference) in self.entries[sid].bindings() {
            match self.binding_in_scope(referrer, name, reference) {
                Binding::Agrees => agreement.agreeing += 1,
                Binding::Absent => agreement.absent += 1,
                Binding::Disagrees => return None,
            }
        }
        Some(agreement)
    }

    pub(super) fn binding_in_scope(
        &self,
        referrer: Referrer<'a>,
        name: &PkgName,
        reference: &NextRef,
    ) -> Binding {
        let agrees = |same: bool| if same { Binding::Agrees } else { Binding::Disagrees };
        match referrer {
            Referrer::Snapshot(sid) => match self.entries[sid].reference_in_scope(name) {
                Some(scoped) => agrees(same_target(name, scoped, reference)),
                None if self.is_itself(sid, name, reference) => Binding::Agrees,
                None => Binding::Absent,
            },
            Referrer::Importer(id) => self.importers[id]
                .resolved_dependencies()
                .find(|(alias, _)| *alias == name)
                .map_or(Binding::Absent, |(_, dep)| {
                    agrees(importer_target_is(name, &dep.version, reference))
                }),
        }
    }

    /// Whether `reference` names the snapshot `sid` itself: a package is in
    /// the scope of its own dependencies, which can have it as a peer.
    pub(super) fn is_itself(&self, sid: Sid, name: &PkgName, reference: &NextRef) -> bool {
        self.keys[sid].name == *name
            && reference
                .resolve(name)
                .map(|key| key.without_peer())
                .as_ref()
                == Some(self.keys[sid])
    }
}

/// How many of a snapshot's bindings the referrer's scope agrees with, and
/// how many it lacks.
#[derive(Debug, Default, Clone, Copy)]
pub(super) struct Agreement {
    agreeing: usize,
    absent: usize,
}

/// How one binding stands to the referrer's scope.
pub(super) enum Binding {
    Agrees,
    Disagrees,
    Absent,
}

fn same_target(name: &PkgName, left: &NextRef, right: &NextRef) -> bool {
    match (left.as_link_target(), right.as_link_target()) {
        (Some(left), Some(right)) => left == right,
        (None, None) => {
            let target =
                |reference: &NextRef| reference.resolve(name).map(|key| key.without_peer());
            target(left) == target(right)
        }
        _ => false,
    }
}

fn importer_target_is(name: &PkgName, version: &ImporterDepVersion, reference: &NextRef) -> bool {
    match (version.as_link_target(), reference.as_link_target()) {
        (Some(left), Some(right)) => left == right,
        (None, None) => {
            let own = version.resolved_key(name).map(|key| key.without_peer());
            own.is_some() && own == reference.resolve(name).map(|key| key.without_peer())
        }
        _ => false,
    }
}
