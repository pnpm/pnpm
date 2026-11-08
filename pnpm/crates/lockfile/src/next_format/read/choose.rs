//! Matching a reference to one variant of the package it names.

use super::{
    super::{NextProjectSnapshot, write::importer_dependencies},
    Graph, Referrer, Vid, compare_names, has_peer_groups,
};
use crate::{
    ImporterDepVersion, PkgName, PkgNameVerPeer, SnapshotDepRef, split_peer_suffix_segments,
};
use pnpm_deps_path::link_path_to_peer_version;
use std::collections::HashSet;

impl<'a> Graph<'a> {
    pub(super) fn choose_children(&self, vid: Vid) -> Vec<(&'a PkgName, Vid)> {
        let entry = self.entries[vid];
        let mut children: Vec<(&'a PkgName, Vid)> = entry.dependencies
            .iter()
            .chain(entry.optional_dependencies.iter())
            .flatten()
            .filter_map(|(alias, reference)| {
                let key = reference.resolve(alias)?;
                Some((alias, self.choose(&key, Referrer::Snapshot(vid))?))
            })
            .collect();
        children.sort_unstable_by(|left, right| compare_names(left.0, right.0));
        children
    }

    pub(super) fn choose_importer_children(
        &self,
        id: &'a str,
        importer: &'a NextProjectSnapshot,
    ) -> Vec<(&'a PkgName, Vid)> {
        let mut children: Vec<(&'a PkgName, Vid)> = importer_dependencies(&importer.snapshot)
            .filter_map(|(alias, spec)| {
                let key = spec.version.resolved_key(alias)?;
                Some((alias, self.choose(&key, Referrer::Importer(id))?))
            })
            .collect();
        children.sort_unstable_by(|left, right| compare_names(left.0, right.0));
        children
    }

    /// The variant `key` names from `referrer`, or `None` when the lockfile
    /// has no snapshot of the package. A lockfile can omit one, for a skipped
    /// optional dependency, and the reference then stays as it is.
    pub(super) fn choose(&self, key: &PkgNameVerPeer, referrer: Referrer<'a>) -> Option<Vid> {
        let explicit = has_peer_groups(&key.suffix);
        let &(start, count) = if explicit {
            self.variants.get(&key.without_peer_bindings())?
        } else {
            self.variants.get(key)?
        };
        if count == 1 {
            return Some(start);
        }
        if explicit && let Some(vid) = self.variant_with_providers(start, count, key) {
            return Some(vid);
        }
        Some(self.variant_in_scope(start, count, referrer))
    }

    /// The variant whose providers are the peer groups of `key`.
    pub(super) fn variant_with_providers(
        &self,
        start: Vid,
        count: usize,
        key: &PkgNameVerPeer,
    ) -> Option<Vid> {
        let wanted: HashSet<String> = split_peer_suffix_segments(key.suffix.peer())?
            .into_iter()
            .filter(|segment| !segment.starts_with("patch_hash="))
            .map(|segment| match segment.find('(') {
                Some(nested) => segment[..nested].to_string(),
                None => segment,
            })
            .collect();
        (start..start + count).find(|&vid| self.provider_pairs(vid) == wanted)
    }

    /// `name@version` of every provider of `vid`, as a collapsed peer suffix
    /// writes it.
    pub(super) fn provider_pairs(&self, vid: Vid) -> HashSet<String> {
        self.entries[vid]
            .bindings()
            .filter_map(|(name, reference)| match reference.as_link_target() {
                Some(path) => Some(format!("{name}@{}", link_path_to_peer_version(path))),
                None => reference
                    .resolve(name)
                    .map(|key| format!("{}@{}", key.name, key.suffix.without_peer())),
            })
            .collect()
    }

    /// The variant whose bindings all agree with the referrer's scope, binding
    /// the most names. The first variant when none agrees.
    pub(super) fn variant_in_scope(&self, start: Vid, count: usize, referrer: Referrer<'a>) -> Vid {
        (start..start + count)
            .filter_map(|vid| Some((vid, self.agreeing_bindings(vid, referrer)?)))
            .min_by_key(|&(vid, bound)| (std::cmp::Reverse(bound), vid))
            .map_or(start, |(vid, _)| vid)
    }

    /// How many bindings `vid` has, if every one agrees with the referrer.
    pub(super) fn agreeing_bindings(&self, vid: Vid, referrer: Referrer<'a>) -> Option<usize> {
        let mut count = 0;
        for (name, reference) in self.entries[vid].bindings() {
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
            Referrer::Snapshot(vid) => match self.entries[vid].reference_in_scope(name) {
                Some(scoped) => same_target(name, scoped, reference),
                None => self.is_itself(vid, name, reference),
            },
            Referrer::Importer(id) => {
                let importer = self.importers[id];
                let own =
                    importer_dependencies(&importer.snapshot).find(|(alias, _)| *alias == name);
                if let Some((_, spec)) = own {
                    return importer_target_is(name, &spec.version, reference);
                }
                importer.transitive_peer_dependencies
                    .as_ref()
                    .and_then(|transitive| transitive.get(name))
                    .is_some_and(|scoped| same_target(name, scoped, reference))
            }
        }
    }

    /// Whether `reference` names the snapshot `vid` itself: a package is in
    /// the scope of its own dependencies, which can have it as a peer.
    pub(super) fn is_itself(&self, vid: Vid, name: &PkgName, reference: &SnapshotDepRef) -> bool {
        self.keys[vid].name == *name
            && reference
                .resolve(name)
                .map(|key| key.without_peer_bindings())
                .as_ref()
                == Some(self.keys[vid])
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
