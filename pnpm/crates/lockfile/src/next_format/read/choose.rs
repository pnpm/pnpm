//! Matching a reference to one snapshot of the package it names.

use super::{
    super::write::importer_dependencies, Graph, Referrer, Sid, compare_names, has_peer_groups,
};
use crate::{
    ImporterDepVersion, PkgName, PkgNameVerPeer, ProjectSnapshot, SnapshotDepRef,
    split_peer_suffix_segments,
};
use pnpm_deps_path::link_path_to_peer_version;
use std::collections::HashSet;

impl<'a> Graph<'a> {
    pub(super) fn choose_children(&self, sid: Sid) -> Vec<(&'a PkgName, Sid)> {
        let entry = self.entries[sid];
        let mut children: Vec<(&'a PkgName, Sid)> = entry.dependencies
            .iter()
            .chain(entry.optional_dependencies.iter())
            .flatten()
            .filter_map(|(alias, reference)| {
                let key = reference.resolve(alias)?;
                Some((alias, self.choose(&key, Referrer::Snapshot(sid))?))
            })
            .collect();
        children.sort_unstable_by(|left, right| compare_names(left.0, right.0));
        children
    }

    pub(super) fn choose_importer_children(
        &self,
        id: &'a str,
        importer: &'a ProjectSnapshot,
    ) -> Vec<(&'a PkgName, Sid)> {
        let mut children: Vec<(&'a PkgName, Sid)> = importer_dependencies(importer)
            .filter_map(|(alias, spec)| {
                let key = spec.version.resolved_key(alias)?;
                Some((alias, self.choose(&key, Referrer::Importer(id))?))
            })
            .collect();
        children.sort_unstable_by(|left, right| compare_names(left.0, right.0));
        children
    }

    /// The snapshot `key` names from `referrer`, or `None` when the lockfile
    /// has no snapshot of the package. A lockfile can omit one, for a skipped
    /// optional dependency, and the reference then stays as it is.
    pub(super) fn choose(&self, key: &PkgNameVerPeer, referrer: Referrer<'a>) -> Option<Sid> {
        let explicit = has_peer_groups(&key.suffix);
        let &(start, count) = if explicit {
            self.by_package.get(&key.without_peer_bindings())?
        } else {
            self.by_package.get(key)?
        };
        if count == 1 {
            return Some(start);
        }
        if explicit && let Some(sid) = self.snapshot_with_providers(start, count, key) {
            return Some(sid);
        }
        Some(self.snapshot_in_scope(start, count, referrer))
    }

    /// The snapshot whose providers are the peer groups of `key`.
    pub(super) fn snapshot_with_providers(
        &self,
        start: Sid,
        count: usize,
        key: &PkgNameVerPeer,
    ) -> Option<Sid> {
        let wanted: HashSet<String> = split_peer_suffix_segments(key.suffix.peer())?
            .into_iter()
            .filter(|segment| !segment.starts_with("patch_hash="))
            .map(|segment| match segment.find('(') {
                Some(nested) => segment[..nested].to_string(),
                None => segment,
            })
            .collect();
        (start..start + count).find(|&sid| self.provider_pairs(sid) == wanted)
    }

    /// `name@version` of every provider of `sid`, as a collapsed peer suffix
    /// writes it.
    pub(super) fn provider_pairs(&self, sid: Sid) -> HashSet<String> {
        self.entries[sid]
            .bindings()
            .filter_map(|(name, reference)| match reference.as_link_target() {
                Some(path) => Some(format!("{name}@{}", link_path_to_peer_version(path))),
                None => reference
                    .resolve(name)
                    .map(|key| format!("{}@{}", key.name, key.suffix.without_peer())),
            })
            .collect()
    }

    /// The snapshot whose bindings all agree with the referrer's scope, binding
    /// the most names. The first snapshot when none agrees.
    pub(super) fn snapshot_in_scope(
        &self,
        start: Sid,
        count: usize,
        referrer: Referrer<'a>,
    ) -> Sid {
        (start..start + count)
            .filter_map(|sid| Some((sid, self.agreeing_bindings(sid, referrer)?)))
            .min_by_key(|&(sid, bound)| (std::cmp::Reverse(bound), sid))
            .map_or(start, |(sid, _)| sid)
    }

    /// How many bindings `sid` has, if every one agrees with the referrer.
    pub(super) fn agreeing_bindings(&self, sid: Sid, referrer: Referrer<'a>) -> Option<usize> {
        let mut count = 0;
        for (name, reference) in self.entries[sid].bindings() {
            if !self.in_scope(referrer, name, reference) {
                return None;
            }
            count += 1;
        }
        Some(count)
    }

    pub(super) fn in_scope(
        &self,
        referrer: Referrer<'a>,
        name: &PkgName,
        reference: &SnapshotDepRef,
    ) -> bool {
        match referrer {
            Referrer::Snapshot(sid) => match self.entries[sid].reference_in_scope(name) {
                Some(scoped) => same_target(name, scoped, reference),
                None => self.is_itself(sid, name, reference),
            },
            Referrer::Importer(id) => importer_dependencies(self.importers[id])
                .find(|(alias, _)| *alias == name)
                .is_some_and(|(_, spec)| importer_target_is(name, &spec.version, reference)),
        }
    }

    /// Whether `reference` names the snapshot `sid` itself: a package is in
    /// the scope of its own dependencies, which can have it as a peer.
    pub(super) fn is_itself(&self, sid: Sid, name: &PkgName, reference: &SnapshotDepRef) -> bool {
        self.keys[sid].name == *name
            && reference
                .resolve(name)
                .map(|key| key.without_peer_bindings())
                .as_ref()
                == Some(self.keys[sid])
    }
}

fn same_target(name: &PkgName, left: &SnapshotDepRef, right: &SnapshotDepRef) -> bool {
    match (left.as_link_target(), right.as_link_target()) {
        (Some(left), Some(right)) => left == right,
        (None, None) => {
            let target = |reference: &SnapshotDepRef| {
                reference.resolve(name).map(|key| key.without_peer_bindings())
            };
            target(left) == target(right)
        }
        _ => false,
    }
}

fn importer_target_is(
    name: &PkgName,
    version: &ImporterDepVersion,
    reference: &SnapshotDepRef,
) -> bool {
    match (version.as_link_target(), reference.as_link_target()) {
        (Some(left), Some(right)) => left == right,
        (None, None) => {
            let own = version.resolved_key(name).map(|key| key.without_peer_bindings());
            own.is_some() && own == reference.resolve(name).map(|key| key.without_peer_bindings())
        }
        _ => false,
    }
}
