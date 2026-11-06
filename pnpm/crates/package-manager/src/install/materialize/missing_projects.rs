//! Stand-in manifests for the projects a filtered install cannot read, such as
//! projects a sparse checkout leaves off disk.

use super::super::{DependencyGroup, HashSet, Lockfile, PackageManifest, Path};
use pnpm_lockfile::{ImporterDepVersion, ProjectSnapshot, ResolvedDependencySpec, VersionPart};
use serde_json::{Map, Value};

/// A manifest for every importer `lockfile` records outside
/// `real_importer_ids`, declaring the specifiers that importer records.
///
/// The resolver resolves these alongside the projects on disk, so their
/// dependencies take part in peer resolution and in the `optional` flags the
/// lockfile records, as they do when the projects are on disk. A dependency
/// recorded as a local path is left out: its target can be relative to a
/// directory that is not on disk.
pub(super) fn missing_project_manifests(
    lockfile: &Lockfile,
    real_importer_ids: &HashSet<String>,
    workspace_root: &Path,
) -> Vec<(String, PackageManifest)> {
    pnpm_deps_restorer::missing_project_importers(lockfile, real_importer_ids)
        .map(|(importer_id, importer)| {
            let manifest = PackageManifest::from_value(
                workspace_root.join(importer_id).join("package.json"),
                Value::Object(declared_dependencies(importer)),
            );
            (importer_id.clone(), manifest)
        })
        .collect()
}

/// Whether `dependency` is recorded as a local path, including an aliased
/// `<name>@file:` reference. The recorded reference decides it: a specifier
/// such as `repo.tgz` can also be a dist-tag.
fn is_local_dependency(dependency: &ResolvedDependencySpec) -> bool {
    match &dependency.version {
        ImporterDepVersion::Link(_) | ImporterDepVersion::File(_) => true,
        ImporterDepVersion::Alias(alias) => match alias.suffix.version() {
            VersionPart::File(_) => true,
            VersionPart::NonSemver(reference) => reference.starts_with("link:"),
            VersionPart::Semver(_) | VersionPart::RegistryQualified { .. } => false,
        },
        ImporterDepVersion::Regular(_) => false,
    }
}

fn declared_dependencies(importer: &ProjectSnapshot) -> Map<String, Value> {
    [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional]
        .into_iter()
        .filter_map(|group| {
            let specifiers: Map<String, Value> = importer
                .get_map_by_group(group)?
                .iter()
                .filter(|(_, dependency)| !is_local_dependency(dependency))
                .map(|(alias, dependency)| {
                    (alias.to_string(), Value::String(dependency.specifier.clone()))
                })
                .collect();
            let field: &'static str = group.into();
            (!specifiers.is_empty()).then(|| (field.to_string(), Value::Object(specifiers)))
        })
        .collect()
}

#[cfg(test)]
mod tests;
