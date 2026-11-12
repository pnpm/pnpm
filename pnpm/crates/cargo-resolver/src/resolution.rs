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
    packages::{chosen_line, package_key},
    registry::{Registry, compatibility_line, matching_lines, matching_versions},
};
use miette::{IntoDiagnostic, Result, WrapErr};
use pubgrub::{
    DefaultStringReporter, Dependencies, DependencyProvider, OfflineDependencyProvider,
    PackageResolutionStatistics, PubGrubError, Ranges, Reporter, resolve,
};
use semver::{Version, VersionReq};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

type InnerProvider = OfflineDependencyProvider<PackageKey, Ranges<Version>>;

struct CancellableProvider<'a> {
    inner: InnerProvider,
    should_cancel: &'a (dyn Fn() -> bool + Send + Sync),
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
    resolve_lockfile_until(metadata, index_files, source, &|| false)
}

/// [`resolve_lockfile`] with cooperative cancellation checked across package
/// registration, solver steps, and feature validation.
pub fn resolve_lockfile_until(
    metadata: &str,
    index_files: &BTreeMap<String, String>,
    source: &str,
    should_cancel: &(dyn Fn() -> bool + Send + Sync),
) -> Result<String> {
    let metadata = parse_metadata(metadata)?;
    let registry = Registry::new_until(index_files, source, should_cancel)?;
    let root_dependencies = root_dependencies(&metadata)?;
    let mut feature_selections = root_feature_selections(&registry, &root_dependencies)?;
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
            should_cancel,
        )?;
        let selected_features =
            feature_selections_for_solution(&registry, &root_dependencies, &solution)?;
        if let Some(validated_solution) = validate_selected_graph(
            &registry,
            &root_dependencies,
            &solution,
            &selected_features,
            should_cancel,
        )? {
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
    should_cancel: &(dyn Fn() -> bool + Send + Sync),
) -> Result<Option<pubgrub::SelectedDependencies<PackageKey, Version>>> {
    let Some(root_version) = solution.get(&PackageKey::Root) else { return Ok(None) };
    let mut validated = BTreeMap::from([(PackageKey::Root, root_version.clone())]);
    let mut pending = VecDeque::from(root_dependencies.to_vec());

    while let Some(dependency) = pending.pop_front() {
        ensure_not_cancelled(should_cancel)?;
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
    should_cancel: &(dyn Fn() -> bool + Send + Sync),
) -> Result<pubgrub::SelectedDependencies<PackageKey, Version>> {
    let mut provider = InnerProvider::new();
    let mut pending = VecDeque::new();
    let root_constraints = constraints_for(registry, root_dependencies, &mut pending)?;
    provider.add_dependencies(PackageKey::Root, Version::new(0, 0, 0), root_constraints);

    let registration = Registration { registry, feature_selections, should_cancel };
    let mut registered = BTreeSet::new();
    while let Some(package) = pending.pop_front() {
        ensure_not_cancelled(should_cancel)?;
        if !registered.insert(package.clone()) {
            continue;
        }
        registration.register(&package, &mut provider, &mut pending)?;
    }

    let provider = CancellableProvider { inner: provider, should_cancel };
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

#[derive(Clone, Copy)]
struct Registration<'a> {
    registry: &'a Registry,
    feature_selections: &'a BTreeMap<PackageKey, FeatureSelection>,
    should_cancel: &'a (dyn Fn() -> bool + Send + Sync),
}

impl Registration<'_> {
    /// Tell the solver what `package` offers, which depends on what kind of
    /// package it is.
    fn register(
        &self,
        package: &PackageKey,
        provider: &mut InnerProvider,
        pending: &mut VecDeque<PackageKey>,
    ) -> Result<()> {
        match package {
            PackageKey::Registry { .. } => self.register_candidates(package, provider, pending),
            PackageKey::Requirement { .. } => {
                self.register_compatibility_lines(package, provider, pending)
            }
            PackageKey::Root | PackageKey::Unsatisfiable { .. } => Ok(()),
        }
    }

    /// Offer the solver the versions of `package` that every dependency able to
    /// settle on it can support, queueing each one's own dependencies.
    fn register_candidates(
        &self,
        package: &PackageKey,
        provider: &mut InnerProvider,
        pending: &mut VecDeque<PackageKey>,
    ) -> Result<()> {
        let PackageKey::Registry { name, compatibility } = package else {
            return Ok(());
        };
        let resolved = self.feature_selections
            .get(package)
            .cloned()
            .unwrap_or_default();
        let versions = self.registry.package(name)?;
        let candidates = versions
            .iter()
            .filter(|version| {
                !version.yanked && compatibility_line(&version.version) == *compatibility
            });
        for version in candidates {
            ensure_not_cancelled(self.should_cancel)?;
            let dependencies = locked_dependencies(version, &resolved)?;
            let constraints = constraints_for(self.registry, &dependencies, pending)?;
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
        &self,
        package: &PackageKey,
        provider: &mut InnerProvider,
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
        let versions = self.registry.package(name)?;
        for (compatibility, representative) in matching_lines(versions, &requirement) {
            ensure_not_cancelled(self.should_cancel)?;
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
