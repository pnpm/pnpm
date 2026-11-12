//! The peer-suffixed snapshot entries and importers.

use super::{
    super::{NextSnapshotEntry, RestoreContext},
    Graph, Provider, RestoredKey, Sid, child_named,
};
use crate::{
    ImporterDepVersion, PackageKey, PkgName, PkgNameVerPeer, ProjectSnapshot, SnapshotDepRef,
    SnapshotEntry,
    next_format::{NextImporter, NextRef},
};
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};

impl Graph<'_> {
    pub(super) fn snapshot_entries(
        &self,
        keys: &[RestoredKey],
        context: RestoreContext<'_>,
    ) -> HashMap<PackageKey, SnapshotEntry> {
        let unbound = self.unbound_peers(context);
        (0..self.keys.len())
            .into_par_iter()
            .map(|sid| {
                let below = self.unbound_below(sid, &unbound, context);
                (keys[sid].key.clone(), self.snapshot_entry(sid, keys, context, &below))
            })
            .collect()
    }

    pub(super) fn snapshot_entry(
        &self,
        sid: Sid,
        keys: &[RestoredKey],
        context: RestoreContext<'_>,
        unbound_below: &HashSet<String>,
    ) -> SnapshotEntry {
        let entry = self.entries[sid];
        let mut dependencies = self.restored_children(sid, entry.dependencies.as_ref(), keys);
        let mut optional_dependencies =
            self.restored_children(sid, entry.optional_dependencies.as_ref(), keys);
        let optional_peers = context.packages
            .and_then(|packages| packages.get(&self.keys[sid].without_peer()))
            .and_then(|metadata| metadata.peer_dependencies_meta.as_ref());
        for (name, reference) in entry.peer_dependencies.iter().flatten() {
            let optional = optional_peers
                .and_then(|meta| meta.get(&name.to_string()))
                .is_some_and(|meta| meta.optional);
            let target = if optional { &mut optional_dependencies } else { &mut dependencies };
            target.insert(name.clone(), self.restored_peer(sid, name, reference, keys));
        }
        SnapshotEntry {
            recorded_peer_bindings: keys[sid].bindings.clone(),
            id: entry.id.clone(),
            dependencies: non_empty(dependencies),
            optional_dependencies: non_empty(optional_dependencies),
            transitive_peer_dependencies: transitive_peer_names(entry, unbound_below),
            patched: None,
            optional: entry.optional,
        }
    }

    /// `children` with each reference pointing at the snapshot chosen for it.
    fn restored_children(
        &self,
        sid: Sid,
        children: Option<&HashMap<PkgName, NextRef>>,
        keys: &[RestoredKey],
    ) -> HashMap<PkgName, SnapshotDepRef> {
        children
            .into_iter()
            .flatten()
            .map(|(alias, reference)| {
                let restored = match child_named(&self.children[sid], alias) {
                    Some(child) => with_key(reference.version(), &keys[child].key),
                    None => reference.version().clone(),
                };
                (alias.clone(), restored)
            })
            .collect()
    }

    /// The reference to the provider `sid` binds its own peer `name` to.
    fn restored_peer(
        &self,
        sid: Sid,
        name: &PkgName,
        reference: &NextRef,
        keys: &[RestoredKey],
    ) -> SnapshotDepRef {
        let provider = self.providers[sid]
            .iter()
            .find(|(bound, _)| *bound == name)
            .map(|(_, provider)| provider);
        match provider {
            Some(Provider::Snapshot(provider)) => {
                with_key(reference.version(), &keys[*provider].key)
            }
            _ => reference.version().clone(),
        }
    }

    pub(super) fn restore_importer(
        &self,
        id: &str,
        importer: &NextImporter,
        keys: &[RestoredKey],
    ) -> ProjectSnapshot {
        let chosen = &self.importer_children[id];
        importer.project_snapshot(|alias, dep| match child_named(chosen, alias) {
            Some(sid) => importer_version_with_key(&dep.version, &keys[sid].key),
            None => dep.version.clone(),
        })
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
/// peers below that nothing binds, sorted.
fn transitive_peer_names(
    entry: &NextSnapshotEntry,
    unbound_below: &HashSet<String>,
) -> Option<Vec<String>> {
    let bound = entry.transitive_peer_dependencies
        .iter()
        .flatten()
        .map(|(name, _)| name.to_string());
    let mut names: Vec<String> = bound.collect();
    match &entry.unbound_peer_dependencies {
        Some(unbound) => names.extend(unbound.iter().cloned()),
        None => names.extend(unbound_below.iter().cloned()),
    }
    names.sort_unstable();
    names.dedup();
    (!names.is_empty()).then_some(names)
}

fn non_empty<Key, Value>(map: HashMap<Key, Value>) -> Option<HashMap<Key, Value>> {
    (!map.is_empty()).then_some(map)
}
