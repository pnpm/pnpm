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
use pacquet_config::{Config, WorkspaceSettings};
use pacquet_env_installer::{ConfigDepsInstallOptions, resolve_and_install_config_deps};
use pacquet_graph_hasher::{detect_node_version, host_arch, host_libc, host_platform};
use pacquet_hooks::{HookContext, LogFn, finder};
use pacquet_network::{NetworkSettings, RetryOpts, ThrottledClient};
use pacquet_reporter::{HookLog, LogEvent, LogLevel, Reporter};
use pacquet_resolving_npm_resolver::{
    InMemoryPackageMetaCache, NpmResolver, shared_packument_fetch_locker,
    shared_picked_manifest_cache,
};
use pacquet_store_dir::StoreDir;
use pacquet_workspace_state::ConfigDependency;
use serde_json::Value;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

/// Resolve + install the project's `configDependencies` (a no-op when
/// none are declared). `root_dir` is the lockfile directory;
/// `frozen_lockfile` forwards `--frozen-lockfile` so config deps refuse
/// to mutate an out-of-date env lockfile.
pub async fn install_config_deps<Reporter: self::Reporter>(
    config: &Config,
    root_dir: &Path,
    frozen_lockfile: bool,
) -> Result<()> {
    let Some(config_dependencies) = config.config_dependencies.as_ref() else {
        return Ok(());
    };
    if config_dependencies.is_empty() {
        return Ok(());
    }
    resolve_and_install::<Reporter>(config, config_dependencies, root_dir, frozen_lockfile).await
}

/// Add a single config dependency: resolve + install it (merged with any
/// already-declared config deps), then write the clean specifier into
/// `pnpm-workspace.yaml`'s `configDependencies` block. Backs
/// `pacquet add --config`. Mirrors pnpm's `resolveConfigDeps`.
pub async fn add_config_dependency<Reporter: self::Reporter>(
    config: &Config,
    root_dir: &Path,
    name: &str,
    specifier: &str,
) -> Result<()> {
    let mut config_dependencies = config.config_dependencies.clone().unwrap_or_default();
    config_dependencies
        .insert(name.to_string(), ConfigDependency::VersionWithIntegrity(specifier.to_string()));

    resolve_and_install::<Reporter>(config, &config_dependencies, root_dir, false).await?;

    pacquet_workspace_manifest_writer::set_config_dependency(root_dir, name, specifier)
        .into_diagnostic()
        .wrap_err("recording the config dependency in pnpm-workspace.yaml")
}

