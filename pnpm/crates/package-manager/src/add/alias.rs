use super::{
    AddError, AddResolveInputs,
    registry::package_registry,
    specifier::{ProtocolSelector, SaveSpecifier, explicit_registry_save_specifier},
};
use pnpm_package_manifest::PackageManifest;
use pnpm_resolving_npm_resolver::{RegistryPackageSpecType, parse_bare_specifier};

pub(super) async fn resolve_npm_alias_save_specifier(
    name: &str,
    spec: Option<String>,
    prev_specifier: Option<&str>,
    manifest: &PackageManifest,
    inputs: &AddResolveInputs<'_, '_>,
) -> Result<SaveSpecifier, AddError> {
    let previous_specifier =
        prev_specifier.and_then(|previous| previous_npm_alias_specifier(previous, name));
    let target_specifier = spec
        .as_deref()
        .filter(|spec| !spec.is_empty());
    if target_specifier.is_none()
        && let Some(previous_specifier) = previous_specifier.as_deref()
    {
        return Ok(format!("npm:{name}@{previous_specifier}").into());
    }
    let mut saved = explicit_registry_save_specifier(
        name,
        target_specifier.unwrap_or_else(|| inputs.add.config.default_tag()),
        previous_specifier.as_deref(),
        manifest,
        inputs,
    )
    .await?;
    let saved_target = target_specifier
        .filter(|specifier| !is_registry_tag(name, specifier, inputs))
        .unwrap_or(&saved.specifier);
    saved.specifier = format!("npm:{name}@{saved_target}");
    Ok(saved)
}

fn is_registry_tag(name: &str, specifier: &str, inputs: &AddResolveInputs<'_, '_>) -> bool {
    let registry = package_registry(inputs.add.config, name);
    parse_bare_specifier(specifier, Some(name), "latest", &registry)
        .is_some_and(|parsed| parsed.spec_type == RegistryPackageSpecType::Tag)
}

pub(super) fn npm_alias_target(
    package_selector: &str,
) -> Result<Option<ProtocolSelector>, AddError> {
    let parsed = pnpm_resolving_parse_wanted_dependency::parse_wanted_dependency(package_selector);
    if parsed.alias.is_none() {
        return Ok(None);
    }
    let Some(specifier) = parsed.bare_specifier.filter(|specifier| specifier.starts_with("npm:"))
    else {
        return Ok(None);
    };
    ProtocolSelector::parse(&specifier)
}

fn previous_npm_alias_specifier(previous: &str, name: &str) -> Option<String> {
    match ProtocolSelector::parse(previous).ok().flatten()? {
        ProtocolSelector::Npm { name: previous_name, spec } if previous_name == name => spec,
        _ => None,
    }
}
