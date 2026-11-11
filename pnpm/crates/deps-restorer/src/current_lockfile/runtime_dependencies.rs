use super::MaterializationClosure;
use pnpm_lockfile::{ImporterDepVersion, Prefix};
use pnpm_modules_yaml::IncludedDependencies;
use pnpm_package_manifest::DependencyGroup;

impl MaterializationClosure {
    #[must_use]
    pub fn runtime_dependency_alias(&self, included: IncludedDependencies) -> Option<String> {
        importer_runtime_dependency_alias(self, included)
            .or_else(|| snapshot_runtime_dependency_alias(self, included))
    }
}

fn importer_runtime_dependency_alias(
    closure: &MaterializationClosure,
    included: IncludedDependencies,
) -> Option<String> {
    closure.importer_ids
        .iter()
        .filter_map(|importer_id| closure.lockfile.importers.get(importer_id))
        .flat_map(|importer| {
            importer.dependencies_by_groups(
                [
                    included.dependencies.then_some(DependencyGroup::Prod),
                    included.dev_dependencies.then_some(DependencyGroup::Dev),
                    included.optional_dependencies.then_some(DependencyGroup::Optional),
                ]
                .into_iter()
                .flatten(),
            )
        })
        .find_map(|(alias, dependency)| {
            let runtime = match &dependency.version {
                ImporterDepVersion::Regular(version) => version.prefix() == Prefix::Runtime,
                ImporterDepVersion::Alias(version) => version.suffix.prefix() == Prefix::Runtime,
                ImporterDepVersion::Link(_) | ImporterDepVersion::File(_) => false,
            };
            runtime.then(|| alias.to_string())
        })
}

fn snapshot_runtime_dependency_alias(
    closure: &MaterializationClosure,
    included: IncludedDependencies,
) -> Option<String> {
    closure.lockfile.snapshots
        .iter()
        .flat_map(|snapshots| snapshots.values())
        .flat_map(|snapshot| {
            snapshot.dependencies
                .iter()
                .chain(
                    included.optional_dependencies
                        .then_some(snapshot.optional_dependencies.as_ref())
                        .flatten(),
                )
        })
        .flat_map(|dependencies| dependencies.iter())
        .find_map(|(alias, dependency)| {
            dependency
                .ver_peer()
                .is_some_and(|version| version.prefix() == Prefix::Runtime)
                .then(|| alias.to_string())
        })
}
