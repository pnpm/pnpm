use super::{
    AddError, AddResolveInputs,
    registry::resolve_explicit_registry_spec,
    specifier::{PickedPackage, SaveSpecifier, split_name_spec},
};
use pnpm_package_manifest::PackageManifest;
use pnpm_package_name::is_valid_dependency_alias;
use pnpm_resolving_jsr_specifier_parser::{JsrSpec, parse_jsr_specifier};
use pnpm_workspace_spec::WorkspaceSpec;

/// A `pacquet add` argument that spells its package name *inside* a
/// dependency protocol, so the manifest key cannot be read off the front of
/// the argument: `pacquet add jsr:@scope/pkg` keys on `@scope/pkg`, not on
/// `jsr:`.
///
/// `catalog:` has no variant. The text after it names a catalog, not a
/// package, so `catalog:foo` carries no name to key an entry by.
pub(super) enum ProtocolSelector {
    /// `npm:<name>[@<spec>]`. Nothing here is aliased — the install name is
    /// the real package name — so the entry is saved as the plain
    /// `<name>[@<spec>]` request would be.
    Npm { name: String, spec: Option<String> },
    /// `jsr:@<scope>/<name>[@<selector>]`.
    Jsr(JsrSpec),
    /// The alias form `workspace:<name>@<range>`. A version-only
    /// (`workspace:^1.2.3`) or path (`workspace:./pkg`, `workspace:C:\pkg`)
    /// specifier names no package, and [`WorkspaceSpec`] already tells the
    /// three apart.
    Workspace { name: String },
}
impl ProtocolSelector {
    /// Returns `Ok(None)` for an argument that carries no name-bearing
    /// protocol, so the caller falls through to the alias-less resolvers
    /// and then to [`split_name_spec`].
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

    /// The specifier half to reconcile against the manifest and the
    /// catalogs. `jsr:` and `workspace:` keep the whole argument, since what
    /// is saved for them is rendered back under the protocol; an `npm:`
    /// request keeps only the registry range it wraps.
    pub(super) fn explicit_spec<'a>(&'a self, selector: &'a str) -> Option<&'a str> {
        match self {
            Self::Npm { spec, .. } => spec.as_deref(),
            Self::Jsr(_) | Self::Workspace { .. } => Some(selector),
        }
    }
}
pub(super) fn protocol_package_name(name: &str, selector: &str) -> Result<String, AddError> {
    if !is_valid_dependency_alias(name) {
        return Err(AddError::InvalidPackageName {
            specifier: selector.to_string(),
            name: name.to_string(),
        });
    }
    Ok(name.to_string())
}
/// The `jsr:` specifier to save for `spec`, with the version the install
/// will lock pinned into it: `pacquet add jsr:@scope/pkg` records
/// `jsr:^1.2.3` and `jsr:@scope/pkg@0.1` records `jsr:~0.1.0`.
///
/// JSR ships every package on npm under the `@jsr` scope, so the version is
/// picked through the same registry path a plain `@jsr/<scope>__<name>` add
/// takes. `None` when that pick finds no version — the caller then keeps the
/// argument verbatim, as it does for any other unresolvable specifier.
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
        // The range operator is read off the selector the user typed, the
        // way pnpm reads it for an alias-less request: there is no alias to
        // find a manifest entry by.
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
