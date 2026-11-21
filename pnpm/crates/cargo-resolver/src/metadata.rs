use crate::{
    features::{
        DependencyOptions,
        dependencies_from_parts,
        widen_selection,
    },
    model::{
        CargoMetadata,
        FeatureSelection,
        MetadataPackage,
        RegistryDependency,
        accepts_declared_version,
    },
};
use miette::{
    IntoDiagnostic,
    Result,
    WrapErr,
};
use semver::VersionReq;
use std::collections::{
    BTreeMap,
    VecDeque,
};

pub(crate) fn parse_metadata(metadata: &str) -> Result<CargoMetadata> {
    serde_json::from_str(metadata).into_diagnostic().wrap_err("parse cargo metadata")
}

/// Git sources declared by the packages in a Cargo metadata document.
/// The metadata need not contain a resolved dependency graph.
pub fn git_dependency_sources(metadata: &str) -> Result<Vec<cargo_lock::SourceId>> {
    parse_metadata(metadata)?.packages
        .into_iter()
        .flat_map(|package| package.dependencies)
        .filter_map(|dependency| dependency.source)
        .filter(|source| source.starts_with("git+"))
        .map(|source| {
            source
                .parse()
                .into_diagnostic()
                .wrap_err("parse Cargo git dependency")
        })
        .collect()
}

/// The `cargo metadata` package keys that resolution reads and copies
/// through unchanged; `id` and `dependencies` are rewritten instead.
/// Mirrors [`MetadataPackage`], so a field read there is listed here too.
const PACKAGE_KEYS: [&str; 3] = ["name", "version", "features"];

/// The keys of a package's dependency that resolution reads. Mirrors
/// [`crate::model::MetadataDependency`], so a field read there is listed
/// here too.
const DEPENDENCY_KEYS: [&str; 8] =
    ["name", "source", "req", "kind", "rename", "optional", "uses_default_features", "features"];

