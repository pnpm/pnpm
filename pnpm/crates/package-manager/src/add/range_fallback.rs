use super::{
    AddError, AddOptions, AddResolveInputs,
    registry::{package_registry, parse_explicit_registry_spec},
};
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_resolving_resolver_base::WorkspacePackages;
use serde_json::Value;
use std::path::Path;

pub(super) fn needs_workspace_range_fallback(
    add: AddOptions<'_>,
    manifest: &PackageManifest,
) -> bool {
    add.package_names
        .iter()
        .any(|selector| {
            package_name_for_versionless_selector(selector)
                .is_some_and(|name| super::specifier::declared_specifier(manifest, &name).is_none())
        })
}

fn package_name_for_versionless_selector(selector: &str) -> Option<String> {
    let parsed = pnpm_resolving_parse_wanted_dependency::parse_wanted_dependency(selector);
    let target = parsed.bare_specifier.as_deref().unwrap_or(selector);
    if parsed.bare_specifier.as_deref().is_none_or(str::is_empty)
        && let Some(alias) = parsed.alias
    {
        return Some(alias);
    }
    let protocol_target = target
        .strip_prefix("npm:")
        .map(|_| target)
        .unwrap_or(selector);
    match super::specifier::ProtocolSelector::parse(protocol_target).ok().flatten()? {
        super::specifier::ProtocolSelector::Npm { name, spec }
            if spec.as_deref().is_none_or(str::is_empty) =>
        {
            Some(name)
        }
        _ => None,
    }
}

pub(super) fn add_range_fallback(
    package_name: &str,
    manifest: &PackageManifest,
    inputs: &AddResolveInputs<'_, '_>,
) -> Result<Option<String>, AddError> {
    let workspace_specifier = workspace_registry_specifier(package_name, inputs);
    let declared = workspace_specifier.as_deref().unwrap_or("*");
    Ok(override_specifier(package_name, declared, manifest, inputs)?.or(workspace_specifier))
}

fn workspace_registry_specifier(
    package_name: &str,
    inputs: &AddResolveInputs<'_, '_>,
) -> Option<String> {
    let registry = package_registry(inputs.add.config, package_name);
    workspace_declared_specifier(package_name, inputs.workspace_packages)
        .filter(|specifier| {
            parse_explicit_registry_spec(package_name, specifier, &registry).is_some()
        })
}

fn override_specifier(
    package_name: &str,
    declared: &str,
    manifest: &PackageManifest,
    inputs: &AddResolveInputs<'_, '_>,
) -> Result<Option<String>, AddError> {
    let Some(overrides) = inputs.add.config.overrides
        .as_ref()
        .filter(|map| !map.is_empty())
    else {
        return Ok(None);
    };
    let parsed =
        pnpm_config_parse_overrides::parse_overrides_iter(overrides.iter(), inputs.catalogs)
            .map_err(|error| AddError::Install(crate::InstallError::InvalidOverrides(error)))?;
    Ok(overridden_specifier(package_name, declared, manifest, inputs, &parsed))
}

fn overridden_specifier(
    package_name: &str,
    declared: &str,
    manifest: &PackageManifest,
    inputs: &AddResolveInputs<'_, '_>,
    parsed: &[pnpm_config_parse_overrides::VersionOverride],
) -> Option<String> {
    let root_dir = manifest
        .path()
        .parent()
        .unwrap_or_else(|| Path::new("."));
    let overrider = crate::overrides::VersionsOverrider::new(parsed, root_dir);
    let group = inputs.owned.dependency_groups
        .as_ref()
        .and_then(|groups| groups.first())
        .copied()
        .unwrap_or(DependencyGroup::Prod);
    let group_name: &str = group.into();
    let mut candidate = manifest.value().clone();
    candidate[group_name][package_name] = Value::String(declared.to_string());
    overrider.apply_to_value(&mut candidate, Some(root_dir));
    candidate
        .get(group_name)
        .and_then(|dependencies| dependencies.get(package_name))
        .and_then(Value::as_str)
        .filter(|specifier| *specifier != declared)
        .map(ToString::to_string)
}

fn workspace_declared_specifier(
    package_name: &str,
    workspace_packages: Option<&WorkspacePackages>,
) -> Option<String> {
    workspace_packages?
        .values()
        .flat_map(|packages| packages.values())
        .find_map(|package| {
            [
                DependencyGroup::Optional,
                DependencyGroup::Prod,
                DependencyGroup::Dev,
                DependencyGroup::Peer,
            ]
            .into_iter()
            .find_map(|group| {
                package.manifest
                    .get::<&str>(group.into())?
                    .get(package_name)?
                    .as_str()
                    .map(ToString::to_string)
            })
        })
}

#[cfg(test)]
mod tests;
