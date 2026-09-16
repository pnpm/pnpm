use crate::{
    features::{
        active_dependencies, feature_selections_for_solution, root_feature_selections,
        supports_features,
    },
    lockfile::lockfile_from_solution,
    metadata::{parse_metadata, root_dependencies},
    model::{FeatureSelection, PackageKey, RegistryDependency, RegistryVersion},
    registry::{Registry, compatibility_line, matching_versions, newest_compatibility},
};
use miette::Result;
use pubgrub::{
    DefaultStringReporter, OfflineDependencyProvider, PubGrubError, Ranges, Reporter, resolve,
};
use semver::Version;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// What discovery has reached a crate with so far: the features every
/// dependency edge has asked of it together, and the versions those edges
/// can select.
#[derive(Default)]
struct Discovered {
    selection: FeatureSelection,
    versions: BTreeSet<Version>,
}

/// Return sparse-index package names still needed to resolve `metadata`
/// against the registry identified by `source`.
///
/// Edges reaching the same package are unified the way resolution unifies
/// them, and a package is walked again whenever that union grows. Feature
/// activation is not monotone across edges, so a union can reach a crate no
/// single edge reaches.
pub fn missing_index_names(
    metadata: &str,
    index_files: &BTreeMap<String, String>,
    source: &str,
) -> Result<Vec<String>> {
    let metadata = parse_metadata(metadata)?;
    let registry = Registry::new(index_files, source)?;
    let mut pending = VecDeque::from(root_dependencies(&metadata)?);
    let mut discovered = BTreeMap::<PackageKey, Discovered>::new();
    let mut visited = BTreeSet::new();
    let mut missing = BTreeSet::new();

    while let Some(dependency) = pending.pop_front() {
        registry.validate_dependency_source(dependency.registry.as_deref())?;
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
        pending.extend(unified_dependencies(&mut discovered, &dependency, versions)?);
    }

    Ok(missing.into_iter().collect())
}

/// Fold what `dependency` asks of its package into what discovery already
/// knows, and return the dependencies of every version that now needs
/// walking: the ones this edge brings into range, or all of them when the
/// unified feature selection grew.
fn unified_dependencies(
    discovered: &mut BTreeMap<PackageKey, Discovered>,
    dependency: &RegistryDependency,
    versions: &[RegistryVersion],
) -> Result<Vec<RegistryDependency>> {
    let Some(compatibility) = newest_compatibility(versions, &dependency.requirement) else {
        return Ok(Vec::new());
    };
    let selectable = matching_versions(versions, &dependency.requirement)
        .filter(|version| compatibility_line(&version.version) == compatibility)
        .map(|version| version.version.clone())
        .collect::<BTreeSet<_>>();
    let package = PackageKey::Registry { name: dependency.name.clone(), compatibility };
    let entry = discovered.entry(package).or_default();
    let previous = entry.selection.clone();
    entry.selection.default_features |= dependency.default_features;
    entry.selection.features.extend(dependency.features.iter().cloned());
    let unwalked = &selectable - &entry.versions;
    entry.versions.extend(selectable);
    let walk = if entry.selection == previous { unwalked } else { entry.versions.clone() };
    let mut reached = Vec::new();
    for version in versions
        .iter()
        .filter(|version| walk.contains(&version.version))
        .filter(|version| supports_features(version, &entry.selection))
    {
        reached.extend(active_dependencies(version, &entry.selection)?);
    }
    Ok(reached)
}

/// Resolve Cargo registry dependencies and serialize a format-v4 `Cargo.lock`.
///
/// `source` identifies the registry the index files came from and is what the
/// resolved packages are recorded under. Build it with
/// [`crate::registry_source`].
pub fn resolve_lockfile(
    metadata: &str,
    index_files: &BTreeMap<String, String>,
    source: &str,
) -> Result<String> {
    let metadata = parse_metadata(metadata)?;
    let registry = Registry::new(index_files, source)?;
    let root_dependencies = root_dependencies(&metadata)?;
    let mut feature_selections = root_feature_selections(&registry, &root_dependencies)?;
    let mut previous_selections = Vec::new();

    loop {
        if previous_selections.contains(&feature_selections) {
            return Err(miette::miette!("Cargo feature resolution did not converge"));
        }
        previous_selections.push(feature_selections.clone());
        let solution = resolve_with_features(&registry, &root_dependencies, &feature_selections)?;
        let selected_features =
            feature_selections_for_solution(&registry, &root_dependencies, &solution)?;
        if let Some(validated_solution) =
            validate_selected_graph(&registry, &root_dependencies, &solution, &selected_features)?
        {
            return lockfile_from_solution(
                &metadata,
                &registry,
                &validated_solution,
                &selected_features,
                source,
            );
        }
        feature_selections = selected_features;
    }
}

