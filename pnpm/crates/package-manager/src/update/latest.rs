use super::{
    UpdateError,
    catalogs::{CatalogCtx, effective_specifier},
};
use crate::{
    package_manifest_prefix,
    resolution_policy::{PickPolicy, create_configured_npm_resolver},
};
use chrono::{DateTime, Utc};
use node_semver::Version;
use pnpm_config::{Config, version_policy::PackageVersionPolicy};
use pnpm_engine_pm_yarn_resolver::YarnResolver;
use pnpm_engine_runtime_bun_resolver::BunResolver;
use pnpm_engine_runtime_deno_resolver::DenoResolver;
use pnpm_engine_runtime_node_resolver::NodeResolver;
use pnpm_lockfile_preferred_versions::get_version_selector_type;
use pnpm_network::ThrottledClient;
use pnpm_package_manifest::PackageManifest;
use pnpm_registry::RangeSpecStyle;
use pnpm_reporter::{LogEvent, LogLevel, PnpmLog, Reporter};
use pnpm_resolving_default_resolver::DefaultResolver;
use pnpm_resolving_resolver_base::{
    ResolveOptions, Resolver, UpdateBehavior, VersionSelectorType, WantedDependency,
};
use std::sync::Arc;

/// `--latest` reaches past the declared range by design, which a manifest that
/// keeps its specifiers can't record, so the update stays inside the range and
/// says so once per project.
pub(super) fn emit_latest_ignored<Reporter: self::Reporter>(manifest: &PackageManifest) {
    Reporter::emit(&LogEvent::Pnpm(PnpmLog {
        level: LogLevel::Warn,
        message: r#"Ignoring "--latest": the manifest keeps its version ranges when updating without saving, so dependencies were updated within them instead."#.to_string(),
        prefix: package_manifest_prefix(manifest),
    }));
}
/// The `--latest` inputs that are the same for every direct dependency of a
/// project, gathered so [`latest_specifier`] takes them as one argument.
pub(super) struct LatestRewriteCtx<'a, 'borrow> {
    pub(super) manifest: &'borrow PackageManifest,
    pub(super) config: &'a Config,
    pub(super) http_client_arc: &'borrow Arc<ThrottledClient>,
    pub(super) resolution_observer: Option<&'borrow Arc<dyn crate::ResolutionObserver>>,
    pub(super) range_spec_style: RangeSpecStyle,
    pub(super) lockfile_only: bool,
}
/// The specifier `--latest` should write for `name`, or `None` when no
/// resolver claims the dependency and its manifest entry therefore stands.
///
/// The answer is the resolvers': the chain is asked to resolve the
/// dependency with [`UpdateBehavior::Latest`], and whichever resolver
/// claims it reports back the specifier its own protocol round-trips to —
/// the npm picker takes the higher of the declared range and the `latest`
/// tag, the `runtime:` resolvers re-resolve within the spec the manifest
/// already declares, and the local resolvers echo their spec unchanged.
/// Nothing here needs to know which protocols those are.
pub(super) async fn latest_specifier(
    ctx: &LatestRewriteCtx<'_, '_>,
    chain: &mut Option<LatestResolverChain>,
    catalog_ctx: &mut Option<CatalogCtx>,
    name: &str,
    previous: &str,
) -> Result<Option<String>, UpdateError> {
    let effective = effective_specifier(catalog_ctx, ctx.manifest, ctx.config, previous, name)?;
    // `preserveWorkspaceProtocol` is always on under `update --latest`, so a
    // `workspace:` entry keeps its text whatever version the workspace
    // package is at. Asking the chain would also hand the npm resolver a
    // spec it answers only against the install's workspace-package map,
    // which manifest preparation has not built.
    if effective.starts_with("workspace:") {
        return Ok(None);
    }
    // A dist-tag names no version of its own, so the version behind it moving
    // leaves the declaration saying exactly what was asked for. Rewriting it
    // to that version would drop the instruction to track the tag.
    if get_version_selector_type(&effective) == Some(VersionSelectorType::Tag) {
        return Ok(None);
    }
    let chain = ensure_latest_resolver_chain(chain, ctx)?;
    // The entry being resolved is also the entry whose operator the rewrite
    // keeps, so it is the previous specifier as well.
    let wanted = WantedDependency {
        alias: Some(name.to_string()),
        bare_specifier: Some(effective.clone()),
        prev_specifier: Some(effective.clone()),
        ..WantedDependency::default()
    };
    let manifest_dir =
        ctx.manifest.path().parent().expect("manifest path always has a parent dir").to_path_buf();
    let opts = ResolveOptions {
        project_dir: manifest_dir.clone(),
        lockfile_dir: manifest_dir,
        default_tag: Some("latest".to_string()),
        update: UpdateBehavior::Latest,
        calc_specifier: true,
        range_spec_style: Some(ctx.range_spec_style),
        published_by: chain.published_by,
        published_by_exclude: chain.published_by_exclude.clone(),
        dry_run: ctx.lockfile_only,
        ..ResolveOptions::default()
    };
    let resolved = Resolver::resolve(&chain.resolver, &wanted, &opts)
        .await
        .map_err(|error| UpdateError::ResolveLatest { name: name.to_string(), error })?;
    // A resolver that reports back what the manifest already says has
    // nothing to rewrite. Recording it anyway would mark the manifest dirty
    // and persist it, which for a `runtime:` dependency means rewriting the
    // entry into `devEngines.runtime` — a change the user never asked for.
    Ok(resolved
        .and_then(|result| result.normalized_bare_specifier)
        .filter(|specifier| *specifier != effective))
}
/// The version dist tag `tag` names for `name`, or `None` when no resolver
/// in the chain claims the dependency.
///
/// An explicit `<name>@<tag>` selector asks for the version behind exactly
/// that tag, so — unlike [`latest_specifier`], which reaches for whichever
/// of the declared range and the `latest` tag is higher — the tag is the
/// whole specifier resolved here.
pub(super) async fn tag_version(
    ctx: &LatestRewriteCtx<'_, '_>,
    chain: &mut Option<LatestResolverChain>,
    name: &str,
    tag: &str,
) -> Result<Option<Version>, UpdateError> {
    let chain = ensure_latest_resolver_chain(chain, ctx)?;
    let wanted = WantedDependency {
        alias: Some(name.to_string()),
        bare_specifier: Some(tag.to_string()),
        ..WantedDependency::default()
    };
    let manifest_dir =
        ctx.manifest.path().parent().expect("manifest path always has a parent dir").to_path_buf();
    let opts = ResolveOptions {
        project_dir: manifest_dir.clone(),
        lockfile_dir: manifest_dir,
        default_tag: Some(tag.to_string()),
        published_by: chain.published_by,
        published_by_exclude: chain.published_by_exclude.clone(),
        dry_run: ctx.lockfile_only,
        ..ResolveOptions::default()
    };
    let resolved = Resolver::resolve(&chain.resolver, &wanted, &opts).await.map_err(|error| {
        UpdateError::ResolveTag { name: name.to_string(), tag: tag.to_string(), error }
    })?;
    Ok(resolved.and_then(|result| result.name_ver).map(|name_ver| name_ver.suffix))
}
/// The resolvers that can answer "what is the latest for this dependency",
/// built on first use so an update whose deps are all local opens no
/// client. Deliberately excludes the git, tarball and local-path
/// resolvers: they have no notion of a `latest`, and asking them would
/// clone or download during manifest preparation only to be told the
/// specifier stands.
pub(super) struct LatestResolverChain {
    resolver: DefaultResolver,
    published_by: Option<DateTime<Utc>>,
    published_by_exclude: Option<PackageVersionPolicy>,
}
pub(super) fn ensure_latest_resolver_chain<'chain>(
    chain: &'chain mut Option<LatestResolverChain>,
    ctx: &LatestRewriteCtx<'_, '_>,
) -> Result<&'chain LatestResolverChain, UpdateError> {
    if chain.is_none() {
        let extra_excludes = ctx
            .resolution_observer
            .and_then(|observer| observer.minimum_release_age_exclude_override());
        let policy =
            PickPolicy::from_config_with_extra_excludes(ctx.config, extra_excludes.as_deref())
                .map_err(UpdateError::MinimumReleaseAgeExclude)?;
        let npm_resolver: Arc<dyn Resolver> = Arc::new(
            create_configured_npm_resolver(ctx.config, Arc::clone(ctx.http_client_arc), &policy)
                .map_err(UpdateError::InvalidNamedRegistry)?,
        );
        let mut node_resolver = NodeResolver::new_with_auth(
            Arc::clone(ctx.http_client_arc),
            Arc::clone(&ctx.config.auth_headers),
        );
        node_resolver.node_download_mirrors.clone_from(&ctx.config.node_download_mirrors);
        node_resolver.offline = ctx.config.offline;
        node_resolver.cache_dir = Some(ctx.config.cache_dir.clone());
        let resolver = DefaultResolver::new(vec![
            Box::new(Arc::clone(&npm_resolver)) as Box<dyn Resolver>,
            Box::new(node_resolver),
            Box::new(DenoResolver::new(Arc::clone(ctx.http_client_arc), Arc::clone(&npm_resolver))),
            Box::new(BunResolver::new(Arc::clone(ctx.http_client_arc), Arc::clone(&npm_resolver))),
            Box::new(YarnResolver::new(
                Arc::clone(ctx.http_client_arc),
                ctx.config.tls.strict_ssl.unwrap_or(true),
            )),
        ]);
        *chain = Some(LatestResolverChain {
            resolver,
            published_by: policy.published_by,
            published_by_exclude: policy.published_by_exclude,
        });
    }
    Ok(chain.as_ref().expect("chain initialized above"))
}
/// Whether `bare_specifier` is a `workspace:` spec that points at a local
/// path (e.g. `workspace:../packages/foo/dist`) rather than a version range
/// (`workspace:*`, `workspace:^1.0.0`). Such specs are preserved verbatim on
/// `--latest` instead of being resolved against the registry, since the path
/// may target a publish directory that a normalized range would drop.
///
/// These are kept out of the registry-resolution path via
/// `preserveWorkspaceProtocol`, which is always on under `update --latest`
/// (the override that derives it from `linkWorkspacePackages` only runs under
/// `--workspace`, and `--workspace` cannot be combined with `--latest`).
pub(crate) fn is_workspace_local_path_specifier(bare_specifier: &str) -> bool {
    let Some(pref) = bare_specifier.strip_prefix("workspace:") else {
        return false;
    };
    let is_windows_drive = {
        let mut chars = pref.chars();
        chars.next().is_some_and(|first| first.is_ascii_alphabetic()) && chars.next() == Some(':')
    };
    pref.starts_with('.') || pref.starts_with('/') || pref.starts_with("~/") || is_windows_drive
}
