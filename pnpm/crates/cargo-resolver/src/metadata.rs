use crate::{
    features::active_dependencies_from_parts,
    model::{CargoMetadata, FeatureSelection, MetadataPackage, RegistryDependency},
};
use miette::{IntoDiagnostic, Result, WrapErr};
use std::collections::BTreeMap;

pub(crate) fn parse_metadata(metadata: &str) -> Result<CargoMetadata> {
    serde_json::from_str(metadata).into_diagnostic().wrap_err("parse cargo metadata")
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
/// so a document that leaves the machine — to a pnpr server resolving on the
/// client's behalf — carries the dependency graph alone.
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
        .map(|package| {
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
        })
        .collect::<Vec<_>>();
    let workspace_members = document
        .get("workspace_members")
        .and_then(serde_json::Value::as_array)
        .map(|members| {
            members
                .iter()
                .filter_map(|member| ids.get(member.as_str()?).map(String::as_str))
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

/// `value`'s object entries whose keys are in `keys`, dropping the rest.
fn retained_keys(
    value: &serde_json::Value,
    keys: &[&str],
) -> serde_json::Map<String, serde_json::Value> {
    keys.iter().filter_map(|key| Some(((*key).to_string(), value.get(key)?.clone()))).collect()
}

pub(crate) fn root_dependencies(metadata: &CargoMetadata) -> Result<Vec<RegistryDependency>> {
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

pub(crate) fn active_metadata_dependencies(
    package: &MetadataPackage,
) -> Result<Vec<RegistryDependency>> {
    let dependencies = package
        .dependencies
        .iter()
        .map(|dependency| RegistryDependency {
            alias: dependency.rename.clone().unwrap_or_else(|| dependency.name.clone()),
            name: dependency.name.clone(),
            requirement: dependency.req.clone(),
            kind: dependency.kind,
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
