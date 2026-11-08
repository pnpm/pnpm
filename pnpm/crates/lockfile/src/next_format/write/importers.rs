//! The next format's importers, and references without peer groups.

use super::{
    super::{NextFormatError, NextProjectSnapshot, NextSnapshotEntry},
    Explicit,
};
use crate::{
    ImporterDepVersion, PackageKey, PkgName, PkgNameVerPeer, PkgVerPeer, ProjectSnapshot,
    ResolvedDependencyMap, ResolvedDependencySpec, SnapshotDepRef,
};
use std::collections::HashMap;

pub(super) fn next_importer<'e>(
    id: &str,
    importer: &ProjectSnapshot,
    entry_of: &impl Fn(&PackageKey) -> Option<&'e NextSnapshotEntry>,
    explicit: &Explicit,
) -> Result<NextProjectSnapshot, NextFormatError> {
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
    let transitive = importer_transitive_peers(id, importer, entry_of)?;
    Ok(NextProjectSnapshot { snapshot, transitive_peer_dependencies: non_empty(transitive) })
}

/// The peers the importer's dependencies bind to something other than the
/// importer's own dependency of that name.
pub(super) fn importer_transitive_peers<'e>(
    id: &str,
    importer: &ProjectSnapshot,
    entry_of: &impl Fn(&PackageKey) -> Option<&'e NextSnapshotEntry>,
) -> Result<HashMap<PkgName, SnapshotDepRef>, NextFormatError> {
    let bindings = importer_dependencies(importer)
        .filter_map(|(alias, spec)| spec.version.resolved_key(alias).and_then(|key| entry_of(&key)))
        .flat_map(NextSnapshotEntry::bindings)
        .filter(|(name, reference)| !same_importer_target(importer, name, reference));
    let mut transitive: HashMap<PkgName, SnapshotDepRef> = HashMap::new();
    for (name, reference) in bindings {
        let reference = without_peer_bindings(reference);
        if let Some(previous) = transitive.insert(name.clone(), reference.clone())
            && previous != reference
        {
            return Err(NextFormatError::ConflictingBindings {
                referrer: format!("importer {id}"),
                name: name.to_string(),
            });
        }
    }
    Ok(transitive)
}

pub(in crate::next_format) fn importer_dependencies(
    importer: &ProjectSnapshot,
) -> impl Iterator<Item = (&PkgName, &ResolvedDependencySpec)> {
    [&importer.dependencies, &importer.dev_dependencies, &importer.optional_dependencies]
        .into_iter()
        .flatten()
        .flat_map(ResolvedDependencyMap::iter)
}

pub(super) fn same_importer_target(
    importer: &ProjectSnapshot,
    name: &PkgName,
    reference: &SnapshotDepRef,
) -> bool {
    importer_dependencies(importer)
        .find(|(alias, _)| *alias == name)
        .is_some_and(|(_, spec)| {
            let own = spec.version.resolved_key(name).map(|key| key.without_peer_bindings());
            own.is_some() && own == reference.resolve(name).map(|key| key.without_peer_bindings())
        })
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
