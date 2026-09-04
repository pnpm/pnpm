use crate::{
    features::active_dependencies_from_parts,
    model::{CargoMetadata, FeatureSelection, MetadataPackage, RegistryDependency},
};
use miette::{IntoDiagnostic, Result, WrapErr};

pub(crate) fn parse_metadata(metadata: &str) -> Result<CargoMetadata> {
    serde_json::from_str(metadata).into_diagnostic().wrap_err("parse cargo metadata")
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
