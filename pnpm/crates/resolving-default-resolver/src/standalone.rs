//! The resolver chain for a single resolve, outside an install.
//!
//! The install path assembles its chain in
//! `pnpm_package_manager::install_with_fresh_lockfile` around resources
//! only an install has: the pnpmfile's custom resolvers, the resolution
//! caches shared across the walk, the tarball fetch context that lets a
//! tarball resolver read a fetched `package.json`. A caller that wants a
//! single answer for a single specifier — `pnpm store add`, the NAPI
//! `resolveDependency` — has none of that, and would otherwise each grow
//! its own copy of the ten-resolver chain.
//!
//! Two deliberate deviations from the install chain follow from having no
//! install to hang off:
//!
//! - The tarball resolver runs without a fetch context, so an `http(s)`
//!   tarball is claimed with its normalized URL but no bundled manifest or
//!   integrity — both live inside the archive, and only the install pass
//!   extracts it.
//! - Custom (pnpmfile) resolvers are omitted: loading a pnpmfile is an
//!   install-time concern.

use crate::DefaultResolver;
use pnpm_config::Config;
use pnpm_engine_pm_yarn_resolver::YarnResolver;
use pnpm_engine_runtime_bun_resolver::BunResolver;
use pnpm_engine_runtime_deno_resolver::DenoResolver;
use pnpm_engine_runtime_node_resolver::NodeResolver;
use pnpm_network::{RetryOpts, ThrottledClient};
use pnpm_resolving_git_resolver::{GitResolver, RealGitProbe, RealGitRunner};
use pnpm_resolving_local_resolver::{LocalPathResolver, LocalResolverContext, LocalSchemeResolver};
use pnpm_resolving_npm_resolver::{
    MergeNamedRegistriesError, NamedRegistryResolver, NpmResolver, merge_named_registries,
    shared_in_memory_cache, shared_packument_fetch_locker, shared_picked_manifest_cache,
};
use pnpm_resolving_resolver_base::Resolver;
use pnpm_resolving_tarball_resolver::TarballResolver;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

/// Inputs of [`build_standalone_chain`].
pub struct StandaloneChainOptions<'a> {
    pub config: &'a Config,
    pub http_client: &'a Arc<ThrottledClient>,
    /// pnpm's `fullMetadata`: fetch the registry's unabbreviated document
    /// so the resolved version object keeps the fields the abbreviated one
    /// drops.
    pub full_metadata: bool,
    /// pnpm's `filterMetadata`: strip the version-object fields the
    /// install path never reads. Only meaningful under
    /// [`Self::full_metadata`], and wrong for a caller that asked for the
    /// full document because it wants those fields.
    pub filter_metadata: bool,
}

