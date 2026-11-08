//! The next format's importers, and references without peer groups.

use super::Explicit;
use crate::{
    ImporterDepVersion, PkgName, PkgNameVerPeer, PkgVerPeer, ProjectSnapshot,
    ResolvedDependencyMap, ResolvedDependencySpec, SnapshotDepRef,
};
use std::collections::HashMap;

/// The importer with its versions' peer groups dropped, except where its
/// scope does not select the snapshot.
pub(super) fn next_importer(
    id: &str,
    importer: &ProjectSnapshot,
    explicit: &Explicit,
) -> ProjectSnapshot {
    let mut snapshot = importer.clone();
    for map in [
        &mut snapshot.dependencies,
        &mut snapshot.dev_dependencies,
        &mut snapshot.optional_dependencies,
    ]
    .into_iter()
    .flatten()
    {
        for (alias, spec) in map.iter_mut() {
            if !explicit.has_importer_reference(id, alias) {
                spec.version = importer_version_without_peer_bindings(&spec.version);
            }
        }
    }
    snapshot
}

pub(in crate::next_format) fn importer_dependencies(
    importer: &ProjectSnapshot,
) -> impl Iterator<Item = (&PkgName, &ResolvedDependencySpec)> {
    [&importer.dependencies, &importer.dev_dependencies, &importer.optional_dependencies]
        .into_iter()
        .flatten()
        .flat_map(ResolvedDependencyMap::iter)
}

pub(super) fn without_peer_bindings(reference: &SnapshotDepRef) -> SnapshotDepRef {
    match reference {
        SnapshotDepRef::Plain(version) => SnapshotDepRef::Plain(version.without_peer_bindings()),
        SnapshotDepRef::Alias(key) => SnapshotDepRef::Alias(key.without_peer_bindings()),
        SnapshotDepRef::Link(path) => SnapshotDepRef::Link(path.clone()),
    }
}

pub(super) fn importer_version_without_peer_bindings(
    version: &ImporterDepVersion,
) -> ImporterDepVersion {
    match version {
        ImporterDepVersion::Regular(version) => {
            ImporterDepVersion::Regular(version.without_peer_bindings())
        }
        ImporterDepVersion::Alias(key) => ImporterDepVersion::Alias(key.without_peer_bindings()),
        ImporterDepVersion::File(payload) => {
            ImporterDepVersion::File(match format!("file:{payload}").parse::<PkgVerPeer>() {
                Ok(version) => version
                    .without_peer_bindings()
                    .to_string()
                    .strip_prefix("file:")
                    .map_or_else(|| payload.clone(), str::to_string),
                Err(_) => payload.clone(),
            })
        }
        ImporterDepVersion::Link(path) => ImporterDepVersion::Link(path.clone()),
    }
}

pub(super) fn non_empty<Key, Value>(map: HashMap<Key, Value>) -> Option<HashMap<Key, Value>> {
    (!map.is_empty()).then_some(map)
}

impl PkgNameVerPeer {
    /// This key with its peer groups dropped and its patch group kept: the
    /// key of the next format.
    #[must_use]
    pub fn without_peer_bindings(&self) -> PkgNameVerPeer {
        PkgNameVerPeer::new(self.name.clone(), self.suffix.without_peer_bindings())
    }
}
