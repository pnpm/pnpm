use super::{
    AddError, AddResolveInputs,
    aliasless::{AliaslessDependency, resolve_aliasless_specifier},
    manifest::apply_catalog_decision,
    registry::{pick_latest_range, resolve_explicit_registry_spec},
    workspace_specifier::workspace_save_specifier,
};
use crate::{
    CatalogModeDep, decide_catalog_outcome,
    runtime_specifier::{node_runtime_version_spec, runtime_save_specifier},
};
use pnpm_catalogs_types::Catalogs;
use pnpm_config::Tool;
use pnpm_engine_runtime_node_resolver::NodeResolver;
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_package_name::is_valid_dependency_alias;
use pnpm_reporter::LogEvent;
use pnpm_resolving_git_resolver::{HostedGit, HostedOpts};
use pnpm_resolving_jsr_specifier_parser::{JsrSpec, parse_jsr_specifier};
use pnpm_workspace_spec::WorkspaceSpec;

pub(super) struct ResolvedAddedDependency {
    pub(super) package_name: String,
    pub(super) manifest_specifier: String,
    pub(super) updated_catalogs: Catalogs,
    pub(super) warnings: Vec<LogEvent>,
    pub(super) types_selector: Option<String>,
    /// The version an explicit registry specifier resolved to, which the
    /// install must lock.
    pub(super) picked: Option<PickedPackage>,
}
/// A registry package version picked while resolving an added selector.
pub(super) struct PickedPackage {
    /// The registry name, which differs from the dependency's alias for a
    /// `jsr:` selector.
    pub(super) name: String,
    pub(super) version: String,
}
pub(super) async fn resolve_added_dependency(
    package_selector: &str,
    manifest: &PackageManifest,
    inputs: &AddResolveInputs<'_, '_>,
) -> Result<ResolvedAddedDependency, AddError> {
    let selector = AddSelector::parse(package_selector, manifest, inputs).await?;
    let package_name = selector.package_name(package_selector);
    let prev_specifier = declared_specifier(manifest, package_name);
    let SaveSpecifier { specifier: bare_specifier, picked } = bare_save_specifier(
        package_selector,
        &selector,
        prev_specifier.as_deref(),
        manifest,
        inputs,
    )
    .await?;
    let types_selector =
        super::types::resolve_types_selector(package_name, &bare_specifier, manifest, inputs)
            .await?;
    let (manifest_specifier, updated_catalogs, mut warnings) = resolve_catalog_and_warnings(
        inputs,
        package_name,
        bare_specifier,
        prev_specifier.as_deref(),
        package_selector,
    )?;
    warnings.extend(selector.linked_peer_dependencies_warning(package_name, inputs.prefix));
    Ok(ResolvedAddedDependency {
        package_name: package_name.to_string(),
        manifest_specifier,
        updated_catalogs,
        warnings,
        types_selector,
        picked,
    })
}

fn resolve_catalog_and_warnings(
    inputs: &AddResolveInputs<'_, '_>,
    package_name: &str,
    bare_specifier: String,
    prev_specifier: Option<&str>,
    package_selector: &str,
) -> Result<(String, Catalogs, Vec<LogEvent>), AddError> {
    let mut updated_catalogs = Catalogs::new();
    let outcome = decide_catalog_outcome(
        inputs.add.config.catalog_mode,
        inputs.owned.save_catalog_name.as_deref(),
        inputs.catalogs,
        &CatalogModeDep { alias: package_name, bare_specifier: &bare_specifier, prev_specifier },
        inputs.prefix,
    )
    .map_err(AddError::CatalogVersionMismatch)?;
    let manifest_specifier = apply_catalog_decision(
        outcome.decision,
        package_name,
        bare_specifier,
        &mut updated_catalogs,
    );
    let warnings = super::source::collect_dependency_warnings(
        outcome.warning,
        prev_specifier,
        package_selector,
        package_name,
        inputs.prefix,
    );
    Ok((manifest_specifier, updated_catalogs, warnings))
}
/// A selector as parsed: its protocol, and the package an aliasless
/// specifier names once resolved.
pub(super) struct AddSelector {
    protocol: Option<ProtocolSelector>,
    aliasless: Option<AliaslessDependency>,
}
impl AddSelector {
    async fn parse(
        package_selector: &str,
        manifest: &PackageManifest,
        inputs: &AddResolveInputs<'_, '_>,
    ) -> Result<Self, AddError> {
        let parsed =
            pnpm_resolving_parse_wanted_dependency::parse_wanted_dependency(package_selector);
        let protocol = ProtocolSelector::parse(package_selector)?;
        let aliasless = match (parsed.alias.as_deref(), parsed.bare_specifier.as_deref()) {
            (None, Some(specifier)) if protocol.is_none() => {
                resolve_aliasless_specifier(specifier, inputs, manifest).await?
            }
            _ => None,
        };
        Ok(Self { protocol, aliasless })
    }

    fn linked_peer_dependencies_warning(
        &self,
        package_name: &str,
        prefix: &str,
    ) -> Option<LogEvent> {
        let linked_manifest = self.aliasless.as_ref()?.linked_manifest.as_deref()?;
        crate::linked_peer_dependencies_warning(package_name, linked_manifest, prefix)
    }

