use super::{
    AddError, AddOptions, AddResolveInputs,
    protocol_selector::ProtocolSelector,
    registry::{package_registry, parse_explicit_registry_spec},
    specifier::listing_catalog_name,
};
use pnpm_catalogs_types::Catalogs;
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_resolving_resolver_base::WorkspacePackages;
use serde_json::Value;
use std::path::Path;

/// Whether some selector names no version for a dependency that neither the
/// manifest nor the catalog `pnpm add` saves to declares, so the range
/// fallback needs the workspace packages.
pub(super) fn needs_workspace_range_fallback(
    add: AddOptions<'_>,
    save_catalog_name: Option<&str>,
    catalogs: &Catalogs,
    manifest: &PackageManifest,
) -> bool {
    add.package_names
        .iter()
        .filter_map(|selector| VersionlessSelector::parse(selector))
        .any(|selector| match selector {
            VersionlessSelector::Registry(name) => {
                super::specifier::declared_specifier(manifest, &name).is_none()
                    && listing_catalog_name(&name, catalogs, save_catalog_name).is_none()
            }
            VersionlessSelector::NpmAlias(alias) => {
                super::specifier::declared_specifier(manifest, &alias).is_none()
            }
        })
}

/// A selector that names no version, by the manifest key it is saved under.
#[derive(Debug, PartialEq)]
enum VersionlessSelector {
    /// `foo` or `npm:foo`.
    Registry(String),
    /// `alias@npm:foo`, keyed by `alias`. A catalog never supplies its range.
    NpmAlias(String),
}

impl VersionlessSelector {
    fn parse(selector: &str) -> Option<Self> {
        let parsed = pnpm_resolving_parse_wanted_dependency::parse_wanted_dependency(selector);
        let Some(specifier) = parsed.bare_specifier.filter(|specifier| !specifier.is_empty())
        else {
            return parsed.alias.map(Self::Registry);
        };
        match ProtocolSelector::parse(&specifier).ok().flatten()? {
            ProtocolSelector::Npm { name, spec } if spec.as_deref().is_none_or(str::is_empty) => {
                Some(parsed.alias.map_or(Self::Registry(name), Self::NpmAlias))
            }
            _ => None,
        }
    }
}

/// The range to save for a registry selector that names no version: the
/// workspace range, else the range a matching override sets.
pub(super) fn add_range_fallback(
    package_name: &str,
    manifest: &PackageManifest,
    inputs: &AddResolveInputs<'_, '_>,
) -> Result<Option<String>, AddError> {
    if let Some(specifier) = workspace_range_fallback(package_name, inputs) {
        return Ok(Some(specifier));
    }
    override_specifier(package_name, manifest, inputs)
}

/// The range every workspace project that declares `package_name` agrees
/// on, when it is a registry range.
pub(super) fn workspace_range_fallback(
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
    let project_dir = manifest
        .path()
        .parent()
        .expect("manifest path always has a parent dir");
    let lockfile_root = crate::install::lockfile_root_dir(inputs.add.config, project_dir)
        .map_err(AddError::FindWorkspaceDir)?;
    let overrider = crate::overrides::VersionsOverrider::new(
        &parsed,
        inputs.add.config.root_project_manifest_dir(&lockfile_root),
    );
    Ok(overridden_specifier(package_name, project_dir, &overrider, inputs, manifest))
}

/// The specifier `overrider` rewrites an unconstrained `package_name` to,
/// with relative paths re-based onto `project_dir`.
fn overridden_specifier(
    package_name: &str,
    project_dir: &Path,
    overrider: &crate::overrides::VersionsOverrider,
    inputs: &AddResolveInputs<'_, '_>,
    manifest: &PackageManifest,
) -> Option<String> {
    let group = inputs.owned.dependency_groups
        .as_ref()
        .and_then(|groups| groups.first())
        .copied()
        .unwrap_or(DependencyGroup::Prod);
    let group_name: &str = group.into();
    const DECLARED: &str = "*";
    let mut candidate = manifest.value().clone();
    candidate[group_name][package_name] = Value::String(DECLARED.to_string());
    overrider.apply_to_value(&mut candidate, Some(project_dir));
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
