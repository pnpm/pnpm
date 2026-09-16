use crate::{
    features::{active_dependencies, indexed_version},
    metadata::{active_metadata_dependencies, root_dependencies},
    model::{CargoMetadata, FeatureSelection, PackageKey, RegistryDependency, RegistryVersion},
    packages::selected_package,
    registry::{CRATES_IO_SOURCE, Registry, is_crates_io_source},
};
use cargo_lock::{Checksum, Dependency, Lockfile, Metadata, Name, Package, Patch, ResolveVersion};
use miette::{IntoDiagnostic, Result, WrapErr};
use semver::{Version, VersionReq};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque, btree_map::Entry},
    str::FromStr,
};

/// Serialize the solved graph as a `Cargo.lock`.
///
/// `configured` is the registry the index files came from; see
/// [`locked_sources`] for which crates are recorded against it.
pub(crate) fn lockfile_from_solution(
    metadata: &CargoMetadata,
    registry: &Registry,
    solution: &pubgrub::SelectedDependencies<PackageKey, Version>,
    feature_selections: &BTreeMap<PackageKey, FeatureSelection>,
    configured: &str,
) -> Result<String> {
    let selected = solution.iter().collect::<BTreeMap<_, _>>();
    let sources = locked_sources(metadata, registry, solution, feature_selections, configured)?;
    let mut packages = Vec::new();

    for (key, version) in &selected {
        let PackageKey::Registry { name, .. } = key else {
            continue;
        };
        let registry_version = indexed_version(registry.package(name)?, name, version)?;
        let selection = feature_selections
            .get(*key)
            .cloned()
            .unwrap_or_default();
        let dependencies = locked_registry_dependencies(
            registry_version,
            &selection,
            registry,
            solution,
            &sources,
        )?;
        packages.push(Package {
            name: Name::from_str(name).into_diagnostic()?,
            version: (*version).clone(),
            source: Some(package_source(&sources, key)?),
            checksum: Some(Checksum::from_str(&registry_version.checksum).into_diagnostic()?),
            dependencies,
            replace: None,
        });
    }

    packages.extend(workspace_packages(metadata, registry, solution, &sources)?);

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

/// The lock entries for the workspace's own members.
fn workspace_packages(
    metadata: &CargoMetadata,
    registry: &Registry,
    solution: &pubgrub::SelectedDependencies<PackageKey, Version>,
    sources: &BTreeMap<PackageKey, String>,
) -> Result<Vec<Package>> {
    let mut packages = Vec::new();
    for package in metadata.packages
        .iter()
        .filter(|package| metadata.workspace_members.contains(&package.id))
    {
        let dependencies = active_metadata_dependencies(package)?
            .iter()
            .map(|dependency| {
                if dependency.registry.is_some() {
                    locked_dependency(
                        &dependency.name,
                        &dependency.requirement,
                        registry,
                        solution,
                        sources,
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

    Ok(packages)
}

fn locked_registry_dependencies(
    package: &RegistryVersion,
    selection: &FeatureSelection,
    registry: &Registry,
    solution: &pubgrub::SelectedDependencies<PackageKey, Version>,
    sources: &BTreeMap<PackageKey, String>,
) -> Result<Vec<Dependency>> {
    let mut dependencies = BTreeSet::new();
    for dependency in active_dependencies(package, selection)? {
        registry.validate_dependency_source(dependency.registry.as_deref())?;
        dependencies.insert(locked_dependency(
            &dependency.name,
            &dependency.requirement,
            registry,
            solution,
            sources,
        )?);
    }
    Ok(dependencies.into_iter().collect())
}

fn locked_dependency(
    name: &str,
    requirement: &VersionReq,
    registry: &Registry,
    solution: &pubgrub::SelectedDependencies<PackageKey, Version>,
    sources: &BTreeMap<PackageKey, String>,
) -> Result<Dependency> {
    let (package, version) = selected_package(registry, name, requirement, solution)?
        .and_then(|package| {
            let version = solution.get(&package)?.clone();
            Some((package, version))
        })
        .ok_or_else(|| miette::miette!("resolver did not select dependency {name}"))?;
    Ok(Dependency {
        name: Name::from_str(name).into_diagnostic()?,
        version,
        source: Some(package_source(sources, &package)?),
    })
}

fn package_source(
    sources: &BTreeMap<PackageKey, String>,
    package: &PackageKey,
) -> Result<cargo_lock::SourceId> {
    let source = sources
        .get(package)
        .ok_or_else(|| miette::miette!("no Cargo source was resolved for {package}"))?;
    cargo_lock::SourceId::from_url(source)
        .into_diagnostic()
        .wrap_err("construct Cargo registry source identifier")
}

/// The source every resolved crate is locked against.
///
/// `cargo` records where a dependency said to look, not where the package
/// was fetched from. A dependency naming crates.io, or naming no registry
/// from a crate that itself came from crates.io, belongs to crates.io even
/// when a replacement serves it. One naming the registry being resolved
/// from belongs to that registry. An index entry names no registry when it
/// means its own, so it inherits the source of the crate that pulled it in.
fn locked_sources(
    metadata: &CargoMetadata,
    registry: &Registry,
    solution: &pubgrub::SelectedDependencies<PackageKey, Version>,
    feature_selections: &BTreeMap<PackageKey, FeatureSelection>,
    configured: &str,
) -> Result<BTreeMap<PackageKey, String>> {
    let mut sources = BTreeMap::<PackageKey, String>::new();
    let mut pending = root_dependencies(metadata)?
        .into_iter()
        .map(|dependency| (dependency, CRATES_IO_SOURCE.to_string()))
        .collect::<VecDeque<_>>();

    while let Some((dependency, inherited)) = pending.pop_front() {
        let source = declared_source(&dependency, &inherited, configured);
        let key = selected_package(registry, &dependency.name, &dependency.requirement, solution)?;
        let Some(key) = key else { continue };
        let Some(version) = solution.get(&key) else { continue };
        match sources.entry(key.clone()) {
            Entry::Occupied(known) if *known.get() == source => continue,
            Entry::Occupied(known) => {
                let first = known.get();
                return Err(miette::miette!(
                    "crate {} is required from two Cargo registries, {first:?} and {source:?}",
                    dependency.name,
                ));
            }
            Entry::Vacant(slot) => slot.insert(source.clone()),
        };
        let entry =
            indexed_version(registry.package(&dependency.name)?, &dependency.name, version)?;
        let selection = feature_selections
            .get(&key)
            .cloned()
            .unwrap_or_default();
        pending.extend(
            active_dependencies(entry, &selection)?
                .into_iter()
                .map(|dependency| (dependency, source.clone())),
        );
    }
    Ok(sources)
}

/// The registry a dependency names, falling back to the one its dependent
/// came from when it names none.
fn declared_source(dependency: &RegistryDependency, inherited: &str, configured: &str) -> String {
    match dependency.registry.as_deref() {
        None => inherited.to_string(),
        Some(registry) if is_crates_io_source(registry) => CRATES_IO_SOURCE.to_string(),
        Some(_) => configured.to_string(),
    }
}

fn locked_workspace_dependency(
    name: &str,
    requirement: &VersionReq,
    metadata: &CargoMetadata,
) -> Result<Dependency> {
    let package = metadata.packages
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
