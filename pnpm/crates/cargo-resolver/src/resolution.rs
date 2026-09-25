use crate::{
    features::{
        feature_selections_for_solution, indexed_version, locked_dependencies,
        root_feature_selections, supports_features,
    },
    lockfile::lockfile_from_solution,
    metadata::{parse_metadata, root_dependencies},
    model::{FeatureSelection, PackageKey, RegistryDependency, RegistryVersion},
    packages::{chosen_line, package_key},
    registry::{Registry, compatibility_line, matching_lines, matching_versions},
};
use miette::{IntoDiagnostic, Result, WrapErr};
use pubgrub::{
    DefaultStringReporter, OfflineDependencyProvider, PubGrubError, Ranges, Reporter, resolve,
};
use semver::{Version, VersionReq};
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
    let mut discovery = IndexDiscovery::new(metadata, source)?;
    discovery.add_entries(index_files)?;
    discovery.missing_names()
}

/// Holds parsed registry state across discovery waves so the caller can
/// feed newly fetched index files incrementally instead of re-parsing the
/// full set each wave.
pub struct IndexDiscovery {
    root_dependencies: Vec<RegistryDependency>,
    registry: Registry,
}

impl IndexDiscovery {
    pub fn new(metadata: &str, source: &str) -> Result<Self> {
        let parsed = parse_metadata(metadata)?;
        let root_dependencies = root_dependencies(&parsed)?;
        Ok(Self { root_dependencies, registry: Registry::empty(source) })
    }

    pub fn add_entries(&mut self, index_files: &BTreeMap<String, String>) -> Result<()> {
        self.registry.add_entries(index_files)
    }

    pub fn missing_names(&self) -> Result<Vec<String>> {
        let mut pending = VecDeque::from(self.root_dependencies.clone());
        let mut discovered = BTreeMap::<PackageKey, Discovered>::new();
        let mut visited = BTreeSet::new();
        let mut missing = BTreeSet::new();

        while let Some(dependency) = pending.pop_front() {
            self.registry.validate_dependency_source(dependency.registry.as_deref())?;
            let visit_key = (
                dependency.name.clone(),
                dependency.requirement.to_string(),
                dependency.default_features,
                dependency.features.clone(),
            );
            if !visited.insert(visit_key) {
                continue;
            }
            let Some(versions) = self.registry.versions(&dependency.name) else {
                missing.insert(dependency.name);
                continue;
            };
            pending.extend(unified_dependencies(&mut discovered, &dependency, versions)?);
        }

        Ok(missing.into_iter().collect())
    }
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
    let mut reached = Vec::new();
    let selection = dependency.feature_selection();
    for (compatibility, _) in matching_lines(versions, &dependency.requirement) {
        // Only what this dependency could settle on: a version missing a
        // feature it asks for is not one it can select, even though another
        // dependency on the same line may select it.
        let selectable = matching_versions(versions, &dependency.requirement)
            .filter(|version| compatibility_line(&version.version) == compatibility)
            .filter(|version| supports_features(version, &selection))
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
        // The features of every dependency reaching the line, because one
        // may turn on a weak feature of another's. A version that has none
        // of them simply activates nothing extra.
        for version in versions
            .iter()
            .filter(|version| walk.contains(&version.version))
        {
            reached.extend(locked_dependencies(version, &entry.selection)?);
        }
    }
    Ok(reached)
}

/// Resolve Cargo registry dependencies and serialize a format-v4 `Cargo.lock`.
///
/// `source` identifies the registry the index files came from and is what a
/// dependency naming a registry is checked against. Build it with
/// [`crate::registry_source`]. It is not what the lockfile records: registry
/// crates are locked against [`crate::CRATES_IO_SOURCE`].
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
        let package = validated_package(registry, &dependency, solution, &mut validated)?;
        let Some(package) = package else { return Ok(None) };
        let Some(selected_version) = solution.get(&package) else { return Ok(None) };
        if !dependency.requirement.matches(selected_version) {
            return Ok(None);
        }
        if validated.contains_key(&package) {
            continue;
        }
        let selected = offered_version(registry, &dependency.name, selected_version)?;
        let selection = feature_selections
            .get(&package)
            .cloned()
            .unwrap_or_default();
        if !supports_features(selected, &selection) {
            return Ok(None);
        }
        validated.insert(package, selected_version.clone());
        pending.extend(locked_dependencies(selected, &selection)?);
    }

    Ok(Some(validated.into_iter().collect()))
}

/// The line package `dependency` settled on.
///
/// A requirement spanning several compatibility lines resolves through its
/// choice package, whose entry is recorded in `validated` so the lockfile
/// can read the chosen line back out.
/// The index entry for a version the solver selected. Only versions the
/// index still offers are registered, so a yanked one means the index the
/// solution was built against is not the one being read.
fn offered_version<'v>(
    registry: &'v Registry,
    name: &str,
    version: &Version,
) -> Result<&'v RegistryVersion> {
    let offered = indexed_version(registry.package(name)?, name, version)?;
    if offered.yanked {
        return Err(miette::miette!("selected {name} {version} is yanked"));
    }
    Ok(offered)
}

