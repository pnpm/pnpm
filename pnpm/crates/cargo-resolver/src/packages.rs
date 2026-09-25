use crate::{
    features::supports_features,
    model::{FeatureSelection, PackageKey, RegistryDependency},
    registry::{Registry, compatibility_line, matching_lines, matching_versions},
};
use miette::Result;
use pubgrub::SelectedDependencies;
use semver::{Version, VersionReq};

/// The solver package a requirement resolves against.
///
/// A requirement met on one compatibility line names that line's package
/// directly. One met on several names a [`PackageKey::Requirement`] package
/// standing for the choice between them, which is how the solver backtracks
/// from a line that cannot be satisfied to an older one, as `cargo` does.
/// A requirement nothing meets names a package with no versions at all.
///
/// The caller is responsible for [`Registry::validate_dependency_source`];
/// this only maps a name and a requirement onto a package.
pub(crate) fn package_key(
    registry: &Registry,
    dependency: &RegistryDependency,
) -> Result<PackageKey> {
    let name = dependency.name.as_str();
    let requirement = &dependency.requirement;
    let unsatisfiable = || PackageKey::Unsatisfiable {
        name: name.to_string(),
        requirement: requirement.to_string(),
    };
    let Some(versions) = registry.versions(name) else {
        return Ok(unsatisfiable());
    };
    let mut lines = matching_lines(versions, requirement);
    if lines.len() > 1 {
        return Ok(PackageKey::Requirement {
            name: name.to_string(),
            requirement: requirement.to_string(),
            default_features: dependency.default_features,
            features: dependency.features
                .iter()
                .cloned()
                .collect(),
        });
    }
    match lines.pop() {
        Some((compatibility, _)) => {
            Ok(PackageKey::Registry { name: name.to_string(), compatibility })
        }
        None => Ok(unsatisfiable()),
    }
}

/// The line package a requirement resolves to before there is a solution to
/// read the choice from: the newest line it is met on, which is the one the
/// solver reaches for first.
pub(crate) fn newest_line_package(
    registry: &Registry,
    dependency: &RegistryDependency,
) -> Result<Option<PackageKey>> {
    let Some(versions) = registry.versions(&dependency.name) else {
        return Ok(None);
    };
    let selection = dependency.feature_selection();
    Ok(matching_lines(versions, &dependency.requirement)
        .into_iter()
        .rfind(|(compatibility, _)| {
            admits_features(versions, &dependency.requirement, compatibility, &selection)
        })
        .map(|(compatibility, _)| PackageKey::Registry {
            name: dependency.name.clone(),
            compatibility,
        }))
}

/// Whether `compatibility` carries a version meeting `requirement` that
/// supports `selection`. A line that does not is not a line the dependency
/// asking for those features can settle on.
pub(crate) fn admits_features(
    versions: &[crate::model::RegistryVersion],
    requirement: &VersionReq,
    compatibility: &str,
    selection: &FeatureSelection,
) -> bool {
    matching_versions(versions, requirement)
        .filter(|version| compatibility_line(&version.version) == compatibility)
        .any(|version| supports_features(version, selection))
}

/// The line package a [`PackageKey::Requirement`] choice settled on, named
/// by the version the solver picked for it.
pub(crate) fn chosen_line(name: &str, representative: &Version) -> PackageKey {
    PackageKey::Registry {
        name: name.to_string(),
        compatibility: compatibility_line(representative),
    }
}

/// The registry package a requirement resolved to, reading the chosen
/// compatibility line out of `solution` when the requirement spans several.
/// `None` when the solution does not reach it.
pub(crate) fn selected_package(
    registry: &Registry,
    dependency: &RegistryDependency,
    solution: &SelectedDependencies<PackageKey, Version>,
) -> Result<Option<PackageKey>> {
    Ok(match package_key(registry, dependency)? {
        line @ PackageKey::Registry { .. } => Some(line),
        choice @ PackageKey::Requirement { .. } => solution
            .get(&choice)
            .map(|representative| chosen_line(&dependency.name, representative)),
        PackageKey::Root | PackageKey::Unsatisfiable { .. } => None,
    })
}
