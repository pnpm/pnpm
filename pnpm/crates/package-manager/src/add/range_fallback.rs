use super::{
    AddError, AddOptions, AddResolveInputs,
    protocol_selector::ProtocolSelector,
    registry::{package_registry, parse_explicit_registry_spec},
};
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_resolving_resolver_base::WorkspacePackages;
use serde_json::Value;
use std::path::Path;

/// Whether some selector names no version for a dependency the manifest
/// does not declare yet, so [`add_range_fallback`] needs the workspace
/// packages.
pub(super) fn needs_workspace_range_fallback(
    add: AddOptions<'_>,
    manifest: &PackageManifest,
) -> bool {
    add.package_names
        .iter()
        .filter_map(|selector| versionless_manifest_key(selector))
        .any(|key| super::specifier::declared_specifier(manifest, &key).is_none())
}

/// The manifest key of a selector that names no version: `foo`, `npm:foo`,
/// or `alias@npm:foo`.
fn versionless_manifest_key(selector: &str) -> Option<String> {
    let parsed = pnpm_resolving_parse_wanted_dependency::parse_wanted_dependency(selector);
    let Some(specifier) = parsed.bare_specifier.filter(|specifier| !specifier.is_empty()) else {
        return parsed.alias;
    };
    match ProtocolSelector::parse(&specifier).ok().flatten()? {
        ProtocolSelector::Npm { name, spec } if spec.as_deref().is_none_or(str::is_empty) => {
            Some(parsed.alias.unwrap_or(name))
        }
        _ => None,
    }
}

pub(super) fn add_range_fallback(
    package_name: &str,
    manifest: &PackageManifest,
    inputs: &AddResolveInputs<'_, '_>,
) -> Result<Option<String>, AddError> {
    if let Some(specifier) = workspace_registry_specifier(package_name, inputs) {
        return Ok(Some(specifier));
    }
    override_specifier(package_name, manifest, inputs)
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

/// The specifier the overrides rewrite an unconstrained `package_name` to.
fn override_specifier(
    package_name: &str,
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
    Ok(overridden_specifier(package_name, manifest, inputs, &parsed))
}

fn overridden_specifier(
    package_name: &str,
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
    const DECLARED: &str = "*";
    let mut candidate = manifest.value().clone();
    candidate[group_name][package_name] = Value::String(DECLARED.to_string());
    overrider.apply_to_value(&mut candidate, Some(root_dir));
    candidate
        .get(group_name)
        .and_then(|dependencies| dependencies.get(package_name))
        .and_then(Value::as_str)
        .filter(|specifier| *specifier != DECLARED)
        .map(ToString::to_string)
}

/// The one specifier every workspace project that declares `package_name`
/// agrees on. `None` when the projects disagree or one of them uses a
/// protocol.
fn workspace_declared_specifier(
    package_name: &str,
    workspace_packages: Option<&WorkspacePackages>,
) -> Option<String> {
    let mut agreed: Option<&str> = None;
    for package in workspace_packages?.values().flat_map(|packages| packages.values()) {
        let Some(specifier) = project_specifier(&package.manifest, package_name) else {
            continue;
        };
        if specifier.contains(':') || agreed.is_some_and(|agreed| agreed != specifier) {
            return None;
        }
        agreed = Some(specifier);
    }
    agreed.map(ToString::to_string)
}

/// The specifier a project's own dependencies declare for `package_name`,
/// read in the precedence a dependency declared in several groups resolves by.
fn project_specifier<'a>(manifest: &'a Value, package_name: &str) -> Option<&'a str> {
    [DependencyGroup::Optional, DependencyGroup::Prod, DependencyGroup::Dev]
        .into_iter()
        .find_map(|group| {
            manifest
                .get::<&str>(group.into())?
                .get(package_name)?
                .as_str()
        })
}

#[cfg(test)]
mod tests;
