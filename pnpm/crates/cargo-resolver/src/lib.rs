//! Cargo-compatible dependency resolution for pnpm.

#![cfg_attr(dylint_lib = "perfectionist", feature(register_tool))]
#![cfg_attr(dylint_lib = "perfectionist", register_tool(perfectionist))]

use cargo_lock::{Checksum, Dependency, Lockfile, Metadata, Name, Package, Patch, ResolveVersion};
use cargo_util_schemas::index::IndexPackage;
use miette::{IntoDiagnostic, Result, WrapErr};
use pubgrub::{
    DefaultStringReporter, OfflineDependencyProvider, PubGrubError, Ranges, Reporter, resolve,
};
use semver::{Version, VersionReq};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fmt,
    str::FromStr,
};

const CRATES_IO_SOURCE: &str = "registry+https://github.com/rust-lang/crates.io-index";
const CRATES_IO_INDEX: &str = "https://github.com/rust-lang/crates.io-index";
const CRATES_IO_SPARSE_INDEX: &str = "sparse+https://index.crates.io/";

#[derive(Debug, Deserialize)]
struct CargoMetadata {
    packages: Vec<MetadataPackage>,
    workspace_members: BTreeSet<String>,
}

#[derive(Debug, Deserialize)]
struct MetadataPackage {
    id: String,
    name: String,
    version: Version,
    dependencies: Vec<MetadataDependency>,
    #[serde(default)]
    features: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct MetadataDependency {
    name: String,
    source: Option<String>,
    req: VersionReq,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    rename: Option<String>,
    #[serde(default)]
    optional: bool,
    #[serde(default = "default_true")]
    uses_default_features: bool,
    #[serde(default)]
    features: Vec<String>,
}

#[derive(Debug, Clone)]
struct RegistryVersion {
    version: Version,
    dependencies: Vec<RegistryDependency>,
    features: BTreeMap<String, Vec<String>>,
    checksum: String,
    yanked: bool,
}

#[derive(Debug, Clone)]
struct RegistryDependency {
    alias: String,
    name: String,
    requirement: VersionReq,
    kind: Option<String>,
    registry: Option<String>,
    optional: bool,
    default_features: bool,
    features: BTreeSet<String>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct FeatureSelection {
    default_features: bool,
    features: BTreeSet<String>,
}

impl RegistryDependency {
    fn feature_selection(&self) -> FeatureSelection {
        FeatureSelection {
            default_features: self.default_features,
            features: self.features.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum PackageKey {
    Root,
    Registry { name: String, compatibility: String },
}

impl fmt::Display for PackageKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Root => formatter.write_str("pnpm Cargo workspace"),
            Self::Registry { name, compatibility } => write!(formatter, "{name}@{compatibility}"),
        }
    }
}

/// Return sparse-index package names needed to resolve `metadata`.
///
/// Callers fetch the returned files and call this function again until the
/// result is empty. Keeping network I/O outside this crate makes the resolver
/// usable by both the CLI and a future pnpr implementation.
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
        let visit_key = (
            dependency.name.clone(),
            dependency.requirement.to_string(),
            dependency.default_features,
            dependency.features.clone(),
        );
        if !visited.insert(visit_key) {
            continue;
        }
        let normalized_name = normalize_name(&dependency.name);
        let Some(versions) = registry.packages.get(&normalized_name) else {
            missing.insert(dependency.name);
            continue;
        };
        if let Some(version) = matching_versions(versions, &dependency.requirement).next_back() {
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
            if dependencies.iter().any(|dependency| {
                !registry.packages.contains_key(&normalize_name(&dependency.name))
            }) {
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

/// Return the newest stable, non-yanked version from a crates.io sparse-index entry.
///
/// `cargo add <name>` records this version as the dependency requirement when the
/// user did not supply one. Pre-releases remain opt-in, matching Cargo's default.
pub fn latest_version(name: &str, index_file: &str) -> Result<String> {
    let registry = Registry::new(&BTreeMap::from([(name.to_string(), index_file.to_string())]))?;
    registry
        .package(name)?
        .iter()
        .rev()
        .find(|version| !version.yanked && version.version.pre.is_empty())
        .map(|version| version.version.to_string())
        .ok_or_else(|| miette::miette!("crate {name} has no stable, non-yanked version"))
}

struct Registry {
    packages: BTreeMap<String, Vec<RegistryVersion>>,
}

impl Registry {
    fn new(index_files: &BTreeMap<String, String>) -> Result<Self> {
        let mut packages = BTreeMap::new();
        for (name, contents) in index_files {
            let mut versions = Vec::new();
            for (line_index, line) in contents.lines().enumerate() {
                let package: IndexPackage<'_> =
                    serde_json::from_str(line).into_diagnostic().wrap_err_with(|| {
                        format!("parse sparse index entry {name}:{}", line_index + 1)
                    })?;
                if package.v.is_some_and(|version| version > 3) {
                    continue;
                }
                let dependencies = package
                    .deps
                    .into_iter()
                    .map(|dependency| {
                        let alias = dependency.name.into_owned();
                        let name = dependency
                            .package
                            .map_or_else(|| alias.clone(), std::borrow::Cow::into_owned);
                        let requirement = VersionReq::parse(&dependency.req)
                            .into_diagnostic()
                            .wrap_err_with(|| format!("parse requirement for {name}"))?;
                        Ok(RegistryDependency {
                            alias,
                            name,
                            requirement,
                            kind: dependency.kind.map(std::borrow::Cow::into_owned),
                            registry: dependency.registry.map(std::borrow::Cow::into_owned),
                            optional: dependency.optional,
                            default_features: dependency.default_features,
                            features: dependency
                                .features
                                .into_iter()
                                .map(std::borrow::Cow::into_owned)
                                .collect(),
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                versions.push(RegistryVersion {
                    version: package.vers,
                    dependencies,
                    features: package
                        .features
                        .into_iter()
                        .chain(package.features2.unwrap_or_default())
                        .map(|(name, values)| {
                            (
                                name.into_owned(),
                                values.into_iter().map(std::borrow::Cow::into_owned).collect(),
                            )
                        })
                        .collect(),
                    checksum: package.cksum,
                    yanked: package.yanked.unwrap_or(false),
                });
            }
            versions.sort_by(|left, right| left.version.cmp(&right.version));
            packages.insert(normalize_name(name), versions);
        }
        Ok(Self { packages })
    }

    fn package(&self, name: &str) -> Result<&[RegistryVersion]> {
        self.packages.get(&normalize_name(name)).map(Vec::as_slice).ok_or_else(|| {
            miette::miette!("sparse index metadata for crate {name} was not fetched")
        })
    }
}

fn parse_metadata(metadata: &str) -> Result<CargoMetadata> {
    serde_json::from_str(metadata).into_diagnostic().wrap_err("parse cargo metadata")
}

fn root_dependencies(metadata: &CargoMetadata) -> Result<Vec<RegistryDependency>> {
    metadata
        .packages
        .iter()
        .filter(|package| metadata.workspace_members.contains(&package.id))
        .map(active_metadata_dependencies)
        .collect::<Result<Vec<_>>>()
        .map(|dependencies| {
            dependencies
                .into_iter()
                .flatten()
                .filter(|dependency| dependency.registry.is_some())
                .collect()
        })
}

fn active_metadata_dependencies(package: &MetadataPackage) -> Result<Vec<RegistryDependency>> {
    let dependencies = package
        .dependencies
        .iter()
        .map(|dependency| RegistryDependency {
            alias: dependency.rename.clone().unwrap_or_else(|| dependency.name.clone()),
            name: dependency.name.clone(),
            requirement: dependency.req.clone(),
            kind: dependency.kind.clone(),
            registry: dependency.source.clone(),
            optional: dependency.optional,
            default_features: dependency.uses_default_features,
            features: dependency.features.iter().cloned().collect(),
        })
        .collect::<Vec<_>>();
    active_dependencies_from_parts(
        &dependencies,
        &package.features,
        &FeatureSelection {
            default_features: true,
            features: package.features.keys().cloned().collect(),
        },
        true,
    )
}

fn active_dependencies(
    package: &RegistryVersion,
    selection: &FeatureSelection,
) -> Result<Vec<RegistryDependency>> {
    active_dependencies_from_parts(&package.dependencies, &package.features, selection, false)
}

fn active_dependencies_from_parts(
    dependencies: &[RegistryDependency],
    features: &BTreeMap<String, Vec<String>>,
    selection: &FeatureSelection,
    include_dev: bool,
) -> Result<Vec<RegistryDependency>> {
    let aliases = dependencies
        .iter()
        .map(|dependency| (dependency.alias.as_str(), dependency))
        .collect::<BTreeMap<_, _>>();
    let mut pending = selection.features.iter().cloned().collect::<VecDeque<_>>();
    if selection.default_features {
        pending.push_back("default".to_string());
    }
    let mut visited = BTreeSet::new();
    let mut active_aliases = BTreeSet::new();
    let mut dependency_features = BTreeMap::<String, BTreeSet<String>>::new();
    let mut weak_dependency_features = Vec::new();

    while let Some(feature) = pending.pop_front() {
        if !visited.insert(feature.clone()) {
            continue;
        }
        let Some(activations) = features.get(&feature) else {
            if aliases.get(feature.as_str()).is_some_and(|dependency| dependency.optional) {
                active_aliases.insert(feature);
            }
            continue;
        };
        for activation in activations {
            if let Some(alias) = activation.strip_prefix("dep:") {
                active_aliases.insert(alias.to_string());
            } else if let Some((alias, feature)) = activation.split_once('/') {
                if let Some(alias) = alias.strip_suffix('?') {
                    weak_dependency_features.push((alias.to_string(), feature.to_string()));
                } else {
                    active_aliases.insert(alias.to_string());
                    dependency_features
                        .entry(alias.to_string())
                        .or_default()
                        .insert(feature.to_string());
                }
            } else if features.contains_key(activation) {
                pending.push_back(activation.clone());
            } else {
                active_aliases.insert(activation.clone());
            }
        }
    }
    for (alias, feature) in weak_dependency_features {
        if active_aliases.contains(&alias) {
            dependency_features.entry(alias).or_default().insert(feature);
        }
    }

    dependencies
        .iter()
        .filter(|dependency| include_dev || dependency.kind.as_deref() != Some("dev"))
        .filter(|dependency| !dependency.optional || active_aliases.contains(&dependency.alias))
        .map(|dependency| {
            let mut dependency = dependency.clone();
            if let Some(features) = dependency_features.get(&dependency.alias) {
                dependency.features.extend(features.iter().cloned());
            }
            Ok(dependency)
        })
        .collect()
}

const fn default_true() -> bool {
    true
}

fn collect_feature_selections(
    registry: &Registry,
    root_dependencies: &[RegistryDependency],
) -> Result<BTreeMap<PackageKey, FeatureSelection>> {
    let mut selections = BTreeMap::<PackageKey, FeatureSelection>::new();
    let mut pending = VecDeque::from(root_dependencies.to_vec());

    while let Some(dependency) = pending.pop_front() {
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
        let requested = dependency.feature_selection();
        let is_new = !selections.contains_key(&package);
        let selection = selections.entry(package).or_default();
        let previous = selection.clone();
        selection.default_features |= requested.default_features;
        selection.features.extend(requested.features);
        if !is_new && *selection == previous {
            continue;
        }
        if let Some(version) = versions.iter().rfind(|version| {
            !version.yanked && compatibility_line(&version.version) == compatibility
        }) {
            pending.extend(active_dependencies(version, selection)?);
        }
    }
    Ok(selections)
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

fn matching_versions<'a>(
    versions: &'a [RegistryVersion],
    requirement: &'a VersionReq,
) -> impl DoubleEndedIterator<Item = &'a RegistryVersion> {
    versions.iter().filter(|version| !version.yanked && requirement.matches(&version.version))
}

fn validate_registry(registry: Option<&str>) -> Result<()> {
    if registry.is_none_or(|registry| {
        matches!(registry, CRATES_IO_SOURCE | CRATES_IO_INDEX | CRATES_IO_SPARSE_INDEX)
    }) {
        Ok(())
    } else {
        Err(miette::miette!(
            "alternate Cargo registry {registry:?} is not supported by the crates.io proof of concept"
        ))
    }
}

fn compatibility_line(version: &Version) -> String {
    if version.major != 0 {
        version.major.to_string()
    } else if version.minor != 0 {
        format!("0.{}", version.minor)
    } else {
        format!("0.0.{}", version.patch)
    }
}

fn normalize_name(name: &str) -> String {
    name.to_ascii_lowercase()
}

fn lockfile_from_solution(
    metadata: &CargoMetadata,
    registry: &Registry,
    solution: &pubgrub::SelectedDependencies<PackageKey, Version>,
    feature_selections: &BTreeMap<PackageKey, FeatureSelection>,
) -> Result<String> {
    let source = cargo_lock::SourceId::from_url(CRATES_IO_SOURCE)
        .into_diagnostic()
        .wrap_err("construct crates.io source identifier")?;
    let selected = solution.iter().collect::<BTreeMap<_, _>>();
    let mut packages = Vec::new();

    for (key, version) in &selected {
        let PackageKey::Registry { name, .. } = key else { continue };
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
        validate_registry(dependency.registry.as_deref())?;
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

#[cfg(test)]
mod tests;
