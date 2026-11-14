use crate::{
    features::{
        feature_selections_for_solution, indexed_version, locked_dependencies,
        root_feature_selections, supports_features,
    },
    lockfile::lockfile_from_solution,
    metadata::{parse_metadata, root_dependencies},
    model::{
        CancelError, FeatureSelection, PackageKey, RegistryDependency, RegistryVersion,
        ensure_not_cancelled,
    },
    packages::{chosen_line, package_key_with_preferences},
    preferences::{PreferredVersions, contains as is_preferred, parse as parse_preferred_versions},
    registry::{Registry, compatibility_line},
};
use miette::Result;
use pubgrub::{
    DefaultStringReporter, Dependencies, DependencyProvider, OfflineDependencyProvider,
    PackageResolutionStatistics, PubGrubError, Ranges, Reporter, resolve,
};
use semver::{Version, VersionReq};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

mod registration;
use registration::{Registration, constraints_for};

type InnerProvider = OfflineDependencyProvider<PackageKey, Ranges<Version>>;

struct CancellableProvider<'a> {
    inner: InnerProvider,
    preferred_versions: &'a PreferredVersions,
    should_cancel: &'a (dyn Fn() -> bool + Send + Sync),
}

impl CancellableProvider<'_> {
    fn preferred_version(&self, package: &PackageKey, range: &Ranges<Version>) -> Option<Version> {
        let name = package_name(package)?;
        let preferred_versions = self.preferred_versions.get(name)?;
        let mut versions = self.inner.versions(package)?;
        match package {
            PackageKey::Registry { .. } => preferred_versions
                .iter()
                .find(|version| {
                    range.contains(version) && versions.any(|candidate| candidate == *version)
                })
                .cloned(),
            PackageKey::Requirement { requirement, .. } => {
                let requirement = VersionReq::parse(requirement).ok()?;
                preferred_versions
                    .iter()
                    .find_map(|preferred| {
                        if !requirement.matches(preferred) {
                            return None;
                        }
                        versions
                            .find(|representative| {
                                range.contains(representative)
                                    && compatibility_line(representative)
                                        == compatibility_line(preferred)
                            })
                            .cloned()
                    })
            }
            PackageKey::Root | PackageKey::Unsatisfiable { .. } => None,
        }
    }
}

impl DependencyProvider for CancellableProvider<'_> {
    type P = PackageKey;
    type V = Version;
    type VS = Ranges<Version>;
    type Priority = <InnerProvider as DependencyProvider>::Priority;
    type M = <InnerProvider as DependencyProvider>::M;
    type Err = CancelError;

    fn prioritize(
        &self,
        package: &Self::P,
        range: &Self::VS,
        package_conflicts_counts: &PackageResolutionStatistics,
    ) -> Self::Priority {
        self.inner.prioritize(package, range, package_conflicts_counts)
    }

    fn choose_version(
        &self,
        package: &Self::P,
        range: &Self::VS,
    ) -> std::result::Result<Option<Self::V>, Self::Err> {
        if let Some(preferred) = self.preferred_version(package, range) {
            return Ok(Some(preferred));
        }
        match self.inner.choose_version(package, range) {
            Ok(version) => Ok(version),
        }
    }

    fn get_dependencies(
        &self,
        package: &Self::P,
        version: &Self::V,
    ) -> std::result::Result<Dependencies<Self::P, Self::VS, Self::M>, Self::Err> {
        match self.inner.get_dependencies(package, version) {
            Ok(dependencies) => Ok(dependencies),
        }
    }

    fn should_cancel(&self) -> std::result::Result<(), Self::Err> {
        if (self.should_cancel)() { Err(CancelError) } else { Ok(()) }
    }
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
    resolve_lockfile_with_preferences(metadata, index_files, source, &BTreeMap::new())
}

pub fn resolve_lockfile_with_preferences(
    metadata: &str,
    index_files: &BTreeMap<String, String>,
    source: &str,
    preferred_versions: &BTreeMap<String, Vec<String>>,
) -> Result<String> {
    resolve_lockfile_until_with_preferences(
        metadata,
        index_files,
        source,
        preferred_versions,
        &|| false,
    )
}

/// [`resolve_lockfile`] with cooperative cancellation checked across package
/// registration, solver steps, and feature validation.
pub fn resolve_lockfile_until(
    metadata: &str,
    index_files: &BTreeMap<String, String>,
    source: &str,
    should_cancel: &(dyn Fn() -> bool + Send + Sync),
) -> Result<String> {
    resolve_lockfile_until_with_preferences(
        metadata,
        index_files,
        source,
        &BTreeMap::new(),
        should_cancel,
    )
}

