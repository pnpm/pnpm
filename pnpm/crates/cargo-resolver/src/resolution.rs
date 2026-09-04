use crate::{
    features::{active_dependencies, collect_feature_selections},
    lockfile::lockfile_from_solution,
    metadata::{parse_metadata, root_dependencies},
    model::{PackageKey, RegistryDependency},
    registry::{Registry, compatibility_line, matching_versions, validate_registry},
};
use miette::Result;
use pubgrub::{
    DefaultStringReporter, OfflineDependencyProvider, PubGrubError, Ranges, Reporter, resolve,
};
use semver::Version;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// Return sparse-index package names still needed to resolve `metadata`.
pub fn missing_index_names(
    metadata: &str,
    index_files: &BTreeMap<String, String>,
) -> Result<Vec<String>> {
    let metadata = parse_metadata(metadata)?;
    let registry = Registry::new(index_files)?;
    let mut pending = VecDeque::from(root_dependencies(&metadata)?);
    let mut visited = BTreeSet::new();
    let mut missing = BTreeSet::new();

    while let Some(dependency) = pending.pop_front() {
        validate_registry(dependency.registry.as_deref())?;
        let visit_key = (
            dependency.name.clone(),
            dependency.requirement.to_string(),
            dependency.default_features,
            dependency.features.clone(),
        );
        if !visited.insert(visit_key) {
            continue;
        }
        let Some(versions) = registry.versions(&dependency.name) else {
            missing.insert(dependency.name);
            continue;
        };
        for version in matching_versions(versions, &dependency.requirement) {
            pending.extend(active_dependencies(version, &dependency.feature_selection())?);
        }
    }

    Ok(missing.into_iter().collect())
}

/// Resolve Cargo registry dependencies and serialize a format-v4 `Cargo.lock`.
pub fn resolve_lockfile(metadata: &str, index_files: &BTreeMap<String, String>) -> Result<String> {
    let metadata = parse_metadata(metadata)?;
    let registry = Registry::new(index_files)?;
    let root_dependencies = root_dependencies(&metadata)?;
    let feature_selections = collect_feature_selections(&registry, &root_dependencies)?;
    let mut provider = OfflineDependencyProvider::<PackageKey, Ranges<Version>>::new();
    let mut pending = VecDeque::new();
    let root_constraints = constraints_for(&registry, &root_dependencies, &mut pending)?;
    provider.add_dependencies(PackageKey::Root, Version::new(0, 0, 0), root_constraints);

    let mut registered = BTreeSet::new();
    while let Some(package) = pending.pop_front() {
        if !registered.insert(package.clone()) {
            continue;
        }
        let PackageKey::Registry { name, compatibility } = &package else {
            continue;
        };
        let versions = registry.package(name)?;
        let selection = feature_selections.get(&package).cloned().unwrap_or_default();
        for version in versions.iter().filter(|version| {
            !version.yanked && compatibility_line(&version.version) == *compatibility
        }) {
            let dependencies = active_dependencies(version, &selection)?;
            if dependencies.iter().any(|dependency| registry.versions(&dependency.name).is_none()) {
                continue;
            }
            let constraints = constraints_for(&registry, &dependencies, &mut pending)?;
            provider.add_dependencies(package.clone(), version.version.clone(), constraints);
        }
    }

    let solution = match resolve(&provider, PackageKey::Root, Version::new(0, 0, 0)) {
        Ok(solution) => solution,
        Err(PubGrubError::NoSolution(mut tree)) => {
            tree.collapse_no_versions();
            let report = DefaultStringReporter::report(&tree);
            return Err(miette::miette!(report));
        }
        Err(error) => {
            let message = error.to_string();
            return Err(miette::miette!(message));
        }
    };
    lockfile_from_solution(&metadata, &registry, &solution, &feature_selections)
}

fn constraints_for(
    registry: &Registry,
    dependencies: &[RegistryDependency],
    pending: &mut VecDeque<PackageKey>,
) -> Result<Vec<(PackageKey, Ranges<Version>)>> {
    let mut constraints = BTreeMap::<PackageKey, Ranges<Version>>::new();
    for dependency in dependencies {
        validate_registry(dependency.registry.as_deref())?;
        let versions = registry.package(&dependency.name)?;
        let compatibility = matching_versions(versions, &dependency.requirement)
            .next_back()
            .map(|version| compatibility_line(&version.version))
            .ok_or_else(|| {
                miette::miette!(
                    "no non-yanked version of {} satisfies {}",
                    dependency.name,
                    dependency.requirement,
                )
            })?;
        let package = PackageKey::Registry {
            name: dependency.name.clone(),
            compatibility: compatibility.clone(),
        };
        let allowed = matching_versions(versions, &dependency.requirement)
            .filter(|version| compatibility_line(&version.version) == compatibility)
            .fold(Ranges::empty(), |range, version| {
                range.union(&Ranges::singleton(version.version.clone()))
            });
        constraints
            .entry(package.clone())
            .and_modify(|range| *range = range.intersection(&allowed))
            .or_insert(allowed);
        pending.push_back(package);
    }
    Ok(constraints.into_iter().collect())
}