fn validated_package(
    registry: &Registry,
    dependency: &RegistryDependency,
    solution: &pubgrub::SelectedDependencies<PackageKey, Version>,
    validated: &mut BTreeMap<PackageKey, Version>,
) -> Result<Option<PackageKey>> {
    registry.validate_dependency_source(dependency.registry.as_deref())?;
    Ok(match package_key(registry, dependency)? {
        line @ PackageKey::Registry { .. } => Some(line),
        choice @ PackageKey::Requirement { .. } => solution
            .get(&choice)
            .map(|representative| {
                let line = chosen_line(&dependency.name, representative);
                validated.insert(choice.clone(), representative.clone());
                line
            }),
        PackageKey::Root | PackageKey::Unsatisfiable { .. } => None,
    })
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
        register(registry, &package, feature_selections, &mut provider, &mut pending)?;
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

/// Tell the solver what `package` offers, which depends on what kind of
/// package it is.
fn register(
    registry: &Registry,
    package: &PackageKey,
    feature_selections: &BTreeMap<PackageKey, FeatureSelection>,
    provider: &mut OfflineDependencyProvider<PackageKey, Ranges<Version>>,
    pending: &mut VecDeque<PackageKey>,
) -> Result<()> {
    match package {
        PackageKey::Registry { .. } => {
            register_candidates(registry, package, feature_selections, provider, pending)
        }
        PackageKey::Requirement { .. } => {
            register_compatibility_lines(registry, package, provider, pending)
        }
        PackageKey::Root | PackageKey::Unsatisfiable { .. } => Ok(()),
    }
}

/// Offer the solver the versions of `package` that every dependency able to
/// settle on it can support, queueing each one's own dependencies.
fn register_candidates(
    registry: &Registry,
    package: &PackageKey,
    feature_selections: &BTreeMap<PackageKey, FeatureSelection>,
    provider: &mut OfflineDependencyProvider<PackageKey, Ranges<Version>>,
    pending: &mut VecDeque<PackageKey>,
) -> Result<()> {
    let PackageKey::Registry { name, compatibility } = package else {
        return Ok(());
    };
    let resolved = feature_selections
        .get(package)
        .cloned()
        .unwrap_or_default();
    let versions = registry.package(name)?;
    let candidates = versions
        .iter()
        .filter(|version| {
            !version.yanked && compatibility_line(&version.version) == *compatibility
        });
    for version in candidates {
        let dependencies = locked_dependencies(version, &resolved)?;
        let constraints = constraints_for(registry, &dependencies, pending)?;
        provider.add_dependencies(package.clone(), version.version.clone(), constraints);
    }
    Ok(())
}

/// Offer the solver one version per compatibility line the requirement is
/// met on, each standing for that line and depending on the versions it
/// admits there. Ordered by version, so the solver reaches for the newest
/// line first and backtracks to an older one, as `cargo` does.
///
/// A line is offered only the versions that support what the lines's
/// dependants ask, so a line that cannot support them is not a choice at
/// all rather than one the solver takes and later has to leave.
fn register_compatibility_lines(
    registry: &Registry,
    package: &PackageKey,
    provider: &mut OfflineDependencyProvider<PackageKey, Ranges<Version>>,
    pending: &mut VecDeque<PackageKey>,
) -> Result<()> {
    let PackageKey::Requirement {
        name,
        requirement,
        default_features,
        features,
    } = package
    else {
        return Ok(());
    };
    let requested = FeatureSelection {
        default_features: *default_features,
        features: features.iter().cloned().collect(),
    };
    let requirement = VersionReq::parse(requirement)
        .into_diagnostic()
        .wrap_err_with(|| format!("parse requirement for {name}"))?;
    let versions = registry.package(name)?;
    for (compatibility, representative) in matching_lines(versions, &requirement) {
        let line =
            PackageKey::Registry { name: name.clone(), compatibility: compatibility.clone() };
        let admitted = matching_versions(versions, &requirement)
            .filter(|version| compatibility_line(&version.version) == compatibility)
            .filter(|version| supports_features(version, &requested))
            .fold(Ranges::empty(), |range, version| {
                range.union(&Ranges::singleton(version.version.clone()))
            });
        if admitted == Ranges::empty() {
            continue;
        }
        provider.add_dependencies(package.clone(), representative, [(line.clone(), admitted)]);
        pending.push_back(line);
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
        registry.validate_dependency_source(dependency.registry.as_deref())?;
        let package = package_key(registry, dependency)?;
        // A version missing a feature this dependency asks for is not one it
        // can settle on, which is how a requirement reaches past a line to an
        // older one.
        let selection = dependency.feature_selection();
        let allowed = if let PackageKey::Registry { compatibility, .. } = &package {
            matching_versions(registry.package(&dependency.name)?, &dependency.requirement)
                .filter(|version| compatibility_line(&version.version) == *compatibility)
                .filter(|version| supports_features(version, &selection))
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
