//! The resolver chain for a single resolve, outside an install.
//!
//! The install path assembles its chain in
//! `pnpm_package_manager::install_with_fresh_lockfile` around resources
//! only an install has: the pnpmfile's custom resolvers, the resolution
//! caches shared across the walk, the tarball fetch context that lets a
//! tarball resolver read a fetched `package.json`. A caller that wants a
//! single answer for a single specifier — `pnpm store add`, the NAPI
//! `resolveDependency` — has none of that, and would otherwise each grow
//! its own copy of the resolver chain.
//!
//! Deliberate deviations from the install chain follow from having no
//! install to hang off:
//!
//! - The tarball resolver runs without a fetch context, so an `http(s)`
//!   tarball is claimed with its normalized URL but no bundled manifest or
//!   integrity — both live inside the archive, and only the install pass
//!   extracts it.
//! - Custom (pnpmfile) resolvers are omitted: loading a pnpmfile is an
//!   install-time concern.

pub use pnpm_resolving_local_resolver::LocalTarballResolution;

use crate::DefaultResolver;
use pnpm_config::Config;
#[cfg(feature = "runtime-provisioning")]
use pnpm_config::Tool;
#[cfg(feature = "runtime-provisioning")]
use pnpm_engine_pm_yarn_resolver::YarnResolver;
#[cfg(feature = "runtime-provisioning")]
use pnpm_engine_runtime_bun_resolver::BunResolver;
#[cfg(feature = "runtime-provisioning")]
use pnpm_engine_runtime_deno_resolver::DenoResolver;
#[cfg(feature = "runtime-provisioning")]
use pnpm_engine_runtime_node_resolver::NodeResolver;
use pnpm_network::{RetryOpts, ThrottledClient};
use pnpm_resolving_git_resolver::{GitResolver, RealGitProbe, RealGitRunner};
use pnpm_resolving_local_resolver::{LocalPathResolver, LocalResolverContext, LocalSchemeResolver};
use pnpm_resolving_npm_resolver::{
    InMemoryPackageMetaCache, MergeNamedRegistriesError, NamedRegistryResolver, NpmResolver,
    OfflineStoreView, merge_named_registries, shared_in_memory_cache,
    shared_packument_fetch_locker, shared_picked_manifest_cache,
};
use pnpm_resolving_resolver_base::Resolver;
#[cfg(not(feature = "runtime-provisioning"))]
use pnpm_resolving_resolver_base::{ResolveError, ResolveFuture, ResolveOptions, WantedDependency};
use pnpm_resolving_tarball_resolver::TarballResolver;
#[cfg(not(feature = "runtime-provisioning"))]
use pnpm_tool_protocol::is_tool_specifier;
use std::{collections::HashMap, sync::Arc};

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
    /// Whether `file:` tarball resolution reads the archive on disk or defers
    /// reading to the caller's fetcher.
    pub local_tarball_resolution: LocalTarballResolution,
}

/// Build the chain a single resolve dispatches through.
///
/// Order mirrors the install path. The local-resolver
/// split (scheme before the runtimes, path last) lets a
/// `<alias>:@scope/pkg` named-registry specifier reach the named-registry
/// resolver instead of being claimed by the path-shape detector on the
/// strength of its embedded `/`.
pub fn build_standalone_chain(
    opts: &StandaloneChainOptions<'_>,
) -> Result<DefaultResolver, MergeNamedRegistriesError> {
    let &StandaloneChainOptions { config, http_client, .. } = opts;
    let retry_opts = config.retry_opts();
    // Shared behind an `Arc` so the deno / bun runtime resolvers (which
    // reuse the npm resolver for their own version picking) and the chain
    // slot below all point at the same instance and its metadata cache.
    let npm_resolver: Arc<dyn Resolver> = Arc::new(build_npm_resolver(opts, retry_opts));
    // `preserveAbsolutePaths` isn't exposed by pacquet's `Config` yet, so
    // the local-resolver context defaults to `false` here — same as the
    // install path.
    let local_ctx = LocalResolverContext {
        preserve_absolute_paths: false,
        tarball_resolution: opts.local_tarball_resolution,
        ..Default::default()
    };

    let mut chain: Vec<Box<dyn Resolver>> = vec![
        Box::new(Arc::clone(&npm_resolver)),
        Box::new(GitResolver::new(
            Arc::new(RealGitProbe::new(Arc::clone(http_client))),
            Arc::new(RealGitRunner::new()),
        )),
        Box::new(TarballResolver { http_client: Arc::clone(http_client), fetch_context: None }),
        Box::new(LocalSchemeResolver::new(local_ctx)),
    ];
    #[cfg(feature = "runtime-provisioning")]
    {
        chain.push(Box::new(build_node_resolver(config, http_client)));
        chain.push(Box::new(DenoResolver::new(Arc::clone(http_client), Arc::clone(&npm_resolver))));
        chain.push(Box::new(
            BunResolver::new(Arc::clone(http_client), Arc::clone(&npm_resolver))
                .with_mirror(config.tool_mirror(Tool::Bun)),
        ));
        chain.push(Box::new(YarnResolver::new(
            Arc::clone(http_client),
            config.tls.strict_ssl.unwrap_or(true),
        )));
    }
    #[cfg(not(feature = "runtime-provisioning"))]
    chain.push(Box::new(UnavailableToolResolver));
    chain.push(Box::new(build_named_registry_resolver(opts, retry_opts)?));
    chain.push(Box::new(LocalPathResolver::new(local_ctx)));
    Ok(DefaultResolver::new(chain))
}