    fn package_name<'s>(&'s self, package_selector: &'s str) -> &'s str {
        match (self.aliasless.as_ref(), self.protocol.as_ref()) {
            (Some(dep), _) => dep.package_name.as_str(),
            (None, Some(protocol)) => protocol.package_name(),
            (None, None) => split_name_spec(package_selector).0,
        }
    }

    fn explicit_spec<'s>(&'s self, package_selector: &'s str) -> Option<&'s str> {
        match (self.aliasless.as_ref(), self.protocol.as_ref()) {
            (Some(dep), _) => Some(dep.manifest_specifier.as_str()),
            (None, Some(protocol)) => protocol.explicit_spec(package_selector),
            (None, None) => split_name_spec(package_selector).1,
        }
        // pnpm reads `foo@` as `foo`, which resolves the `tag` setting.
        .filter(|spec| !spec.is_empty())
    }
}
/// The dependency's current specifier, so a re-add keeps the
/// existing range / `catalog:` reference rather than re-pinning to
/// `^<latest>`. The scan order matches pnpm's `findSpec` /
/// `guessDependencyType` (`DEPENDENCIES_OR_PEER_FIELDS`):
/// `optionalDependencies`, `dependencies`, `devDependencies`,
/// `peerDependencies` — the first-found specifier wins even when the
/// add targets a different group ([`PackageManifest::add_dependency`]
/// then removes the entry from its old group).
pub(super) fn declared_specifier(manifest: &PackageManifest, package_name: &str) -> Option<String> {
    manifest
        .dependencies([
            DependencyGroup::Optional,
            DependencyGroup::Prod,
            DependencyGroup::Dev,
            DependencyGroup::Peer,
        ])
        .find(|(name, _)| *name == package_name)
        .map(|(_, spec)| spec.to_string())
}
/// The bare specifier to reconcile against the catalogs:
/// - an explicit `@<version>` is resolved to a concrete version and
///   recorded with the range operator it (or the existing entry)
///   pins — `pnpm add foo@^7` records `^7.8.4`, not
///   `^7`. Specifiers that aren't a plain registry range/tag/version
///   for this package (protocols, `npm:` aliases) stay verbatim;
/// - an explicit `node@runtime:<spec>` is likewise pinned to the
///   picked Node.js version, so the `devEngines.runtime` entry the
///   saved dependency folds into records e.g. `26.5.0`, not the
///   requested `26`;
/// - an explicit `deno` / `bun` `tool:` or `runtime:` request is saved as
///   `runtime:`, the spelling its folded declaration reads back as;
/// - a `jsr:` selector is pinned the same way and rendered back under
///   its protocol — `pnpm add jsr:@scope/pkg` records `jsr:^1.2.3`;
/// - a re-add with no version keeps the dependency's current
///   specifier verbatim (a `catalog:` reference, a range, or an
///   exact pin) — `pnpm add <existing>` without a
///   version leaves the declared range untouched;
/// - a brand-new dependency the catalog already lists takes the
///   `catalog:` reference — naming no version asks for whatever the
///   workspace agreed on, which is the entry, not the `latest` range
///   it happens to resolve to today;
/// - any other brand-new dependency fetches and pins the range of the
///   configured `tag`, `latest` by default.
pub(super) async fn bare_save_specifier(
    package_selector: &str,
    selector: &AddSelector,
    prev_specifier: Option<&str>,
    manifest: &PackageManifest,
    inputs: &AddResolveInputs<'_, '_>,
) -> Result<SaveSpecifier, AddError> {
    let package_name = selector.package_name(package_selector);
    let explicit_spec = selector.explicit_spec(package_selector);
    if let Some(workspace_specifier) = workspace_save_specifier(
        package_name,
        explicit_spec,
        prev_specifier,
        inputs.add.config,
        inputs.add.range_spec_style,
        inputs.workspace_packages,
    ) {
        return Ok(workspace_specifier.into());
    }
    if let Some(version_spec) =
        explicit_spec.and_then(|spec| node_runtime_version_spec(package_name, spec))
    {
        return resolve_node_runtime_specifier(version_spec, prev_specifier, inputs)
            .await
            .map(SaveSpecifier::from);
    }
    if let Some(specifier) =
        explicit_spec.and_then(|spec| runtime_save_specifier(package_name, spec))
    {
        return Ok(specifier.into());
    }
    if let Some(ProtocolSelector::Jsr(jsr)) = selector.protocol.as_ref() {
        return Ok(resolve_jsr_save_specifier(jsr, manifest, inputs).await?
            .unwrap_or_else(|| package_selector.to_string().into()));
    }
    registry_save_specifier(package_name, explicit_spec, prev_specifier, manifest, inputs).await
}
/// [`bare_save_specifier`] for a plain registry selector.
async fn registry_save_specifier(
    package_name: &str,
    explicit_spec: Option<&str>,
    prev_specifier: Option<&str>,
    manifest: &PackageManifest,
    inputs: &AddResolveInputs<'_, '_>,
) -> Result<SaveSpecifier, AddError> {
    match (explicit_spec, prev_specifier) {
        (Some(spec), prev) => {
            explicit_registry_save_specifier(package_name, spec, prev, manifest, inputs).await
        }
        (None, Some(prev)) => Ok(prev.to_string().into()),
        (None, None) => match cataloged_specifier(package_name, inputs) {
            Some(specifier) => Ok(specifier.into()),
            None => match inputs.add.config.default_tag() {
                // The default tag keeps the shared picker every plain add uses.
                "latest" => {
                    pick_latest_range(package_name, manifest, inputs).await.map(SaveSpecifier::from)
                }
                tag => {
                    explicit_registry_save_specifier(package_name, tag, None, manifest, inputs)
                        .await
                }
            },
        },
    }
}
/// [`registry_save_specifier`] for a registry selector that names `spec`.
async fn explicit_registry_save_specifier(
    package_name: &str,
    spec: &str,
    prev_specifier: Option<&str>,
    manifest: &PackageManifest,
    inputs: &AddResolveInputs<'_, '_>,
) -> Result<SaveSpecifier, AddError> {
    Ok(
        match resolve_explicit_registry_spec(package_name, spec, prev_specifier, manifest, inputs)
            .await?
        {
            Some(picked) => SaveSpecifier {
                specifier: picked.range,
                picked: Some(PickedPackage {
                    name: package_name.to_string(),
                    version: picked.version,
                }),
            },
            None => normalized_save_specifier(spec).into(),
        },
    )
}
/// What [`bare_save_specifier`] records, and the version it resolved an
/// explicit registry specifier to.
pub(super) struct SaveSpecifier {
    specifier: String,
    picked: Option<PickedPackage>,
}
impl From<String> for SaveSpecifier {
    fn from(specifier: String) -> Self {
        Self { specifier, picked: None }
    }
}
/// The `catalog:` reference for a dependency the catalog already lists, or
/// `None` when it lists no entry for it.
fn cataloged_specifier(package_name: &str, inputs: &AddResolveInputs<'_, '_>) -> Option<String> {
    let catalog_name = crate::per_dep_catalog_name(None, inputs.owned.save_catalog_name.as_deref());
    inputs.catalogs.get(catalog_name)?.get(package_name)?;
    Some(if catalog_name == pnpm_catalogs_types::DEFAULT_CATALOG_NAME {
        "catalog:".to_string()
    } else {
        format!("catalog:{catalog_name}")
    })
}

