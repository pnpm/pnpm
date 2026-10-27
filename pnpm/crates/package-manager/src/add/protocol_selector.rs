use super::{
    AddError, AddResolveInputs,
    registry::resolve_explicit_registry_spec,
    specifier::{PickedPackage, SaveSpecifier, split_name_spec},
};
use pnpm_package_manifest::PackageManifest;
use pnpm_package_name::is_valid_dependency_alias;
use pnpm_resolving_jsr_specifier_parser::{JsrSpec, parse_jsr_specifier};
use pnpm_workspace_spec::WorkspaceSpec;

pub(super) enum ProtocolSelector {
    Npm { name: String, spec: Option<String> },
    Jsr(JsrSpec),
    Workspace { name: String },
}

impl ProtocolSelector {
    pub(super) fn parse(selector: &str) -> Result<Option<Self>, AddError> {
        if let Some(rest) = selector.strip_prefix("npm:") {
            let (name, spec) = split_name_spec(rest);
            let name = protocol_package_name(name, selector)?;
            return Ok(Some(Self::Npm { name, spec: spec.map(str::to_string) }));
        }
        if let Some(spec) =
            parse_jsr_specifier(selector, None).map_err(AddError::ParseJsrSpecifier)?
        {
            return Ok(Some(Self::Jsr(spec)));
        }
        let Some(alias) = WorkspaceSpec::parse(selector).and_then(|spec| spec.alias) else {
            return Ok(None);
        };
        let name = protocol_package_name(&alias, selector)?;
        Ok(Some(Self::Workspace { name }))
    }

    pub(super) fn package_name(&self) -> &str {
        match self {
            Self::Npm { name, .. } | Self::Workspace { name } => name,
            Self::Jsr(spec) => &spec.jsr_pkg_name,
        }
    }

    pub(super) fn explicit_spec<'a>(&'a self, selector: &'a str) -> Option<&'a str> {
        match self {
            Self::Npm { spec, .. } => spec.as_deref(),
            Self::Jsr(_) | Self::Workspace { .. } => Some(selector),
        }
    }
}

fn protocol_package_name(name: &str, selector: &str) -> Result<String, AddError> {
    if !is_valid_dependency_alias(name) {
        return Err(AddError::InvalidPackageName {
            specifier: selector.to_string(),
            name: name.to_string(),
        });
    }
    Ok(name.to_string())
}

pub(super) async fn resolve_jsr_save_specifier(
    spec: &JsrSpec,
    manifest: &PackageManifest,
    inputs: &AddResolveInputs<'_, '_>,
) -> Result<Option<SaveSpecifier>, AddError> {
    let inputs =
        AddResolveInputs { add: super::AddOptions { save_types: false, ..inputs.add }, ..*inputs };
    let version_selector =
        spec.version_selector.as_deref().unwrap_or_else(|| inputs.add.config.default_tag());
    let range = resolve_explicit_registry_spec(
        &spec.npm_pkg_name,
        version_selector,
        None,
        manifest,
        &inputs,
    )
    .await?;
    Ok(range.map(|picked| SaveSpecifier {
        specifier: format!("jsr:{}", picked.range),
        picked: Some(PickedPackage { name: spec.npm_pkg_name.clone(), version: picked.version }),
    }))
}