fn validate_selected_graph(
    registry: &Registry,
    root_dependencies: &[RegistryDependency],
    solution: &pubgrub::SelectedDependencies<PackageKey, Version>,
    feature_selections: &BTreeMap<PackageKey, FeatureSelection>,
) -> Result<Option<pubgrub::SelectedDependencies<PackageKey, Version>>> {
    let Some(root_version) = solution.get(&PackageKey::Root) else { return Ok(None) };
    let mut validated = BTreeMap::from([(PackageKey::Root, root_version.clone())]);
    let mut pending = VecDeque::from(root_dependencies.to_vec());

    while let Some(dependency) = pending.pop_front() {
        let package = package_key(registry, &dependency)?;
        let Some(selected_version) = solution.get(&package) else { return Ok(None) };
        if !dependency.requirement.matches(selected_version) {
            return Ok(None);
        }
        if validated.contains_key(&package) {
            continue;
        }
        let selected = registry
            .package(&dependency.name)?
            .iter()
            .find(|candidate| !candidate.yanked && candidate.version == *selected_version)
            .ok_or_else(|| {
                miette::miette!(
                    "selected {} {} is absent from the index",
                    dependency.name,
                    selected_version,
                )
            })?;
        let selection = feature_selections
            .get(&package)
            .cloned()
            .unwrap_or_default();
        if !supports_features(selected, &selection) {
            return Ok(None);
        }
        validated.insert(package, selected_version.clone());
        pending.extend(active_dependencies(selected, &selection)?);
    }

    Ok(Some(validated.into_iter().collect()))
}

fn resolve_with_features(
    registry: &Registry,
    root_dependencies: &[RegistryDependency],
    feature_selections: &BTreeMap<PackageKey, FeatureSelection>,
) -> Result<pubgrub::SelectedDependencies<PackageKey, Version>> {
    let mut provider = OfflineDependencyProvider::<PackageKey, Ranges<Version>>::new();
    let mut pending = VecDeque::new();
    let root_constraints = constraints_for(registry, root_dependencies, &mut pending)?;
    provider.add_dependencies(PackageKey::Root, Version::new(0, 0, 0), root_constraints);

    let mut registered = BTreeSet::new();
    while let Some(package) = pending.pop_front() {
        if !registered.insert(package.clone()) {
            continue;
        }
        let selection = feature_selections
            .get(&package)
            .cloned()
            .unwrap_or_default();
        register_candidates(registry, &package, &selection, &mut provider, &mut pending)?;
    }

    match resolve(&provider, PackageKey::Root, Version::new(0, 0, 0)) {
        Ok(solution) => Ok(solution),
        Err(PubGrubError::NoSolution(mut tree)) => {
            tree.collapse_no_versions();
            let report = DefaultStringReporter::report(&tree);
            Err(miette::miette!(report))
        }
        Err(error) => {
            let message = error.to_string();
            Err(miette::miette!(message))
        }
    }
}

/// Offer the solver every version of `package` that the selected features
/// admit, queueing each one's own dependencies.
fn register_candidates(
    registry: &Registry,
    package: &PackageKey,
    selection: &FeatureSelection,
    provider: &mut OfflineDependencyProvider<PackageKey, Ranges<Version>>,
    pending: &mut VecDeque<PackageKey>,
) -> Result<()> {
    let PackageKey::Registry { name, compatibility } = package else {
        return Ok(());
    };
    let versions = registry.package(name)?;
    let candidates = versions
        .iter()
        .filter(|version| {
            !version.yanked && compatibility_line(&version.version) == *compatibility
        });
    for version in candidates {
        if !supports_features(version, selection) {
            continue;
        }
        let dependencies = active_dependencies(version, selection)?;
        let constraints = constraints_for(registry, &dependencies, pending)?;
        provider.add_dependencies(package.clone(), version.version.clone(), constraints);
    }
    Ok(())
}

fn constraints_for(
    registry: &Registry,
    dependencies: &[RegistryDependency],
    pending: &mut VecDeque<PackageKey>,
) -> Result<Vec<(PackageKey, Ranges<Version>)>> {
    let mut constraints = BTreeMap::<PackageKey, Ranges<Version>>::new();
    for dependency in dependencies {
        let package = package_key(registry, dependency)?;
        let allowed = if let PackageKey::Registry { compatibility, .. } = &package {
            matching_versions(registry.package(&dependency.name)?, &dependency.requirement)
                .filter(|version| compatibility_line(&version.version) == *compatibility)
                .fold(Ranges::empty(), |range, version| {
                    range.union(&Ranges::singleton(version.version.clone()))
                })
        } else {
            Ranges::full()
        };
        constraints
            .entry(package.clone())
            .and_modify(|range| *range = range.intersection(&allowed))
            .or_insert(allowed);
        pending.push_back(package);
    }
    Ok(constraints.into_iter().collect())
}

/// The solver package `dependency` resolves against.
///
/// A dependency the index cannot meet still gets a package of its own, one
/// the solver finds no version under. That keeps a dead-end candidate a
/// plain incompatibility the solver backtracks over and can name in its
/// report, rather than a failure of the whole resolution.
fn package_key(registry: &Registry, dependency: &RegistryDependency) -> Result<PackageKey> {
    registry.validate_dependency_source(dependency.registry.as_deref())?;
    let compatibility = registry
        .versions(&dependency.name)
        .and_then(|versions| newest_compatibility(versions, &dependency.requirement));
    Ok(match compatibility {
        Some(compatibility) => {
            PackageKey::Registry { name: dependency.name.clone(), compatibility }
        }
        None => PackageKey::Unsatisfiable {
            name: dependency.name.clone(),
            requirement: dependency.requirement.to_string(),
        },
    })
}
