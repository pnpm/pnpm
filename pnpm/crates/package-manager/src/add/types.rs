use super::{
    AddError, AddResolveInputs,
    registry::{explicit_registry_pick_options, package_registry},
    specifier::declared_specifier,
};
use crate::resolution_policy::{PickPolicy, pick_package_context};
use pnpm_lockfile_preferred_versions::get_preferred_versions_from_lockfile_and_manifests;
use pnpm_package_manifest::PackageManifest;
use pnpm_registry::PackageVersion;
use pnpm_resolving_npm_resolver::{
    FetchMetadataError, PickPackageError, calc_version_range, parse_bare_specifier, pick_package,
};
use std::sync::Arc;

pub(super) async fn resolve_types_selector(
    package_name: &str,
    specifier: &str,
    manifest: &PackageManifest,
    inputs: &AddResolveInputs<'_, '_>,
) -> Result<Option<String>, AddError> {
    if !inputs.add.save_types || package_name.starts_with("@types/") {
        return Ok(None);
    }
    let types_alias = types_package_name(package_name);
    if types_already_requested(&types_alias, manifest, inputs) {
        return Ok(None);
    }
    let Some(specifier) = catalog_specifier(package_name, specifier, inputs) else {
        return Ok(None);
    };
    let preferred_versions = get_preferred_versions_from_lockfile_and_manifests(
        inputs.add.lockfile.document.and_then(|lockfile| lockfile.snapshots.as_ref()),
        &[manifest],
    );
    let Some(package) =
        pick_types_metadata(package_name, specifier, inputs, Some(&preferred_versions)).await?
    else {
        return Ok(None);
    };
    if package.name.starts_with("@types/") || has_bundled_types(&package) {
        return Ok(None);
    }
    resolve_companion_selector(&types_alias, &types_package_name(&package.name), inputs).await
}

async fn resolve_companion_selector(
    types_alias: &str,
    types_name: &str,
    inputs: &AddResolveInputs<'_, '_>,
) -> Result<Option<String>, AddError> {
    let catalog_name = crate::per_dep_catalog_name(None, inputs.save_catalog_name);
    let catalog_entry = inputs.catalogs
        .get(catalog_name)
        .and_then(|entries| entries.get(types_alias));
    let specifier = catalog_entry.map_or("latest", String::as_str);
    let types_package = match pick_types_metadata(types_name, specifier, inputs, None).await {
        Ok(package) => package,
        Err(AddError::ResolveSpec(error)) if is_not_found(&error) => return Ok(None),
        Err(error) => return Err(error),
    };
    Ok(types_package.map(|package| {
        if catalog_entry.is_some() {
            return types_alias.to_string();
        }
        let range = calc_version_range(&package.version, None, None, inputs.add.range_spec_style);
        if types_alias == types_name {
            format!("{types_alias}@{range}")
        } else {
            format!("{types_alias}@npm:{types_name}@{range}")
        }
    }))
}

fn types_already_requested(
    name: &str,
    manifest: &PackageManifest,
    inputs: &AddResolveInputs<'_, '_>,
) -> bool {
    declared_specifier(manifest, name).is_some()
        || inputs.add.package_names
            .iter()
            .any(|selector| {
                pnpm_resolving_parse_wanted_dependency::parse_wanted_dependency(
                    selector.strip_prefix("npm:").unwrap_or(selector),
                )
                .alias
                .as_deref()
                    == Some(name)
            })
}

fn catalog_specifier<'a>(
    name: &str,
    specifier: &'a str,
    inputs: &'a AddResolveInputs<'_, '_>,
) -> Option<&'a str> {
    match pnpm_catalogs_protocol_parser::parse_catalog_protocol(specifier) {
        Some(catalog) => inputs.catalogs
            .get(catalog)?
            .get(name)
            .map(String::as_str),
        None => Some(specifier),
    }
}

async fn pick_types_metadata(
    name: &str,
    specifier: &str,
    inputs: &AddResolveInputs<'_, '_>,
    preferred_versions: Option<&pnpm_resolving_resolver_base::PreferredVersions>,
) -> Result<Option<Arc<PackageVersion>>, AddError> {
    let registry = package_registry(inputs.add.config, name);
    let Some(spec) = parse_bare_specifier(specifier, Some(name), "latest", &registry)
        .filter(|spec| spec.normalized_bare_specifier.is_none())
    else {
        return Ok(None);
    };
    let registry = package_registry(inputs.add.config, &spec.name);
    let mut policy =
        PickPolicy::from_config(inputs.add.config).map_err(AddError::MinimumReleaseAgeExclude)?;
    policy.force_unfiltered_full_metadata();
    let context = pick_package_context(
        inputs.add.http_client,
        inputs.add.config,
        &policy,
        &inputs.resolution.meta_cache,
        &inputs.resolution.fetch_locker,
    );
    let options = explicit_registry_pick_options(
        inputs.add.config,
        &registry,
        &policy,
        preferred_versions.and_then(|preferred| preferred.get(&spec.name)),
    );
    pick_package(&context, &spec, &options).await
        .map(|result| result.picked_package)
        .map_err(|error| AddError::ResolveSpec(Box::new(error)))
}

fn types_package_name(name: &str) -> String {
    match name.strip_prefix('@') {
        Some(scoped) => format!("@types/{}", scoped.replace('/', "__")),
        None => format!("@types/{name}"),
    }
}

fn has_bundled_types(package: &PackageVersion) -> bool {
    ["types", "typings"]
        .iter()
        .any(|field| {
            package.other
                .get(*field)
                .and_then(serde_json::Value::as_str)
                .is_some_and(|path| !path.is_empty())
        })
        || package.other.get("exports").is_some_and(exports_types)
}

fn exports_types(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Object(exports) => exports
            .iter()
            .any(|(condition, target)| {
                ((condition == "types" || condition.starts_with("types@"))
                    && has_export_target(target))
                    || exports_types(target)
            }),
        serde_json::Value::Array(targets) => targets.iter().any(exports_types),
        _ => false,
    }
}

fn has_export_target(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::String(path) => !path.is_empty(),
        serde_json::Value::Array(targets) => targets.iter().any(has_export_target),
        serde_json::Value::Object(conditions) => conditions.values().any(has_export_target),
        _ => false,
    }
}

fn is_not_found(error: &PickPackageError) -> bool {
    matches!(error, PickPackageError::Fetch(FetchMetadataError::Network { error, .. }) if error.status().is_some_and(|status| status.as_u16() == 404))
}
