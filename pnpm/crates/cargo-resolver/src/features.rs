use crate::{
    model::{FeatureSelection, PackageKey, RegistryDependency, RegistryVersion},
    registry::{Registry, compatibility_line, matching_versions, validate_registry},
};
use miette::Result;
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
        } else {
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

pub(crate) fn active_dependencies_from_parts(
    dependencies: &[RegistryDependency],
    features: &BTreeMap<String, Vec<String>>,
    selection: &FeatureSelection,
    include_dev: bool,
) -> Result<Vec<RegistryDependency>> {
    let activations = collect_feature_activations(dependencies, features, selection);
    Ok(dependencies
        .iter()
        .filter(|dependency| include_dev || dependency.kind.as_deref() != Some("dev"))
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
    let optional_aliases = dependencies
        .iter()
        .filter(|dependency| dependency.optional)
        .map(|dependency| dependency.alias.as_str())
        .collect::<BTreeSet<_>>();
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
            if optional_aliases.contains(feature.as_str()) {
                activations.active_aliases.insert(feature);
            }
            continue;
        };
        for activation in feature_activations {
            activations.record_feature_activation(activation, features, &mut pending);
        }
    }
    activations.activate_weak_dependency_features();
    activations
}

pub(crate) fn collect_feature_selections(
    registry: &Registry,
    root_dependencies: &[RegistryDependency],
) -> Result<BTreeMap<PackageKey, FeatureSelection>> {
    let mut selections = BTreeMap::<PackageKey, FeatureSelection>::new();
    let mut candidate_versions = BTreeMap::<PackageKey, BTreeSet<Version>>::new();
    let mut pending = VecDeque::from(root_dependencies.to_vec());

    while let Some(dependency) = pending.pop_front() {
        validate_registry(dependency.registry.as_deref())?;
        let versions = registry.package(&dependency.name)?;
        let version =
            matching_versions(versions, &dependency.requirement).next_back().ok_or_else(|| {
                miette::miette!(
                    "no non-yanked version of {} satisfies {}",
                    dependency.name,
                    dependency.requirement,
                )
            })?;
        let compatibility = compatibility_line(&version.version);
        let package = PackageKey::Registry { name: dependency.name.clone(), compatibility };
        let candidate_is_new =
            candidate_versions.entry(package.clone()).or_default().insert(version.version.clone());
        let requested = dependency.feature_selection();
        let selection = selections.entry(package.clone()).or_default();
        let previous = selection.clone();
        selection.default_features |= requested.default_features;
        selection.features.extend(requested.features);
        let selection_changed = *selection != previous;
        let selection = selection.clone();
        if !candidate_is_new && !selection_changed {
            continue;
        }
        for candidate_version in &candidate_versions[&package] {
            let candidate = versions
                .iter()
                .find(|candidate| candidate.version == *candidate_version)
                .expect("recorded candidate came from this registry package");
            pending.extend(active_dependencies(candidate, &selection)?);
        }
    }
    Ok(selections)
}