#[cfg(not(feature = "runtime-provisioning"))]
struct UnavailableToolResolver;

#[cfg(not(feature = "runtime-provisioning"))]
impl Resolver for UnavailableToolResolver {
    fn resolve<'a>(
        &'a self,
        wanted_dependency: &'a WantedDependency,
        _opts: &'a ResolveOptions,
    ) -> ResolveFuture<'a> {
        Box::pin(async move {
            let Some(alias) = wanted_dependency.alias.as_deref() else { return Ok(None) };
            let Some(specifier) = wanted_dependency.bare_specifier.as_deref() else {
                return Ok(None);
            };
            if !matches!(alias, "node" | "deno" | "bun" | "yarn") || !is_tool_specifier(specifier) {
                return Ok(None);
            }
            Err(Box::new(super::ProvisioningUnavailableError {
                alias: alias.to_string(),
                specifier: specifier.to_string(),
            }) as ResolveError)
        })
    }

    fn resolve_latest<'a>(
        &'a self,
        _query: &'a pnpm_resolving_resolver_base::LatestQuery,
        _opts: &'a ResolveOptions,
    ) -> pnpm_resolving_resolver_base::ResolveLatestFuture<'a> {
        Box::pin(async { Ok(None) })
    }
}

#[cfg(all(test, not(feature = "runtime-provisioning")))]
mod unavailable_tool_tests {
    use super::*;

