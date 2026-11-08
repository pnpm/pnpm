//! The peer-suffixed snapshot entries and importers.

use super::{
    super::{NextSnapshotEntry, RestoreContext},
    Graph, Provider, Vid, child_named,
};
use crate::{
    ImporterDepVersion, PackageKey, PkgName, PkgNameVerPeer, ProjectSnapshot, SnapshotDepRef,
    SnapshotEntry,
};
use rayon::prelude::*;
use std::collections::HashMap;

impl Graph<'_> {
    pub(super) fn snapshot_entries(
        &self,
        keys: &[PkgNameVerPeer],
        context: RestoreContext<'_>,
    ) -> HashMap<PackageKey, SnapshotEntry> {
        (0..self.keys.len())
            .into_par_iter()
            .map(|vid| (keys[vid].clone(), self.snapshot_entry(vid, keys, context)))
            .collect()
    }

    pub(super) fn snapshot_entry(
        &self,
        vid: Vid,
        keys: &[PkgNameVerPeer],
        context: RestoreContext<'_>,
    ) -> SnapshotEntry {
        let entry = self.entries[vid];
        let mut dependencies = self.restored_children(vid, entry.dependencies.as_ref(), keys);
        let mut optional_dependencies =
            self.restored_children(vid, entry.optional_dependencies.as_ref(), keys);
        let optional_peers = context.packages
            .and_then(|packages| packages.get(&self.keys[vid].without_peer()))
            .and_then(|metadata| metadata.peer_dependencies_meta.as_ref());
        for (name, reference) in entry.peer_dependencies.iter().flatten() {
            let optional = optional_peers
                .and_then(|meta| meta.get(&name.to_string()))
                .is_some_and(|meta| meta.optional);
            let target = if optional { &mut optional_dependencies } else { &mut dependencies };
            target.insert(name.clone(), self.restored_peer(vid, name, reference, keys));
        }
        SnapshotEntry {
            id: entry.id.clone(),
            dependencies: non_empty(dependencies),
            optional_dependencies: non_empty(optional_dependencies),
            transitive_peer_dependencies: transitive_peer_names(entry),
            patched: entry.patched,
            optional: entry.optional,
        }
    }

    /// `children` with each reference pointing at the variant chosen for it.
    fn restored_children(
        &self,
        vid: Vid,
        children: Option<&HashMap<PkgName, SnapshotDepRef>>,
        keys: &[PkgNameVerPeer],
    ) -> HashMap<PkgName, SnapshotDepRef> {
        children
            .into_iter()
            .flatten()
            .map(|(alias, reference)| {
                let restored = match child_named(&self.children[vid], alias) {
                    Some(child) => with_key(reference, &keys[child]),
                    None => reference.clone(),
                };
                (alias.clone(), restored)
            })
            .collect()
    }

    /// The reference to the provider `vid` binds its own peer `name` to.
    fn restored_peer(
        &self,
        vid: Vid,
        name: &PkgName,
        reference: &SnapshotDepRef,
        keys: &[PkgNameVerPeer],
    ) -> SnapshotDepRef {
        let provider = self.providers[vid]
            .iter()
            .find(|(bound, _)| *bound == name)
            .map(|(_, provider)| provider);
        match provider {
            Some(Provider::Variant(provider)) => with_key(reference, &keys[*provider]),
            _ => reference.clone(),
        }
    }

    pub(super) fn restore_importer(
        &self,
        id: &str,
        importer: &ProjectSnapshot,
        keys: &[PkgNameVerPeer],
    ) -> ProjectSnapshot {
        let mut snapshot = importer.clone();
        let chosen = &self.importer_children[id];
        for map in [
            &mut snapshot.dependencies,
            &mut snapshot.dev_dependencies,
            &mut snapshot.optional_dependencies,
        ]
        .into_iter()
        .flatten()
        {
            for (alias, spec) in map.iter_mut() {
                if let Some(vid) = child_named(chosen, alias) {
                    spec.version = importer_version_with_key(&spec.version, &keys[vid]);
                }
            }
        }
        snapshot
    }
}

/// `reference` pointing at `key`, in the same shape.
fn with_key(reference: &SnapshotDepRef, key: &PkgNameVerPeer) -> SnapshotDepRef {
    match reference {
        SnapshotDepRef::Plain(_) => SnapshotDepRef::Plain(key.suffix.clone()),
        SnapshotDepRef::Alias(_) => SnapshotDepRef::Alias(key.clone()),
        SnapshotDepRef::Link(path) => SnapshotDepRef::Link(path.clone()),
    }
}

fn importer_version_with_key(
    version: &ImporterDepVersion,
    key: &PkgNameVerPeer,
) -> ImporterDepVersion {
    match version {
        ImporterDepVersion::Regular(_) => ImporterDepVersion::Regular(key.suffix.clone()),
        ImporterDepVersion::Alias(_) => ImporterDepVersion::Alias(key.clone()),
        ImporterDepVersion::File(payload) => {
            let full = key.suffix.to_string();
            ImporterDepVersion::File(
                full.strip_prefix("file:").map_or_else(|| payload.clone(), str::to_string),
            )
        }
        ImporterDepVersion::Link(path) => ImporterDepVersion::Link(path.clone()),
    }
}

/// Today's `transitivePeerDependencies`: the bound transitive peers and the
/// missing ones, sorted.
fn transitive_peer_names(entry: &NextSnapshotEntry) -> Option<Vec<String>> {
    let bound = entry.transitive_peer_dependencies
        .iter()
        .flatten()
        .map(|(name, _)| name.to_string());
    let missing = entry.missing_peer_dependencies
        .iter()
        .flatten()
        .cloned();
    let mut names: Vec<String> = bound.chain(missing).collect();
    names.sort_unstable();
    (!names.is_empty()).then_some(names)
}

fn non_empty<Key, Value>(map: HashMap<Key, Value>) -> Option<HashMap<Key, Value>> {
    (!map.is_empty()).then_some(map)
}
