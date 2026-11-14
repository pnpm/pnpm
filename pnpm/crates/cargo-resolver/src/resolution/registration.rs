use crate::{
    features::{locked_dependencies, supports_features},
    model::{FeatureSelection, PackageKey, RegistryDependency},
    packages::package_key_with_preferences,
    preferences::{PreferredVersions, contains as is_preferred},
    registry::{Registry, compatibility_line},
};
use miette::{IntoDiagnostic, Result, WrapErr};
use pubgrub::{OfflineDependencyProvider, Ranges};
use semver::{Version, VersionReq};
use std::collections::{BTreeMap, VecDeque};

type InnerProvider = OfflineDependencyProvider<PackageKey, Ranges<Version>>;

struct Requirement<'a> {
    package: &'a PackageKey,
    name: &'a str,
    version: VersionReq,
    features: FeatureSelection,
}

#[derive(Clone, Copy)]
pub(super) struct Registration<'a> {
    registry: &'a Registry,
    feature_selections: &'a BTreeMap<PackageKey, FeatureSelection>,
    preferred_versions: &'a PreferredVersions,
    should_cancel: &'a (dyn Fn() -> bool + Send + Sync),
}

impl Registration<'_> {
    pub(super) fn new<'a>(
        registry: &'a Registry,
        feature_selections: &'a BTreeMap<PackageKey, FeatureSelection>,
        preferred_versions: &'a PreferredVersions,
        should_cancel: &'a (dyn Fn() -> bool + Send + Sync),
    ) -> Registration<'a> {
        Registration { registry, feature_selections, preferred_versions, should_cancel }
    }

    pub(super) fn register(
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
        for version in versions
            .iter()
            .filter(|version| {
                (!version.yanked || is_preferred(self.preferred_versions, name, &version.version))
                    && compatibility_line(&version.version) == *compatibility
            })
        {
            crate::model::ensure_not_cancelled(self.should_cancel)?;
            let dependencies = locked_dependencies(version, &resolved)?;
            let constraints =
                constraints_for(self.registry, &dependencies, self.preferred_versions, pending)?;
            provider.add_dependencies(package.clone(), version.version.clone(), constraints);
        }
        Ok(())
    }

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
        let requirement = Requirement { package, name, version: requirement, features: requested };
        for (compatibility, representative) in matching_compatibility_lines(
            versions,
            &requirement.version,
            requirement.name,
            self.preferred_versions,
        ) {
            self.register_compatibility_line(
                provider,
                pending,
                versions,
                &requirement,
                &compatibility,
                representative,
            )?;
        }
        Ok(())
    }

    fn register_compatibility_line(
        &self,
        provider: &mut InnerProvider,
        pending: &mut VecDeque<PackageKey>,
        versions: &[crate::model::RegistryVersion],
        requirement: &Requirement<'_>,
        compatibility: &str,
        representative: Version,
    ) -> Result<()> {
        crate::model::ensure_not_cancelled(self.should_cancel)?;
        let line = PackageKey::Registry {
            name: requirement.name.to_string(),
            compatibility: compatibility.to_string(),
        };
        let admitted = admitted_versions(
            versions,
            &requirement.version,
            &requirement.features,
            requirement.name,
            compatibility,
            self.preferred_versions,
        );
        if admitted == Ranges::empty() {
            return Ok(());
        }
        provider.add_dependencies(
            requirement.package.clone(),
            representative,
            [(line.clone(), admitted)],
        );
        pending.push_back(line);
        Ok(())
    }
}

pub(super) fn constraints_for(
    registry: &Registry,
    dependencies: &[RegistryDependency],
    preferred_versions: &BTreeMap<String, Vec<Version>>,
    pending: &mut VecDeque<PackageKey>,
) -> Result<Vec<(PackageKey, Ranges<Version>)>> {
    let mut constraints = BTreeMap::<PackageKey, Ranges<Version>>::new();
    for dependency in dependencies {
        registry.validate_dependency_source(dependency.registry.as_deref())?;
        let package = package_key_with_preferences(registry, dependency, preferred_versions)?;
        let selection = dependency.feature_selection();
        let allowed = if let PackageKey::Registry { compatibility, .. } = &package {
            registry
                .package(&dependency.name)?
                .iter()
                .filter(|version| {
                    (!version.yanked
                        || is_preferred(preferred_versions, &dependency.name, &version.version))
                        && dependency.requirement.matches(&version.version)
                })
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

fn matching_compatibility_lines(
    versions: &[crate::model::RegistryVersion],
    requirement: &VersionReq,
    name: &str,
    preferred_versions: &BTreeMap<String, Vec<Version>>,
) -> Vec<(String, Version)> {
    let mut newest_by_line = BTreeMap::new();
    for version in versions
        .iter()
        .filter(|version| {
            (!version.yanked || is_preferred(preferred_versions, name, &version.version))
                && requirement.matches(&version.version)
        })
    {
        newest_by_line.insert(compatibility_line(&version.version), version.version.clone());
    }
    let mut lines = newest_by_line.into_iter().collect::<Vec<_>>();
    lines.sort_by(|left, right| left.1.cmp(&right.1));
    lines
}

fn admitted_versions(
    versions: &[crate::model::RegistryVersion],
    requirement: &VersionReq,
    requested: &FeatureSelection,
    name: &str,
    compatibility: &str,
    preferred_versions: &BTreeMap<String, Vec<Version>>,
) -> Ranges<Version> {
    versions
        .iter()
        .filter(|version| {
            (!version.yanked || is_preferred(preferred_versions, name, &version.version))
                && requirement.matches(&version.version)
        })
        .filter(|version| compatibility_line(&version.version) == compatibility)
        .filter(|version| supports_features(version, requested))
        .fold(Ranges::empty(), |range, version| {
            range.union(&Ranges::singleton(version.version.clone()))
        })
}