pub fn resolve_lockfile_until_with_preferences(
    metadata: &str,
    index_files: &BTreeMap<String, String>,
    source: &str,
    preferred_versions: &BTreeMap<String, Vec<String>>,
    should_cancel: &(dyn Fn() -> bool + Send + Sync),
) -> Result<String> {
    let metadata = parse_metadata(metadata)?;
    let registry = Registry::new_until(index_files, source, should_cancel)?;
    let preferred_versions = parse_preferred_versions(preferred_versions)?;
    let root_dependencies = root_dependencies(&metadata)?;
    let mut feature_selections =
        root_feature_selections(&registry, &root_dependencies, &preferred_versions)?;
    let mut previous_selections = Vec::new();

    loop {
        ensure_not_cancelled(should_cancel)?;
        if previous_selections.contains(&feature_selections) {
            return Err(miette::miette!("Cargo feature resolution did not converge"));
        }
        previous_selections.push(feature_selections.clone());
        let solution = resolve_with_features(
            &registry,
            &root_dependencies,
            &feature_selections,
            &preferred_versions,
            should_cancel,
        )?;
        let selected_features = feature_selections_for_solution(
            &registry,
            &root_dependencies,
            &solution,
            &preferred_versions,
        )?;
        if let Some(validated_solution) = validate_selected_graph(
            &registry,
            &root_dependencies,
            &solution,
            &selected_features,
            &preferred_versions,
            should_cancel,
        )? {
            return lockfile_from_solution(
                &metadata,
                &registry,
                &validated_solution,
                &selected_features,
                source,
                &preferred_versions,
            );
        }
        feature_selections = selected_features;
    }
}

fn package_name(package: &PackageKey) -> Option<&str> {
    match package {
        PackageKey::Registry { name, .. }
        | PackageKey::Requirement { name, .. }
        | PackageKey::Unsatisfiable { name, .. } => Some(name),
        PackageKey::Root => None,
    }
}

fn validate_selected_graph(
    registry: &Registry,
    root_dependencies: &[RegistryDependency],
    solution: &pubgrub::SelectedDependencies<PackageKey, Version>,
    feature_selections: &BTreeMap<PackageKey, FeatureSelection>,
    preferred_versions: &PreferredVersions,
    should_cancel: &(dyn Fn() -> bool + Send + Sync),
) -> Result<Option<pubgrub::SelectedDependencies<PackageKey, Version>>> {
    let Some(root_version) = solution.get(&PackageKey::Root) else { return Ok(None) };
    let mut validated = BTreeMap::from([(PackageKey::Root, root_version.clone())]);
    let mut pending = VecDeque::from(root_dependencies.to_vec());

    while let Some(dependency) = pending.pop_front() {
        ensure_not_cancelled(should_cancel)?;
        let package =
            validated_package(registry, &dependency, solution, preferred_versions, &mut validated)?;
        let Some(package) = package else { return Ok(None) };
        let Some(selected_version) = solution.get(&package) else { return Ok(None) };
        if !dependency.requirement.matches(selected_version) {
            return Ok(None);
        }
        if validated.contains_key(&package) {
            continue;
        }
        let selected =
            offered_version(registry, &dependency.name, selected_version, preferred_versions)?;
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

/// The index entry for a version the solver selected. Only versions the
/// index still offers are registered, so a yanked one means the index the
/// solution was built against is not the one being read.
fn offered_version<'v>(
    registry: &'v Registry,
    name: &str,
    version: &Version,
    preferred_versions: &BTreeMap<String, Vec<Version>>,
) -> Result<&'v RegistryVersion> {
    let offered = indexed_version(registry.package(name)?, name, version)?;
    if offered.yanked && !is_preferred(preferred_versions, name, version) {
        return Err(miette::miette!("selected {name} {version} is yanked"));
    }
    Ok(offered)
}

fn validated_package(
    registry: &Registry,
    dependency: &RegistryDependency,
    solution: &pubgrub::SelectedDependencies<PackageKey, Version>,
    preferred_versions: &PreferredVersions,
    validated: &mut BTreeMap<PackageKey, Version>,
) -> Result<Option<PackageKey>> {
    registry.validate_dependency_source(dependency.registry.as_deref())?;
    Ok(match package_key_with_preferences(registry, dependency, preferred_versions)? {
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
    preferred_versions: &PreferredVersions,
    should_cancel: &(dyn Fn() -> bool + Send + Sync),
) -> Result<pubgrub::SelectedDependencies<PackageKey, Version>> {
    let mut provider = InnerProvider::new();
    let mut pending = VecDeque::new();
    let root_constraints =
        constraints_for(registry, root_dependencies, preferred_versions, &mut pending)?;
    provider.add_dependencies(PackageKey::Root, Version::new(0, 0, 0), root_constraints);

    let registration =
        Registration::new(registry, feature_selections, preferred_versions, should_cancel);
    let mut registered = BTreeSet::new();
    while let Some(package) = pending.pop_front() {
        ensure_not_cancelled(should_cancel)?;
        if !registered.insert(package.clone()) {
            continue;
        }
        registration.register(&package, &mut provider, &mut pending)?;
    }

    let provider = CancellableProvider { inner: provider, preferred_versions, should_cancel };
    match resolve(&provider, PackageKey::Root, Version::new(0, 0, 0)) {
        Ok(solution) => Ok(solution),
        Err(PubGrubError::NoSolution(mut tree)) => {
            tree.collapse_no_versions();
            let report = DefaultStringReporter::report(&tree);
            Err(miette::miette!(report))
        }
        Err(PubGrubError::ErrorInShouldCancel(CancelError)) => Err(CancelError.into()),
        Err(error) => {
            let message = error.to_string();
            Err(miette::miette!(message))
        }
    }
}