/// Build the resolver + install options from `config` and resolve +
/// install `config_dependencies`. Shared by [`install_config_deps`] and
/// [`add_config_dependency`].
async fn resolve_and_install<Reporter: self::Reporter>(
    config: &Config,
    config_dependencies: &std::collections::BTreeMap<String, ConfigDependency>,
    root_dir: &Path,
    frozen_lockfile: bool,
) -> Result<()> {
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

    // `DownloadTarballToStore` needs a `&'static StoreDir`; the config
    // (and thus its store dir) is itself leaked for the process
    // lifetime, but we only hold a borrowed `&Config` here so the
    // caller keeps the unique `&mut Config` for the `updateConfig`
    // pass. Leak a clone to recover the `'static` borrow — a single
    // small, one-per-install allocation in a short-lived CLI process.
    let store_dir: &'static StoreDir = Box::leak(Box::new(config.store_dir.clone()));
    let node_version = detect_node_version().unwrap_or_else(|| "0.0.0".to_string());
    let options = ConfigDepsInstallOptions {
        root_dir,
        store_dir,
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

/// Run the `updateConfig` pnpmfile hooks contributed by config-dependency
/// plugins (and the project's own pnpmfile), applying their result to
/// `config`. Mirrors the hook half of pnpm's
/// [`installConfigDepsAndLoadHooks`](https://github.com/pnpm/pnpm/blob/31858c544b/pnpm/src/getConfig.ts#L74-L98):
/// plugin pnpmfiles run before the project pnpmfile (pnpm `unshift`s
/// them), each transforming the config object in turn.
///
/// Config round-trips through [`WorkspaceSettings`], so any settings key
/// a hook changes is applied back the same way `pnpm-workspace.yaml` is.
/// Only the keys a hook actually changed are applied, so values resolved
/// from `.npmrc` / CLI flags that the hooks leave untouched are not
/// clobbered. Settings pacquet models outside `WorkspaceSettings` (e.g.
/// named `catalogs`) are not yet round-tripped.
pub async fn run_update_config_hooks<Reporter: self::Reporter>(
    config: &mut Config,
    root_dir: &Path,
) -> Result<()> {
    let config_modules_dir = root_dir.join("node_modules").join(".pnpm-config");
    let mut pnpmfiles: Vec<PathBuf> = match config.config_dependencies.as_ref() {
        Some(deps) => finder::calc_pnpmfile_paths_of_plugin_deps(
            &config_modules_dir,
            deps.keys().map(String::as_str),
        ),
        None => Vec::new(),
    };
    if let Some(root_pnpmfile) = finder::find_pnpmfile(root_dir) {
        pnpmfiles.push(root_pnpmfile);
    }
    if pnpmfiles.is_empty() {
        return Ok(());
    }

    let (base_dir, settings) = match WorkspaceSettings::find_and_load(root_dir).into_diagnostic()? {
        Some((path, settings)) => {
            (path.parent().map_or_else(|| root_dir.to_path_buf(), Path::to_path_buf), settings)
        }
        None => (root_dir.to_path_buf(), WorkspaceSettings::default()),
    };
    let input = serde_json::to_value(&settings)
        .into_diagnostic()
        .wrap_err("serialize workspace settings for updateConfig hooks")?;

    let prefix = root_dir.to_string_lossy().into_owned();
    let mut current = input.clone();
    for pnpmfile in &pnpmfiles {
        let hooks = finder::load_pnpmfile_at(pnpmfile.clone());
        let ctx = HookContext { log: hook_logger::<Reporter>(pnpmfile, &prefix) };
        current = hooks
            .update_config(current, ctx)
            .await
            .map_err(|err| miette::miette!("{err}"))
            .wrap_err_with(|| {
            format!("running updateConfig hook from {}", pnpmfile.display())
        })?;
    }

    let delta = config_delta(&input, &current);
    if delta.as_object().is_none_or(serde_json::Map::is_empty) {
        return Ok(());
    }
    let delta_settings: WorkspaceSettings = serde_json::from_value(delta)
        .into_diagnostic()
        .wrap_err("deserialize the updateConfig hook result")?;
    delta_settings.apply_to(config, &base_dir);
    Ok(())
}

/// The keys whose value the hooks changed between the serialized input
/// config and the hooks' output. Applying only these avoids clobbering
/// config resolved elsewhere (`.npmrc`, CLI flags) that a hook left
/// untouched.
fn config_delta(input: &Value, output: &Value) -> Value {
    let (Some(input_obj), Some(output_obj)) = (input.as_object(), output.as_object()) else {
        return output.clone();
    };
    let mut delta = serde_json::Map::new();
    for (key, value) in output_obj {
        if input_obj.get(key) != Some(value) {
            delta.insert(key.clone(), value.clone());
        }
    }
    Value::Object(delta)
}

/// A `context.log(...)` sink that forwards each hook log line to the
/// `pnpm:hook` channel, tagged with the pnpmfile it came from.
fn hook_logger<Reporter: self::Reporter>(pnpmfile: &Path, prefix: &str) -> LogFn {
    let from = pnpmfile.to_string_lossy().into_owned();
    let prefix = prefix.to_owned();
    Arc::new(move |message| {
        Reporter::emit(&LogEvent::Hook(HookLog {
            level: LogLevel::Debug,
            from: from.clone(),
            hook: "updateConfig".to_string(),
            prefix: prefix.clone(),
            message,
        }));
    })
}