pub(super) async fn resolve_node_runtime_specifier(
    version_spec: &str,
    prev_specifier: Option<&str>,
    inputs: &AddResolveInputs<'_, '_>,
) -> Result<String, AddError> {
    let config = inputs.add.config;
    let mut node_resolver = NodeResolver::new_with_auth(
        std::sync::Arc::clone(&inputs.owned.http_client_arc),
        std::sync::Arc::clone(&config.auth_headers),
    );
    node_resolver.node_download_mirrors.clone_from(&config.node_download_mirrors);
    node_resolver.mirror = config.tool_mirror(Tool::Node).map(ToString::to_string);
    node_resolver.channel_mirrors = config.tool_channel_mirrors(Tool::Node);
    node_resolver.offline = config.offline;
    node_resolver.cache_dir = Some(config.cache_dir.clone());
    node_resolver
        .resolve_save_specifier(version_spec, prev_specifier)
        .await
        .map_err(AddError::ResolveRuntimeSpec)
}
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
/// Split a `pacquet add` argument into its package name and optional
/// `@<version>` part. The version separator is the first `@` at or after
/// index 1, so a leading scope `@` (`@scope/pkg`) is never mistaken for a
/// version.
pub(super) fn split_name_spec(input: &str) -> (&str, Option<&str>) {
    match input
        .get(1..)
        .and_then(|rest| rest.find('@'))
        .map(|offset| offset + 1)
    {
        Some(idx) => (&input[..idx], Some(&input[idx + 1..])),
        None => (input, None),
    }
}
/// The specifier `pacquet add <name>@<spec>` saves when `<spec>` isn't a plain
/// registry range. A hosted-git request — a bare `owner/repo#committish`
/// shorthand or a GitHub / GitLab / Bitbucket URL — is rewritten to its
/// `github:` / `gitlab:` / `bitbucket:` shortcut form. Everything else
/// (`file:`, `link:`, `workspace:`, `npm:` aliases, tarball URLs) is
/// kept verbatim.
///
/// An auth-bearing HTTPS URL (`git+https://<token>@github.com/...`) is also
/// kept verbatim: the shortcut form cannot carry userinfo, so shortcutting
/// would silently drop the credentials the follow-up install needs to reach a
/// private repo. This mirrors the git resolver, which keeps such URLs in a
/// `git+https` form rather than shortcutting them
/// (see `parse_bare_specifier`'s `hosted.auth.is_some()` branch).
pub(super) fn normalized_save_specifier(spec: &str) -> String {
    match HostedGit::from_url(spec) {
        Some(hosted) if hosted.auth.is_none() => hosted.shortcut(HostedOpts::default()),
        _ => spec.to_string(),
    }
}