/// Reduce a `cargo metadata` document to what resolution reads, replacing
/// each package id with its position.
///
/// The document a workspace produces describes the machine it was produced
/// on: `manifest_path`, `workspace_root`, every target's `src_path`, and the
/// package ids that embed those paths. None of it takes part in resolution,
/// so a document that leaves the machine (to a pnpr server resolving on the
/// client's behalf) carries the dependency graph alone.
pub fn resolve_inputs(metadata: &str) -> Result<String> {
    let document: serde_json::Value =
        serde_json::from_str(metadata).into_diagnostic().wrap_err("parse cargo metadata")?;
    let packages = document
        .get("packages")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| miette::miette!("cargo metadata has no `packages` array"))?;
    let ids: BTreeMap<&str, String> = packages
        .iter()
        .enumerate()
        .filter_map(|(position, package)| {
            Some((package.get("id")?.as_str()?, position.to_string()))
        })
        .collect();
    let packages = packages
        .iter()
        .map(|package| reduce_package(package, &ids))
        .collect::<Vec<_>>();
    let workspace_members = document
        .get("workspace_members")
        .and_then(serde_json::Value::as_array)
        .map(|members| {
            members
                .iter()
                .filter_map(|member| {
                    ids.get(member.as_str()?)
                        .map(String::as_str)
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    serde_json::to_string(&serde_json::json!({
        "packages": packages,
        "workspace_members": workspace_members,
    }))
    .into_diagnostic()
    .wrap_err("serialize cargo metadata")
}

fn reduce_package(package: &serde_json::Value, ids: &BTreeMap<&str, String>) -> serde_json::Value {
    let mut reduced = retained_keys(package, &PACKAGE_KEYS);
    if let Some(id) = package.get("id").and_then(serde_json::Value::as_str)
        && let Some(position) = ids.get(id)
    {
        reduced.insert("id".to_string(), position.as_str().into());
    }
    let dependencies = package
        .get("dependencies")
        .and_then(serde_json::Value::as_array)
        .map(|dependencies| {
            dependencies
                .iter()
                .map(|dependency| {
                    serde_json::Value::Object(retained_keys(dependency, &DEPENDENCY_KEYS))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    reduced.insert("dependencies".to_string(), dependencies.into());
    serde_json::Value::Object(reduced)
}

/// `value`'s object entries whose keys are in `keys`, dropping the rest.
fn retained_keys(
    value: &serde_json::Value,
    keys: &[&str],
) -> serde_json::Map<String, serde_json::Value> {
    keys.iter()
        .filter_map(|key| Some(((*key).to_string(), value.get(key)?.clone())))
        .collect()
}

/// A package Cargo locks without a source: a workspace member, or a path
/// dependency that is not one.
pub(crate) struct LocalPackage<'a> {
    pub(crate) package: &'a MetadataPackage,
    pub(crate) member: bool,
    /// The dependencies Cargo records for it. A member records every one it
    /// declares, as Cargo locks members with all their features and
    /// dev-dependencies. A path dependency outside the workspace records only
    /// the ones the features its dependents ask for activate, without its
    /// dev-dependencies.
    pub(crate) dependencies: Vec<RegistryDependency>,
}

/// The workspace members and every path dependency they reach, in the order
/// of `metadata.packages`.
///
/// `cargo metadata --no-deps` lists only the members, so the caller appends
/// the packages of path dependencies outside the workspace. A path
/// dependency on a package `metadata` does not list is not walked.
pub(crate) fn local_packages(metadata: &CargoMetadata) -> Result<Vec<LocalPackage<'_>>> {
    let mut locked = member_dependencies(metadata)?;
    let mut pending = locked
        .values()
        .flat_map(|dependencies| path_dependencies(dependencies))
        .collect::<VecDeque<_>>();
    let mut selections = BTreeMap::<usize, FeatureSelection>::new();
    while let Some(dependency) = pending.pop_front() {
        let Some(position) =
            path_package_position(metadata, &dependency.name, &dependency.requirement)
        else {
            continue;
        };
        let package = &metadata.packages[position];
        if metadata.workspace_members.contains(&package.id)
            || !widen_selection(&mut selections, &dependency, &position)
        {
            continue;
        }
        let dependencies = activated_dependencies(
            package,
            &selections[&position],
            DependencyOptions { include_dev: false, lockfile: true },
        )?;
        pending.extend(path_dependencies(&dependencies));
        locked.insert(position, dependencies);
    }
    Ok(locked
        .into_iter()
        .map(|(position, dependencies)| {
            let package = &metadata.packages[position];
            let member = metadata.workspace_members.contains(&package.id);
            LocalPackage { package, member, dependencies }
        })
        .collect())
}

/// The dependencies of each workspace member, by position, with all its
/// features and dev-dependencies.
fn member_dependencies(
    metadata: &CargoMetadata,
) -> Result<BTreeMap<usize, Vec<RegistryDependency>>> {
    metadata.packages
        .iter()
        .enumerate()
        .filter(|(_, package)| metadata.workspace_members.contains(&package.id))
        .map(|(position, package)| {
            let selection = FeatureSelection {
                default_features: true,
                features: package.features
                    .keys()
                    .cloned()
                    .collect(),
            };
            let options = DependencyOptions { include_dev: true, lockfile: true };
            Ok((position, activated_dependencies(package, &selection, options)?))
        })
        .collect()
}

/// The registry and git dependencies of every local package.
pub(crate) fn root_dependencies(metadata: &CargoMetadata) -> Result<Vec<RegistryDependency>> {
    Ok(local_packages(metadata)?
        .into_iter()
        .flat_map(|local| local.dependencies)
        .filter(|dependency| dependency.registry.is_some())
        .collect())
}

/// The position of the package a path dependency on `name` with `requirement`
/// points to, `None` when `metadata` does not list it.
pub(crate) fn path_package_position(
    metadata: &CargoMetadata,
    name: &str,
    requirement: &VersionReq,
) -> Option<usize> {
    metadata.packages
        .iter()
        .position(|package| {
            package.name == name && accepts_declared_version(requirement, None, &package.version)
        })
}

fn path_dependencies(
    dependencies: &[RegistryDependency],
) -> impl Iterator<Item = RegistryDependency> + '_ {
    dependencies
        .iter()
        .filter(|dependency| dependency.registry.is_none())
        .cloned()
}

fn activated_dependencies(
    package: &MetadataPackage,
    selection: &FeatureSelection,
    options: DependencyOptions,
) -> Result<Vec<RegistryDependency>> {
    let dependencies = package.dependencies
        .iter()
        .map(|dependency| RegistryDependency {
            alias: dependency.rename.clone().unwrap_or_else(|| dependency.name.clone()),
            name: dependency.name.clone(),
            requirement: dependency.req.clone(),
            kind: dependency.kind,
            registry: dependency.source.clone(),
            optional: dependency.optional,
            default_features: dependency.uses_default_features,
            features: dependency.features
                .iter()
                .cloned()
                .collect(),
        })
        .collect::<Vec<_>>();
    dependencies_from_parts(&dependencies, &package.features, selection, options)
}
