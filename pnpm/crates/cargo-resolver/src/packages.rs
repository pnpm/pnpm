use crate::{
    model::PackageKey,
    registry::{Registry, compatibility_line, matching_lines},
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
    name: &str,
    requirement: &VersionReq,
) -> Result<PackageKey> {
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
    name: &str,
    requirement: &VersionReq,
) -> Result<Option<PackageKey>> {
    let Some(versions) = registry.versions(name) else {
        return Ok(None);
    };
    Ok(matching_lines(versions, requirement)
        .pop()
        .map(|(compatibility, _)| PackageKey::Registry { name: name.to_string(), compatibility }))
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
    name: &str,
    requirement: &VersionReq,
    solution: &SelectedDependencies<PackageKey, Version>,
) -> Result<Option<PackageKey>> {
    Ok(match package_key(registry, name, requirement)? {
        line @ PackageKey::Registry { .. } => Some(line),
        choice @ PackageKey::Requirement { .. } => {
            solution.get(&choice).map(|representative| chosen_line(name, representative))
        }
        PackageKey::Root | PackageKey::Unsatisfiable { .. } => None,
    })
}
