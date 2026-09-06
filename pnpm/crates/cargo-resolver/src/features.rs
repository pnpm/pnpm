use crate::{
    model::{DependencyKind, FeatureSelection, PackageKey, RegistryDependency, RegistryVersion},
    registry::{Registry, compatibility_line, matching_versions},
};
use miette::Result;
use pubgrub::SelectedDependencies;
use semver::Version;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Debug, Default)]
struct FeatureActivations {
    active_aliases: BTreeSet<String>,
    dependency_features: BTreeMap<String, BTreeSet<String>>,
    weak_dependency_features: Vec<(String, String)>,
}

impl FeatureActivations {
    fn record_feature_activation(
        &mut self,
        activation: &str,
        features: &BTreeMap<String, Vec<String>>,
        implicit_optional_aliases: &BTreeSet<&str>,
        pending: &mut VecDeque<String>,
    ) {
        if let Some(alias) = activation.strip_prefix("dep:") {
            self.active_aliases.insert(alias.to_string());
        } else if let Some((alias, feature)) = activation.split_once('/') {
            if let Some(alias) = alias.strip_suffix('?') {
                self.weak_dependency_features.push((alias.to_string(), feature.to_string()));
            } else {
                self.active_aliases.insert(alias.to_string());
                self.dependency_features
                    .entry(alias.to_string())
                    .or_default()
                    .insert(feature.to_string());
            }
        } else if features.contains_key(activation) {
            pending.push_back(activation.to_string());
        } else if implicit_optional_aliases.contains(activation) {
            self.active_aliases.insert(activation.to_string());
        }
    }

    fn activate_weak_dependency_features(&mut self) {
        for (alias, feature) in std::mem::take(&mut self.weak_dependency_features) {
            if self.active_aliases.contains(&alias) {
                self.dependency_features.entry(alias).or_default().insert(feature);
            }
        }
    }
}

pub(crate) fn active_dependencies(
    package: &RegistryVersion,
    selection: &FeatureSelection,
) -> Result<Vec<RegistryDependency>> {
    active_dependencies_from_parts(&package.dependencies, &package.features, selection, false)
}

pub(crate) fn supports_features(package: &RegistryVersion, selection: &FeatureSelection) -> bool {
    let implicit_optional_aliases =
        implicit_optional_aliases(&package.dependencies, &package.features);
    selection.features.iter().all(|feature| {
        package.features.contains_key(feature)
            || implicit_optional_aliases.contains(feature.as_str())
    })
}

pub(crate) fn active_dependencies_from_parts(
    dependencies: &[RegistryDependency],
    features: &BTreeMap<String, Vec<String>>,
    selection: &FeatureSelection,
    include_dev: bool,
) -> Result<Vec<RegistryDependency>> {
    let activations = collect_feature_activations(dependencies, features, selection);
    Ok(dependencies
        .iter()
        .filter(|dependency| include_dev || dependency.kind != DependencyKind::Dev)
        .filter(|dependency| {
            !dependency.optional || activations.active_aliases.contains(&dependency.alias)
        })
        .map(|dependency| {
            let mut dependency = dependency.clone();
            if let Some(features) = activations.dependency_features.get(&dependency.alias) {
                dependency.features.extend(features.iter().cloned());
            }
            dependency
        })
        .collect())
}

fn collect_feature_activations(
    dependencies: &[RegistryDependency],
    features: &BTreeMap<String, Vec<String>>,
    selection: &FeatureSelection,
) -> FeatureActivations {
    let implicit_optional_aliases = implicit_optional_aliases(dependencies, features);
    let mut pending = selection.features.iter().cloned().collect::<VecDeque<_>>();
    if selection.default_features {
        pending.push_back("default".to_string());
    }
    let mut visited = BTreeSet::new();
    let mut activations = FeatureActivations::default();

    while let Some(feature) = pending.pop_front() {
        if !visited.insert(feature.clone()) {
            continue;
        }
        let Some(feature_activations) = features.get(&feature) else {
            if implicit_optional_aliases.contains(feature.as_str()) {
                activations.active_aliases.insert(feature);
            }
            continue;
        };
        for activation in feature_activations {
            activations.record_feature_activation(
                activation,
                features,
                &implicit_optional_aliases,
                &mut pending,
            );
        }
    }
    activations.activate_weak_dependency_features();
    activations
}

fn implicit_optional_aliases<'a>(
    dependencies: &'a [RegistryDependency],
    features: &BTreeMap<String, Vec<String>>,
) -> BTreeSet<&'a str> {
    let explicitly_activated_aliases = features
        .values()
        .flatten()
        .filter_map(|activation| activation.strip_prefix("dep:"))
        .collect::<BTreeSet<_>>();
    dependencies
        .iter()
        .filter(|dependency| {
            dependency.optional && !explicitly_activated_aliases.contains(dependency.alias.as_str())
        })
        .map(|dependency| dependency.alias.as_str())
        .collect()
}

pub(crate) fn root_feature_selections(
    registry: &Registry,
    root_dependencies: &[RegistryDependency],
) -> Result<BTreeMap<PackageKey, FeatureSelection>> {
    collect_feature_selections(registry, root_dependencies, None)
}

pub(crate) fn feature_selections_for_solution(
    registry: &Registry,
    root_dependencies: &[RegistryDependency],
    solution: &SelectedDependencies<PackageKey, Version>,
) -> Result<BTreeMap<PackageKey, FeatureSelection>> {
    collect_feature_selections(registry, root_dependencies, Some(solution))
}

fn collect_feature_selections(
    registry: &Registry,
    root_dependencies: &[RegistryDependency],
    solution: Option<&SelectedDependencies<PackageKey, Version>>,
) -> Result<BTreeMap<PackageKey, FeatureSelection>> {
    let mut selections = BTreeMap::<PackageKey, FeatureSelection>::new();
    let mut pending = VecDeque::from(root_dependencies.to_vec());

    while let Some(dependency) = pending.pop_front() {
        registry.validate_dependency_source(dependency.registry.as_deref())?;
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
        let package = PackageKey::Registry { name: dependency.name.clone(), compatibility };
        let requested = dependency.feature_selection();
        let previous = selections.get(&package).cloned();
        let selection = selections.entry(package.clone()).or_default();
        selection.default_features |= requested.default_features;
        selection.features.extend(requested.features);
        if previous.as_ref() == Some(selection) {
            continue;
        }
        let Some(selected_version) = solution.and_then(|solution| solution.get(&package)) else {
            continue;
        };
        let selected = versions
            .iter()
            .find(|candidate| candidate.version == *selected_version)
            .ok_or_else(|| {
                miette::miette!(
                    "selected {} {} is absent from the index",
                    dependency.name,
                    selected_version,
                )
            })?;
        pending.extend(active_dependencies(selected, selection)?);
    }
    Ok(selections)
}
