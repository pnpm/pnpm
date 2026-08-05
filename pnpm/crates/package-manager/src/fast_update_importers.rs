use node_semver::Range;
use pacquet_lockfile::{Lockfile, PkgName, ProjectSnapshot, ResolvedDependencySpec};
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use std::collections::HashMap;

pub(crate) fn try_fast_update_importers(
    lockfile: &Lockfile,
    manifests: &[(String, &PackageManifest)],
) -> Option<Lockfile> {
    let mut candidate = lockfile.clone();
    let mut changed = false;
    for (importer_id, manifest) in manifests {
        let importer = candidate.importers.get_mut(importer_id)?;
        let manifest_specifiers = manifest
            .dependencies([DependencyGroup::Dev, DependencyGroup::Prod, DependencyGroup::Optional])
            .collect::<HashMap<_, _>>();
        for (alias, specifier) in manifest_specifiers {
            let alias = PkgName::parse(alias).ok()?;
            let dependency = importer_dependency_mut(importer, &alias)?;
            if dependency.specifier == specifier {
                continue;
            }
            let range = Range::parse(specifier).ok()?;
            let version = dependency.version.ver_peer()?.version_semver()?;
            if !version.satisfies(&range) {
                return None;
            }
            dependency.specifier = specifier.to_string();
            changed = true;
        }
    }
    changed.then_some(candidate)
}

fn importer_dependency_mut<'a>(
    importer: &'a mut ProjectSnapshot,
    alias: &PkgName,
) -> Option<&'a mut ResolvedDependencySpec> {
    [
        importer.optional_dependencies.as_mut(),
        importer.dependencies.as_mut(),
        importer.dev_dependencies.as_mut(),
    ]
    .into_iter()
    .find_map(|group| group.and_then(|dependencies| dependencies.get_mut(alias)))
}

#[cfg(test)]
mod tests;
