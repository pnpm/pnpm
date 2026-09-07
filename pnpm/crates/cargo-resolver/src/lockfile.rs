use crate::{
    features::active_dependencies,
    metadata::active_metadata_dependencies,
    model::{CargoMetadata, FeatureSelection, PackageKey, RegistryVersion},
    registry::{Registry, compatibility_line, matching_versions},
};
use cargo_lock::{Checksum, Dependency, Lockfile, Metadata, Name, Package, Patch, ResolveVersion};
use miette::{IntoDiagnostic, Result, WrapErr};
use semver::{Version, VersionReq};
use std::{
    collections::{BTreeMap, BTreeSet},
    str::FromStr,
};

pub(crate) fn lockfile_from_solution(
    metadata: &CargoMetadata,
    registry: &Registry,
    solution: &pubgrub::SelectedDependencies<PackageKey, Version>,
    feature_selections: &BTreeMap<PackageKey, FeatureSelection>,
    source: &str,
) -> Result<String> {
    let source = cargo_lock::SourceId::from_url(source)
        .into_diagnostic()
        .wrap_err("construct Cargo registry source identifier")?;
    let selected = solution.iter().collect::<BTreeMap<_, _>>();
    let mut packages = Vec::new();

    for (key, version) in &selected {
        let PackageKey::Registry { name, .. } = key else {
            continue;
        };
        let registry_version = registry
            .package(name)?
            .iter()
            .find(|candidate| candidate.version == **version)
            .ok_or_else(|| miette::miette!("selected {name} {version} is absent from the index"))?;
        let selection = feature_selections.get(*key).cloned().unwrap_or_default();
        let dependencies = locked_registry_dependencies(
            registry_version,
            &selection,
            registry,
            &selected,
            &source,
        )?;
        packages.push(Package {
            name: Name::from_str(name).into_diagnostic()?,
            version: (*version).clone(),
            source: Some(source.clone()),
            checksum: Some(Checksum::from_str(&registry_version.checksum).into_diagnostic()?),
            dependencies,
            replace: None,
        });
    }

    for package in
        metadata.packages.iter().filter(|package| metadata.workspace_members.contains(&package.id))
    {
        let dependencies = active_metadata_dependencies(package)?
            .iter()
            .map(|dependency| {
                if dependency.registry.is_some() {
                    locked_dependency(
                        &dependency.name,
                        &dependency.requirement,
                        registry,
                        &selected,
                        &source,
                    )
                } else {
                    locked_workspace_dependency(&dependency.name, &dependency.requirement, metadata)
                }
            })
            .collect::<Result<Vec<_>>>()?;
        packages.push(Package {
            name: Name::from_str(&package.name).into_diagnostic()?,
            version: package.version.clone(),
            source: None,
            checksum: None,
            dependencies,
            replace: None,
        });
    }

    packages.sort();
    let lockfile = Lockfile {
        version: ResolveVersion::V4,
        packages,
        root: None,
        metadata: Metadata::default(),
        patch: Patch::default(),
    };
    Ok(lockfile.to_string())
}

fn locked_registry_dependencies(
    package: &RegistryVersion,
    selection: &FeatureSelection,
    registry: &Registry,
    selected: &BTreeMap<&PackageKey, &Version>,
    source: &cargo_lock::SourceId,
) -> Result<Vec<Dependency>> {
    let mut dependencies = BTreeSet::new();
    for dependency in active_dependencies(package, selection)? {
        registry.validate_dependency_source(dependency.registry.as_deref())?;
        dependencies.insert(locked_dependency(
            &dependency.name,
            &dependency.requirement,
            registry,
            selected,
            source,
        )?);
    }
    Ok(dependencies.into_iter().collect())
}

fn locked_dependency(
    name: &str,
    requirement: &VersionReq,
    registry: &Registry,
    selected: &BTreeMap<&PackageKey, &Version>,
    source: &cargo_lock::SourceId,
) -> Result<Dependency> {
    let compatibility = matching_versions(registry.package(name)?, requirement)
        .next_back()
        .map(|version| compatibility_line(&version.version))
        .ok_or_else(|| miette::miette!("no version of {name} satisfies {requirement}"))?;
    let key = PackageKey::Registry { name: name.to_string(), compatibility };
    let version = selected
        .get(&key)
        .ok_or_else(|| miette::miette!("resolver did not select dependency {name}"))?;
    Ok(Dependency {
        name: Name::from_str(name).into_diagnostic()?,
        version: (*version).clone(),
        source: Some(source.clone()),
    })
}

fn locked_workspace_dependency(
    name: &str,
    requirement: &VersionReq,
    metadata: &CargoMetadata,
) -> Result<Dependency> {
    let package = metadata
        .packages
        .iter()
        .filter(|package| metadata.workspace_members.contains(&package.id))
        .find(|package| package.name == name && requirement.matches(&package.version))
        .ok_or_else(|| miette::miette!("workspace dependency {name} is not a workspace member"))?;
    Ok(Dependency {
        name: Name::from_str(&package.name).into_diagnostic()?,
        version: package.version.clone(),
        source: None,
    })
}
