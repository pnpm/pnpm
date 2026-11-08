//! The next format's importers, with references without groups.

use super::{Explicit, snapshots::Snapshots};
use crate::{
    ImporterDepVersion, PkgName, PkgVerPeer, ProjectSnapshot, ResolvedDependencyMap,
    ResolvedDependencySpec,
    next_format::{NextImporter, NextImporterDep, NextImporterDeps, NextResolvedDep},
};
use std::collections::HashMap;

/// The importer with its versions' groups dropped. A reference the importer's
/// scope does not select names its snapshot by its bindings.
pub(super) fn next_importer(
    id: &str,
    importer: &ProjectSnapshot,
    explicit: &Explicit,
    snapshots: &Snapshots<'_>,
) -> NextImporter {
    let convert = |map: &Option<ResolvedDependencyMap>| -> Option<NextImporterDeps> {
        map.as_ref()
            .map(|map| {
                map.iter()
                    .map(|(alias, spec)| {
                        (alias.clone(), next_dep(id, alias, spec, explicit, snapshots))
                    })
                    .collect()
            })
    };
    NextImporter {
        dependencies: convert(&importer.dependencies),
        dev_dependencies: convert(&importer.dev_dependencies),
        optional_dependencies: convert(&importer.optional_dependencies),
        dependencies_meta: importer.dependencies_meta.clone(),
        publish_directory: importer.publish_directory.clone(),
        link_directory: importer.link_directory,
    }
}

fn next_dep(
    id: &str,
    alias: &PkgName,
    spec: &ResolvedDependencySpec,
    explicit: &Explicit,
    snapshots: &Snapshots<'_>,
) -> NextImporterDep {
    let peers = explicit
        .has_importer_reference(id, alias)
        .then(|| spec.version.resolved_key(alias))
        .flatten()
        .and_then(|target| snapshots.naming_bindings(&target));
    NextImporterDep::Resolved(Box::new(NextResolvedDep {
        specifier: spec.specifier.clone(),
        version: importer_version_without_groups(&spec.version),
        peers,
    }))
}

pub(in crate::next_format) fn importer_dependencies(
    importer: &ProjectSnapshot,
) -> impl Iterator<Item = (&PkgName, &ResolvedDependencySpec)> {
    [&importer.dependencies, &importer.dev_dependencies, &importer.optional_dependencies]
        .into_iter()
        .flatten()
        .flat_map(ResolvedDependencyMap::iter)
}

pub(super) fn importer_version_without_groups(version: &ImporterDepVersion) -> ImporterDepVersion {
    match version {
        ImporterDepVersion::Regular(version) => ImporterDepVersion::Regular(version.without_peer()),
        ImporterDepVersion::Alias(key) => ImporterDepVersion::Alias(key.without_peer()),
        ImporterDepVersion::File(payload) => {
            ImporterDepVersion::File(match format!("file:{payload}").parse::<PkgVerPeer>() {
                Ok(version) => version
                    .without_peer()
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