/// Build the chain a single resolve dispatches through.
///
/// Order mirrors the install path: npm → git → tarball → localScheme →
/// node → deno → bun → yarn → namedRegistry → localPath. The local-resolver
/// split (scheme before the runtimes, path last) lets a
/// `<alias>:@scope/pkg` named-registry specifier reach the named-registry
/// resolver instead of being claimed by the path-shape detector on the
/// strength of its embedded `/`.
pub fn build_standalone_chain(
    opts: &StandaloneChainOptions<'_>,
) -> Result<DefaultResolver, MergeNamedRegistriesError> {
    let &StandaloneChainOptions { config, http_client, full_metadata, filter_metadata } = opts;
    let retry_opts = RetryOpts {
        retries: config.fetch_retries,
        factor: config.fetch_retry_factor,
        min_timeout: std::time::Duration::from_millis(config.fetch_retry_mintimeout),
        max_timeout: std::time::Duration::from_millis(config.fetch_retry_maxtimeout),
    };

    // Shared behind an `Arc` so the deno / bun runtime resolvers (which
    // reuse the npm resolver for their own version picking) and the chain
    // slot below all point at the same instance and its metadata cache.
    let npm_resolver: Arc<dyn Resolver> = Arc::new(NpmResolver {
        // `resolved_registries` inserts the `default` route from
        // `config.registry`; `config.registries` alone omits it, which
        // would leave the picker with a host-less `/pkg` URL.
        registries: config.resolved_registries().into_iter().collect(),
        registries_by_prefix: config.registries_by_prefix.clone().into_iter().collect(),
        http_client: Arc::clone(http_client),
        auth_headers: Arc::clone(&config.auth_headers),
        meta_cache: shared_in_memory_cache(),
        fetch_locker: shared_packument_fetch_locker(),
        picked_manifest_cache: shared_picked_manifest_cache(),
        cache_dir: Some(config.cache_dir.clone()),
        offline: config.offline,
        prefer_offline: config.prefer_offline,
        ignore_missing_time_field: config.minimum_release_age_ignore_missing_time,
        full_metadata,
        needs_full_metadata_for: None,
        filter_metadata,
        retry_opts,
    });

    let git_resolver = GitResolver::new(
        Arc::new(RealGitProbe::new(Arc::clone(http_client))),
        Arc::new(RealGitRunner::new()),
    );
    let tarball_resolver =
        TarballResolver { http_client: Arc::clone(http_client), fetch_context: None };

    // `preserveAbsolutePaths` isn't exposed by pacquet's `Config` yet, so
    // the local-resolver context defaults to `false` here — same as the
    // install path.
    let local_ctx = LocalResolverContext { preserve_absolute_paths: false };
    let local_scheme_resolver = LocalSchemeResolver::new(local_ctx);
    let local_path_resolver = LocalPathResolver::new(local_ctx);

    let mut node_resolver = NodeResolver::new(Arc::clone(http_client));
    node_resolver.node_download_mirrors.clone_from(&config.node_download_mirrors);
    node_resolver.offline = config.offline;
    node_resolver.cache_dir = Some(config.cache_dir.clone());
    let deno_resolver = DenoResolver::new(Arc::clone(http_client), Arc::clone(&npm_resolver));
    let bun_resolver = BunResolver::new(Arc::clone(http_client), Arc::clone(&npm_resolver));
    let yarn_resolver = YarnResolver::new(Arc::clone(http_client));

    // User-supplied named-registry aliases from
    // `pnpm-workspace.yaml#namedRegistries`, merged with pacquet's
    // built-ins (today: `gh:` → GitHub Packages). A malformed URL here
    // fails fast with `ERR_PNPM_INVALID_NAMED_REGISTRY_URL`, matching the
    // install path.
    let user_registries_by_prefix: HashMap<String, String> =
        config.registries_by_prefix.iter().map(|(name, url)| (name.clone(), url.clone())).collect();
    let merged_registries_by_prefix = merge_named_registries(&user_registries_by_prefix)?;
    let named_registry_aliases: HashSet<String> =
        merged_registries_by_prefix.keys().cloned().collect();
    let named_registry_resolver = NamedRegistryResolver {
        registries_by_prefix: merged_registries_by_prefix,
        registry_names: named_registry_aliases,
        http_client: Arc::clone(http_client),
        auth_headers: Arc::clone(&config.auth_headers),
        meta_cache: shared_in_memory_cache(),
        fetch_locker: shared_packument_fetch_locker(),
        picked_manifest_cache: shared_picked_manifest_cache(),
        cache_dir: Some(config.cache_dir.clone()),
        offline: config.offline,
        prefer_offline: config.prefer_offline,
        ignore_missing_time_field: config.minimum_release_age_ignore_missing_time,
        full_metadata,
        needs_full_metadata_for: None,
        filter_metadata,
        retry_opts,
    };

    Ok(DefaultResolver::new(vec![
        Box::new(Arc::clone(&npm_resolver)),
        Box::new(git_resolver),
        Box::new(tarball_resolver),
        Box::new(local_scheme_resolver),
        Box::new(node_resolver),
        Box::new(deno_resolver),
        Box::new(bun_resolver),
        Box::new(yarn_resolver),
        Box::new(named_registry_resolver),
        Box::new(local_path_resolver),
    ]))
}
