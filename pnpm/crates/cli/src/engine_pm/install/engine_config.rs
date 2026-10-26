use super::{super::error::EngineError, package_manager_engine_store_root};
use pnpm_config::Config;
use pnpm_store_dir::StoreDir;
use std::sync::Arc;

pub(super) fn package_manager_engine_config(config: &Config) -> miette::Result<Config> {
    let global_pkg_dir = config.global_pkg_dir.as_ref().ok_or(EngineError::NoGlobalDir)?;
    let store_dir = StoreDir::new(package_manager_engine_store_root(global_pkg_dir));
    let mut engine_config = Config {
        global_pkg_dir: Some(global_pkg_dir.clone()),
        store_dir: store_dir.clone(),
        cache_dir: config.cache_dir.clone(),
        target_workspace_dir: config.target_workspace_dir.clone(),
        global_virtual_store_dir: store_dir.links(),
        enable_global_virtual_store: true,
        package_manager_bootstrap: config.package_manager_bootstrap.clone(),
        registry: config.package_manager_bootstrap.registry.clone(),
        registries_by_scope: config.package_manager_bootstrap.registries.clone(),
        proxy: config.package_manager_bootstrap.proxy.clone(),
        proxy_keys: config.package_manager_bootstrap.proxy_keys.clone(),
        tls: config.package_manager_bootstrap.tls.clone(),
        tls_by_uri: config.package_manager_bootstrap.tls_by_uri.clone(),
        auth_headers: Arc::clone(&config.package_manager_bootstrap.auth_headers),
        offline: config.offline,
        prefer_offline: config.prefer_offline,
        fetch_retries: config.fetch_retries,
        fetch_retry_factor: config.fetch_retry_factor,
        fetch_retry_maxtimeout: config.fetch_retry_maxtimeout,
        fetch_retry_mintimeout: config.fetch_retry_mintimeout,
        fetch_timeout: config.fetch_timeout,
        fetch_warn_timeout_ms: config.fetch_warn_timeout_ms,
        network_concurrency: config.network_concurrency,
        max_sockets: config.max_sockets,
        engine_strict: config.engine_strict,
        minimum_release_age: config.minimum_release_age,
        minimum_release_age_strict: config.minimum_release_age_strict,
        minimum_release_age_exclude: config.minimum_release_age_exclude.clone(),
        minimum_release_age_ignore_missing_time: config.minimum_release_age_ignore_missing_time,
        trust_policy: config.trust_policy,
        trust_policy_exclude: config.trust_policy_exclude.clone(),
        trust_policy_ignore_after: config.trust_policy_ignore_after,
        ..Config::default()
    };
    engine_config.disable_agent_skills();
    Ok(engine_config)
}
