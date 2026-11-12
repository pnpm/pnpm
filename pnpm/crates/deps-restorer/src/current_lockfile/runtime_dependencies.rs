use crate::SkippedSnapshots;
use pnpm_lockfile::{ImporterDepVersion, Lockfile, PackageKey, Prefix};
use pnpm_modules_yaml::IncludedDependencies;
use pnpm_package_manifest::DependencyGroup;

#[must_use]
pub fn runtime_dependency_alias(
    lockfile: &Lockfile,
    included: IncludedDependencies,
    skipped: &SkippedSnapshots,
) -> Option<String> {
    importer_runtime_dependency_alias(lockfile, included, skipped)
        .or_else(|| snapshot_runtime_dependency_alias(lockfile, included, skipped))
}

fn importer_runtime_dependency_alias(
    lockfile: &Lockfile,
    included: IncludedDependencies,
    skipped: &SkippedSnapshots,
) -> Option<String> {
    lockfile.importers
        .values()
        .flat_map(|importer| {
            importer.dependencies_by_groups(
                [
                    included.dependencies.then_some(DependencyGroup::Prod),
                    included.dev_dependencies.then_some(DependencyGroup::Dev),
                    included
                        .includes_project_optional_dependencies()
                        .then_some(DependencyGroup::Optional),
                ]
                .into_iter()
                .flatten(),
            )
        })
        .find_map(|(alias, dependency)| {
            let key = match &dependency.version {
                ImporterDepVersion::Regular(version) if version.prefix() == Prefix::Runtime => {
                    PackageKey::new(alias.clone(), version.clone())
                }
                ImporterDepVersion::Alias(version)
                    if version.suffix.prefix() == Prefix::Runtime =>
                {
                    PackageKey::new(version.name.clone(), version.suffix.clone())
                }
                ImporterDepVersion::Regular(_)
                | ImporterDepVersion::Alias(_)
                | ImporterDepVersion::Link(_)
                | ImporterDepVersion::File(_) => return None,
            };
            (!skipped.contains(&key)).then(|| alias.to_string())
        })
}

fn snapshot_runtime_dependency_alias(
    lockfile: &Lockfile,
    included: IncludedDependencies,
    skipped: &SkippedSnapshots,
) -> Option<String> {
    lockfile.snapshots
        .iter()
        .flat_map(|snapshots| snapshots.iter())
        .filter(|(key, _)| !skipped.contains(key))
        .map(|(_, snapshot)| snapshot)
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
            let target = dependency.resolve(alias)?;
            let runtime = target.suffix.prefix() == Prefix::Runtime;
            let key = PackageKey::new(target.name, target.suffix);
            (runtime && !skipped.contains(&key)).then(|| alias.to_string())
        })
}
