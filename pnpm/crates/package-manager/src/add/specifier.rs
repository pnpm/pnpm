pub(super) use super::protocol_selector::ProtocolSelector;
use super::{
    AddError, AddResolveInputs,
    alias::{npm_alias_target, resolve_npm_alias_save_specifier},
    manifest::apply_catalog_decision,
    protocol_selector::resolve_jsr_save_specifier,
    range_fallback::add_range_fallback,
    registry::{pick_latest_range, resolve_explicit_registry_spec},
    selector::AddSelector,
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
use pnpm_reporter::LogEvent;
use pnpm_resolving_git_resolver::{HostedGit, HostedOpts};

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
    let (manifest_specifier, mut updated_catalogs, mut warnings) = resolve_catalog_and_warnings(
        inputs,
        package_name,
        bare_specifier,
        prev_specifier.as_deref(),
        package_selector,
        picked.as_ref(),
    )?;
    if let Some(picked) =
        picked.as_ref().filter(|_| pins_dist_tag(package_selector, inputs.add.config.default_tag()))
    {
        pin_catalog_entry(&mut updated_catalogs, package_name, &picked.version);
    }
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

/// Whether the selector asks for a dist-tag other than `latest`, whose new
/// catalog entry [`pin_catalog_entry`] pins.
fn pins_dist_tag(package_selector: &str, default_tag: &str) -> bool {
    super::registry::requested_dist_tag(package_selector, Some(default_tag))
        .is_some_and(|tag| tag != "latest")
}

/// A catalog entry written for a dist-tag records the exact version the tag
/// pointed to, so the catalog does not drift when the tag moves.
fn pin_catalog_entry(updated_catalogs: &mut Catalogs, package_name: &str, version: &str) {
    for entry in updated_catalogs
        .values_mut()
        .filter_map(|catalog| catalog.get_mut(package_name))
    {
        *entry = version.to_string();
    }
}

fn resolve_catalog_and_warnings(
    inputs: &AddResolveInputs<'_, '_>,
    package_name: &str,
    bare_specifier: String,
    prev_specifier: Option<&str>,
    package_selector: &str,
    picked: Option<&PickedPackage>,
) -> Result<(String, Catalogs, Vec<LogEvent>), AddError> {
    let mut updated_catalogs = Catalogs::new();
    // A tag resolved locally through `linkWorkspacePackages` names no registry
    // version, so it cannot match a catalog entry.
    let requested_dist_tag =
        picked.and_then(|_| super::registry::requested_dist_tag(package_selector, None));
    let outcome = decide_catalog_outcome(
        inputs.add.config.catalog_mode,
        inputs.owned.save_catalog_name.as_deref(),
        inputs.catalogs,
        &CatalogModeDep {
            alias: package_name,
            bare_specifier: &bare_specifier,
            requested_dist_tag,
            prev_specifier,
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
    let warnings = super::source::collect_dependency_warnings(
        outcome.warning,
        prev_specifier,
        package_selector,
        package_name,
        inputs.prefix,
    );
    Ok((manifest_specifier, updated_catalogs, warnings))
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
///   `^7`. An `npm:` alias records its target the same way —
///   `pnpm add v2@npm:foo@1` records `npm:foo@^1.0.0`. Other protocols
///   stay verbatim;
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
/// - any other brand-new dependency takes the range the workspace
///   projects agree on, or the one a matching override sets
///   ([`add_range_fallback`]), and otherwise fetches and pins the range of
///   the configured `tag`, `latest` by default.
pub(super) async fn bare_save_specifier(
    package_selector: &str,
    selector: &AddSelector,
    prev_specifier: Option<&str>,
    manifest: &PackageManifest,
    inputs: &AddResolveInputs<'_, '_>,
) -> Result<SaveSpecifier, AddError> {
    let package_name = selector.package_name(package_selector);
    let explicit_spec = selector.explicit_spec(package_selector);
    if let Some(ProtocolSelector::Npm { name, spec }) = npm_alias_target(package_selector)? {
        return resolve_npm_alias_save_specifier(&name, spec, prev_specifier, manifest, inputs)
            .await;
    }
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
        (None, None) => {
            if let Some(specifier) = cataloged_specifier(package_name, inputs) {
                return Ok(specifier.into());
            }
            if let Some(range) = add_range_fallback(package_name, manifest, inputs)? {
                return resolve_fallback_range(package_name, &range, manifest, inputs).await;
            }
            match inputs.add.config.default_tag() {
                // The default tag keeps the shared picker every plain add uses.
                "latest" => {
                    pick_latest_range(package_name, manifest, inputs).await.map(SaveSpecifier::from)
                }
                tag => {
                    explicit_registry_save_specifier(package_name, tag, None, manifest, inputs)
                        .await
                }
            }
        }
    }
}
/// A fallback `range` resolved and re-rendered in the configured range
/// style.
pub(super) async fn resolve_fallback_range(
    package_name: &str,
    range: &str,
    manifest: &PackageManifest,
    inputs: &AddResolveInputs<'_, '_>,
) -> Result<SaveSpecifier, AddError> {
    let mut resolved =
        explicit_registry_save_specifier(package_name, range, None, manifest, inputs).await?;
    if let Some(picked) = &resolved.picked {
        let version = picked.version.parse().expect("picked versions are valid");
        let previous_specifier = match pnpm_resolving_npm_resolver::infer_range_spec_style(range) {
            Some(pnpm_registry::RangeSpecStyle::Patch | pnpm_registry::RangeSpecStyle::Exact) => {
                None
            }
            _ => Some(range),
        };
        resolved.specifier = pnpm_resolving_npm_resolver::calc_version_range(
            &version,
            previous_specifier,
            None,
            inputs.add.range_spec_style,
        );
    }
    Ok(resolved)
}
/// [`registry_save_specifier`] for a registry selector that names `spec`.
pub(super) async fn explicit_registry_save_specifier(
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
    pub(super) specifier: String,
    pub(super) picked: Option<PickedPackage>,
}
impl From<String> for SaveSpecifier {
    fn from(specifier: String) -> Self {
        Self { specifier, picked: None }
    }
}
/// The `catalog:` reference for a dependency the catalog already lists, or
/// `None` when it lists no entry for it.
fn cataloged_specifier(package_name: &str, inputs: &AddResolveInputs<'_, '_>) -> Option<String> {
    let catalog_name = listing_catalog_name(
        package_name,
        inputs.catalogs,
        inputs.owned.save_catalog_name.as_deref(),
    )?;
    Some(if catalog_name == pnpm_catalogs_types::DEFAULT_CATALOG_NAME {
        "catalog:".to_string()
    } else {
        format!("catalog:{catalog_name}")
    })
}

/// The catalog `pnpm add` saves to, when it lists `package_name`.
pub(super) fn listing_catalog_name<'a>(
    package_name: &str,
    catalogs: &Catalogs,
    save_catalog_name: Option<&'a str>,
) -> Option<&'a str> {
    let catalog_name = crate::per_dep_catalog_name(None, save_catalog_name);
    catalogs.get(catalog_name)?.get(package_name)?;
    Some(catalog_name)
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
