use super::{
    AddError, AddResolution, AddResolveInputs, AddView,
    aliasless::{AliaslessDependency, resolve_aliasless_specifier},
    manifest::apply_catalog_decision,
    registry::{pick_latest_range, resolve_explicit_registry_spec},
};
use crate::{CatalogModeDep, decide_catalog_outcome};
use pnpm_catalogs_types::Catalogs;
use pnpm_config::{Config, SaveWorkspaceProtocol};
use pnpm_engine_runtime_node_resolver::NodeResolver;
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_package_name::is_valid_dependency_alias;
use pnpm_registry::RangeSpecStyle;
use pnpm_reporter::LogEvent;
use pnpm_resolving_git_resolver::{HostedGit, HostedOpts};
use pnpm_resolving_jsr_specifier_parser::{JsrSpec, parse_jsr_specifier};
use pnpm_resolving_npm_resolver::{
    DeclaredSpecifiers, calc_specifier_for_workspace_dep, parse_bare_specifier,
    pick_matching_local_version_or_null, pick_registry_for_package,
};
use pnpm_resolving_resolver_base::WorkspacePackages;
use pnpm_workspace_range_resolver::resolve_workspace_range;
use pnpm_workspace_spec::WorkspaceSpec;

pub(super) struct ResolvedAddedDependency {
    pub(super) package_name: String,
    pub(super) manifest_specifier: String,
    pub(super) updated_catalogs: Catalogs,
    pub(super) warning: Option<LogEvent>,
}
pub(super) async fn resolve_added_dependency(
    package_selector: &str,
    manifest: &PackageManifest,
    inputs: &AddResolveInputs<'_, '_>,
) -> Result<ResolvedAddedDependency, AddError> {
    let selector = AddSelector::parse(package_selector, manifest, inputs).await?;
    let package_name = selector.package_name(package_selector);
    let prev_specifier = declared_specifier(manifest, package_name);
    let bare_specifier = bare_save_specifier(
        package_selector,
        &selector,
        prev_specifier.as_deref(),
        manifest,
        inputs,
    )
    .await?;
    let mut updated_catalogs = Catalogs::new();
    let outcome = decide_catalog_outcome(
        inputs.add.config.catalog_mode,
        inputs.save_catalog_name,
        inputs.catalogs,
        &CatalogModeDep {
            alias: package_name,
            bare_specifier: &bare_specifier,
            prev_specifier: prev_specifier.as_deref(),
        },
        inputs.prefix,
    )
    .map_err(AddError::CatalogVersionMismatch)?;
    let manifest_specifier = apply_catalog_decision(
        outcome.decision,
        package_name,
        bare_specifier,
        &mut updated_catalogs,
    );
    Ok(ResolvedAddedDependency {
        package_name: package_name.to_string(),
        manifest_specifier,
        updated_catalogs,
        warning: outcome.warning,
    })
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
/// - a `jsr:` selector is pinned the same way and rendered back under
///   its protocol — `pnpm add jsr:@scope/pkg` records `jsr:^1.2.3`;
/// - a re-add with no version keeps the dependency's current
///   specifier verbatim (a `catalog:` reference, a range, or an
///   exact pin) — `pnpm add <existing>` without a
///   version leaves the declared range untouched;
/// - a brand-new dependency fetches and pins the `latest` range.
pub(super) async fn bare_save_specifier(
    package_selector: &str,
    selector: &AddSelector,
    prev_specifier: Option<&str>,
    manifest: &PackageManifest,
    inputs: &AddResolveInputs<'_, '_>,
) -> Result<String, AddError> {
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
        return Ok(workspace_specifier);
    }
    if let Some(version_spec) = node_runtime_version_spec(package_name, explicit_spec) {
        return resolve_node_runtime_specifier(version_spec, prev_specifier, inputs).await;
    }
    if let Some(ProtocolSelector::Jsr(jsr)) = selector.protocol.as_ref() {
        return Ok(resolve_jsr_save_specifier(jsr, inputs.add, manifest, inputs.resolution)
            .await?
            .unwrap_or_else(|| package_selector.to_string()));
    }
    match (explicit_spec, prev_specifier) {
        (Some(spec), prev) => Ok(resolve_explicit_registry_spec(
            package_name,
            spec,
            prev,
            inputs.add,
            manifest,
            inputs.resolution,
        )
        .await?
        .unwrap_or_else(|| normalized_save_specifier(spec))),
        (None, Some(prev)) => Ok(prev.to_string()),
        (None, None) => pick_latest_range(package_name, inputs).await,
    }
}
pub(super) async fn resolve_node_runtime_specifier(
    version_spec: &str,
    prev_specifier: Option<&str>,
    inputs: &AddResolveInputs<'_, '_>,
) -> Result<String, AddError> {
    let config = inputs.add.config;
    let mut node_resolver = NodeResolver::new_with_auth(
        std::sync::Arc::clone(inputs.http_client_arc),
        std::sync::Arc::clone(&config.auth_headers),
    );
    node_resolver.node_download_mirrors.clone_from(&config.node_download_mirrors);
    node_resolver.offline = config.offline;
    node_resolver.cache_dir = Some(config.cache_dir.clone());
    node_resolver
        .resolve_save_specifier(version_spec, prev_specifier)
        .await
        .map_err(AddError::ResolveRuntimeSpec)
}
/// Index the workspace projects by name and version, or `None` when
/// there is no workspace or it cannot be enumerated.
///
/// A failure to walk the workspace is not this function's problem to
/// report — the install that follows the manifest write surfaces it with
/// far more context — so it degrades to "no workspace packages", which
/// only costs the pinned form its version.
pub(super) fn workspace_packages_for_add(config: &Config) -> Option<WorkspacePackages> {
    let workspace_dir = config.workspace_dir.as_ref()?;
    let manifest = pnpm_workspace::read_workspace_manifest(workspace_dir).ok()??;
    let projects = pnpm_workspace::find_workspace_projects(
        workspace_dir,
        &pnpm_workspace::FindWorkspaceProjectsOpts {
            patterns: Some(pnpm_workspace::workspace_package_patterns(&manifest)),
        },
    )
    .ok()?;
    crate::install::build_workspace_packages_map(Some(&projects))
}
/// The `workspace:` specifier to save for `package_name`, or `None`
/// when this add isn't a workspace dependency.
///
/// A relative `workspace:./pkg` is left alone: it names a directory, not
/// a range, so there is no operator to roll.
pub(super) fn workspace_save_specifier(
    package_name: &str,
    explicit_spec: Option<&str>,
    prev_specifier: Option<&str>,
    config: &Config,
    range_spec_style: RangeSpecStyle,
    workspace_packages: Option<&WorkspacePackages>,
) -> Option<String> {
    let (target_name, resolved_version) = match explicit_spec.and_then(WorkspaceSpec::parse) {
        Some(spec) => explicit_workspace_target(spec, package_name, workspace_packages)?,
        None => implicit_workspace_target(package_name, explicit_spec, config, workspace_packages)?,
    };
    let workspace_specifier = calc_specifier_for_workspace_dep(
        DeclaredSpecifiers { prev: prev_specifier, bare: explicit_spec },
        Some(package_name),
        &target_name,
        resolved_version.as_deref(),
        config.save_workspace_protocol,
        range_spec_style,
    );
    if config.save_workspace_protocol == SaveWorkspaceProtocol::Off
        && !explicit_spec.is_some_and(|specifier| specifier.starts_with("workspace:"))
    {
        return workspace_specifier.strip_prefix("workspace:").map(str::to_string);
    }
    Some(workspace_specifier)
}
/// The workspace package a `workspace:` specifier names. A path form
/// (`workspace:./pkg`) names no package here.
pub(super) fn explicit_workspace_target(
    spec: WorkspaceSpec,
    package_name: &str,
    workspace_packages: Option<&WorkspacePackages>,
) -> Option<(String, Option<String>)> {
    if spec.version.starts_with('.') {
        return None;
    }
    let target_name = spec.alias.unwrap_or_else(|| package_name.to_string());
    let resolved_version =
        workspace_packages.and_then(|packages| packages.get(&target_name)).and_then(|versions| {
            let available: Vec<String> = versions.keys().cloned().collect();
            // Not `spec.version`: the pinned form records the local
            // package's own version, which wins over the range the
            // user typed.
            resolve_workspace_range("*", &available)
        });
    Some((target_name, resolved_version))
}
/// The workspace package a plain registry request resolves to locally, when
/// `linkWorkspacePackages` lets it.
pub(super) fn implicit_workspace_target(
    package_name: &str,
    explicit_spec: Option<&str>,
    config: &Config,
    workspace_packages: Option<&WorkspacePackages>,
) -> Option<(String, Option<String>)> {
    if !config.link_workspace_packages.enabled_at_depth(0) {
        return None;
    }
    if explicit_spec.is_some_and(|specifier| specifier.starts_with("npm:")) {
        return None;
    }
    let registries: std::collections::HashMap<String, String> =
        config.resolved_registries().into_iter().collect();
    let registry = pick_registry_for_package(&registries, package_name, explicit_spec);
    let parsed = parse_bare_specifier(
        explicit_spec.unwrap_or("latest"),
        Some(package_name),
        "latest",
        &registry,
    )?;
    if parsed.name != package_name || parsed.normalized_bare_specifier.is_some() {
        return None;
    }
    let versions = workspace_packages?.get(package_name)?;
    let resolved_version = pick_matching_local_version_or_null(versions, &parsed)?;
    Some((package_name.to_string(), Some(resolved_version)))
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
    add: AddView<'_>,
    manifest: &PackageManifest,
    resolution: &AddResolution<'_>,
) -> Result<Option<String>, AddError> {
    let version_selector = spec.version_selector.as_deref().unwrap_or("latest");
    let range = resolve_explicit_registry_spec(
        &spec.npm_pkg_name,
        version_selector,
        // The range operator is read off the selector the user typed, the
        // way pnpm reads it for an alias-less request: there is no alias to
        // find a manifest entry by.
        None,
        add,
        manifest,
        resolution,
    )
    .await?;
    Ok(range.map(|range| format!("jsr:{range}")))
}
/// Split a `pacquet add` argument into its package name and optional
/// `@<version>` part. The version separator is the first `@` at or after
/// index 1, so a leading scope `@` (`@scope/pkg`) is never mistaken for a
/// version.
pub(super) fn split_name_spec(input: &str) -> (&str, Option<&str>) {
    match input.get(1..).and_then(|rest| rest.find('@')).map(|offset| offset + 1) {
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
/// The `<spec>` half of an explicit `node@runtime:<spec>` request, when that
/// is what's being added. Only the node resolver pins the saved specifier to
/// the picked version; deno and bun normalize to the requested spec, so they
/// stay on the verbatim save path.
pub(super) fn node_runtime_version_spec<'a>(
    package_name: &str,
    explicit_spec: Option<&'a str>,
) -> Option<&'a str> {
    if package_name != "node" {
        return None;
    }
    explicit_spec?.strip_prefix("runtime:")
}
