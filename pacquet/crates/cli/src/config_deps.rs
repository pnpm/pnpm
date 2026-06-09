//! Resolve and install configurational dependencies before the main
//! install runs.
//!
//! Mirrors the install half of pnpm's
//! [`installConfigDepsAndLoadHooks`](https://github.com/pnpm/pnpm/blob/31858c544b/pnpm/src/getConfig.ts#L45-L99):
//! config dependencies are materialized at config-finalization time, so
//! the env lockfile (the first YAML document of `pnpm-lock.yaml`) is
//! written before the regular install reads or rewrites the wanted
//! lockfile. Plugin-hook loading (the `updateConfig` half) is wired in
//! separately.

use miette::{IntoDiagnostic, Result, WrapErr};
use pacquet_config::Config;
use pacquet_env_installer::{ConfigDepsInstallOptions, resolve_and_install_config_deps};
use pacquet_graph_hasher::{detect_node_version, host_arch, host_libc, host_platform};
use pacquet_network::{NetworkSettings, RetryOpts, ThrottledClient};
use pacquet_reporter::Reporter;
use pacquet_resolving_npm_resolver::{
    InMemoryPackageMetaCache, NpmResolver, shared_packument_fetch_locker,
    shared_picked_manifest_cache,
};
use std::{collections::HashMap, path::Path, sync::Arc, time::Duration};

/// Resolve + install the project's `configDependencies` (a no-op when
/// none are declared). `root_dir` is the lockfile directory;
/// `frozen_lockfile` forwards `--frozen-lockfile` so config deps refuse
/// to mutate an out-of-date env lockfile.
pub async fn install_config_deps<Reporter: self::Reporter>(
    config: &'static Config,
    root_dir: &Path,
    frozen_lockfile: bool,
) -> Result<()> {
    let Some(config_dependencies) = config.config_dependencies.as_ref() else {
        return Ok(());
    };
    if config_dependencies.is_empty() {
        return Ok(());
    }

    let http_client = Arc::new(
        ThrottledClient::for_installs(
            &config.proxy,
            &config.tls,
            &config.tls_by_uri,
            &NetworkSettings {
                network_concurrency: config.network_concurrency,
                fetch_timeout: Duration::from_millis(config.fetch_timeout),
                user_agent: config.user_agent.clone(),
            },
        )
        .into_diagnostic()
        .wrap_err("create the network client for configurational dependencies")?,
    );

    let mut registries = HashMap::new();
    registries.insert("default".to_string(), config.registry.clone());

    let retry_opts = RetryOpts {
        retries: config.fetch_retries,
        factor: config.fetch_retry_factor,
        min_timeout: Duration::from_millis(config.fetch_retry_mintimeout),
        max_timeout: Duration::from_millis(config.fetch_retry_maxtimeout),
    };

    let resolver = NpmResolver {
        registries: registries.clone(),
        named_registries: HashMap::new(),
        http_client: Arc::clone(&http_client),
        auth_headers: Arc::clone(&config.auth_headers),
        meta_cache: Arc::new(InMemoryPackageMetaCache::default()),
        fetch_locker: shared_packument_fetch_locker(),
        picked_manifest_cache: shared_picked_manifest_cache(),
        cache_dir: Some(config.cache_dir.clone()),
        offline: config.offline,
        prefer_offline: config.prefer_offline,
        ignore_missing_time_field: config.minimum_release_age_ignore_missing_time,
        full_metadata: false,
        retry_opts,
    };

    let node_version = detect_node_version().unwrap_or_else(|| "0.0.0".to_string());
    let options = ConfigDepsInstallOptions {
        root_dir,
        store_dir: &config.store_dir,
        http_client: &http_client,
        auth_headers: config.auth_headers.as_ref(),
        registries: &registries,
        verify_store_integrity: config.verify_store_integrity,
        offline: config.offline,
        package_import_method: config.package_import_method,
        retry_opts,
        frozen_lockfile,
        supported_architectures: None,
        current_node_version: &node_version,
        current_os: host_platform(),
        current_cpu: host_arch(),
        current_libc: host_libc(),
    };

    resolve_and_install_config_deps::<Reporter>(config_dependencies, &resolver, &options)
        .await
        .map_err(miette::Report::new)
        .wrap_err("install configurational dependencies")
}