    #[tokio::test(flavor = "current_thread")]
    async fn rejects_tool_runtime_requests_before_local_path_resolution() {
        let resolver = UnavailableToolResolver;
        let wanted = WantedDependency {
            alias: Some("node".to_string()),
            bare_specifier: Some("runtime:rc/22.0.0".to_string()),
            ..WantedDependency::default()
        };
        let error = resolver
            .resolve(&wanted, &ResolveOptions::default())
            .await
            .expect_err("runtime provisioning is disabled");

        assert_eq!(
            error.to_string(),
            "Provisioning is not enabled in this build for node@runtime:rc/22.0.0",
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn leaves_registry_requests_to_the_named_registry_resolver() {
        let resolver = UnavailableToolResolver;
        let wanted = WantedDependency {
            alias: Some("node".to_string()),
            bare_specifier: Some("^22.0.0".to_string()),
            ..WantedDependency::default()
        };

        assert!(
            resolver
                .resolve(&wanted, &ResolveOptions::default())
                .await
                .expect("registry requests are not tool requests")
                .is_none()
        );
    }
}

fn build_npm_resolver(
    opts: &StandaloneChainOptions<'_>,
    retry_opts: RetryOpts,
) -> NpmResolver<InMemoryPackageMetaCache> {
    let &StandaloneChainOptions { config, http_client, .. } = opts;
    NpmResolver {
        // `resolved_registries` inserts the `default` route from
        // `config.registry`; `config.registries` alone omits it, which
        // would leave the picker with a host-less `/pkg` URL.
        registries: config
            .resolved_registries()
            .into_iter()
            .collect(),
        declared_prefixes: config.declared_registry_prefixes(),
        metadata: pnpm_resolving_npm_resolver::RegistryMetadataClient {
            http_client: Arc::clone(http_client),
            auth_headers: Arc::clone(&config.auth_headers),
            meta_cache: shared_in_memory_cache(),
            fetch_locker: shared_packument_fetch_locker(),
            picked_manifest_cache: shared_picked_manifest_cache(),
            cache_dir: Some(config.cache_dir.clone()),
            retry_opts,
        },
        format: metadata_format(opts),
        cache_policy: pnpm_resolving_npm_resolver::MetadataCachePolicy {
            network: config.network_policy(),
            ignore_missing_time_field: config.minimum_release_age_ignore_missing_time,
        },
        store_view: offline_store_view(config),
    }
}

fn metadata_format(
    opts: &StandaloneChainOptions<'_>,
) -> pnpm_resolving_npm_resolver::RegistryMetadataFormat {
    let config = opts.config;
    pnpm_resolving_npm_resolver::RegistryMetadataFormat {
        full_metadata: opts.full_metadata,
        needs_full_metadata_for: None,
        filter_metadata: opts.filter_metadata,
        registry_options_by_url: Arc::new(config.registry_options_by_url.clone()),
        minimum_release_age: config.resolved_minimum_release_age(),
    }
}

fn offline_store_view(config: &Config) -> Option<OfflineStoreView> {
    OfflineStoreView::open_for_offline(config.offline, &config.store_dir, config.frozen_store)
}

#[cfg(feature = "runtime-provisioning")]
fn build_node_resolver(config: &Config, http_client: &Arc<ThrottledClient>) -> NodeResolver {
    let mut node_resolver =
        NodeResolver::new_with_auth(Arc::clone(http_client), Arc::clone(&config.auth_headers));
    node_resolver.node_download_mirrors.clone_from(&config.node_download_mirrors);
    node_resolver.mirror = config.tool_mirror(Tool::Node).map(ToString::to_string);
    node_resolver.channel_mirrors = config.tool_channel_mirrors(Tool::Node);
    node_resolver.offline = config.offline;
    node_resolver.cache_dir = Some(config.cache_dir.clone());
    node_resolver
}

/// The resolver for user-supplied named-registry aliases from
/// `pnpm-workspace.yaml#namedRegistries`, merged with pacquet's built-ins
/// (today: `gh:` → GitHub Packages). A malformed URL here fails fast with
/// `ERR_PNPM_INVALID_NAMED_REGISTRY_URL`, matching the install path.
fn build_named_registry_resolver(
    opts: &StandaloneChainOptions<'_>,
    retry_opts: RetryOpts,
) -> Result<NamedRegistryResolver<InMemoryPackageMetaCache>, MergeNamedRegistriesError> {
    let &StandaloneChainOptions { config, http_client, .. } = opts;
    let user_registries_by_prefix: HashMap<String, String> = config
        .registries_by_prefix
        .iter()
        .map(|(name, url)| (name.clone(), url.clone()))
        .collect();
    let merged_registries_by_prefix = merge_named_registries(&user_registries_by_prefix)?;
    Ok(NamedRegistryResolver {
        registry_names: merged_registries_by_prefix
            .keys()
            .cloned()
            .collect(),
        registries_by_prefix: merged_registries_by_prefix,
        registries: config
            .resolved_registries()
            .into_iter()
            .collect(),
        declared_prefixes: config.declared_registry_prefixes(),
        metadata: pnpm_resolving_npm_resolver::RegistryMetadataClient {
            http_client: Arc::clone(http_client),
            auth_headers: Arc::clone(&config.auth_headers),
            meta_cache: shared_in_memory_cache(),
            fetch_locker: shared_packument_fetch_locker(),
            picked_manifest_cache: shared_picked_manifest_cache(),
            cache_dir: Some(config.cache_dir.clone()),
            retry_opts,
        },
        format: metadata_format(opts),
        cache_policy: pnpm_resolving_npm_resolver::MetadataCachePolicy {
            network: config.network_policy(),
            ignore_missing_time_field: config.minimum_release_age_ignore_missing_time,
        },
        store_view: offline_store_view(config),
    })
}
