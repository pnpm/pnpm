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
use pacquet_catalogs_config::get_catalogs_from_workspace_manifest;
use pacquet_config::{Config, WorkspaceSettings};
use pacquet_env_installer::{
    ConfigDepsInstallOptions, resolve_and_install_config_deps, resolve_package_manager_integrities,
};
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

/// Resolve `pnpm` / `@pnpm/exe` into the env lockfile's
/// `packageManagerDependencies` block before the wanted lockfile is
/// loaded.
pub async fn sync_package_manager_dependencies(
    config: &Config,
    root_dir: &Path,
    wanted_specifier: &str,
    pnpm_version: &str,
    frozen_lockfile: bool,
) -> Result<()> {
    let context = EnvInstallerContext::for_package_manager(config)?;
    let options = context.options(root_dir, frozen_lockfile);
    resolve_package_manager_integrities(wanted_specifier, pnpm_version, &context.resolver, &options)
        .await
        .map_err(miette::Report::new)
        .wrap_err("resolve package manager dependencies")
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
    let context = EnvInstallerContext::new(config)?;
    let options = context.options(root_dir, frozen_lockfile);

    resolve_and_install_config_deps::<Reporter>(config_dependencies, &context.resolver, &options)
        .await
        .map_err(miette::Report::new)
        .wrap_err("install configurational dependencies")
}

struct EnvInstallerContext {
    http_client: Arc<ThrottledClient>,
    auth_headers: Arc<pacquet_network::AuthHeaders>,
    registries: HashMap<String, String>,
    retry_opts: RetryOpts,
    store_dir: &'static StoreDir,
    node_version: String,
    verify_store_integrity: bool,
    offline: bool,
    package_import_method: pacquet_config::PackageImportMethod,
    resolver: NpmResolver<InMemoryPackageMetaCache>,
}

impl EnvInstallerContext {
    /// Context for resolving the project's `configDependencies`, using the
    /// project's configured registries and network settings.
    fn new(config: &Config) -> Result<Self> {
        Self::build(
            config,
            &config.proxy,
            &config.tls,
            &config.tls_by_uri,
            config.resolved_registries(),
            Arc::clone(&config.auth_headers),
        )
    }

    /// Context for resolving the package manager pnpm auto-switches to
    /// (`pnpm` / `@pnpm/exe`), routed through the trusted
    /// [`PackageManagerBootstrap`](pacquet_config::PackageManagerBootstrap)
    /// config instead of the repository-controlled project registries.
    fn for_package_manager(config: &Config) -> Result<Self> {
        let bootstrap = &config.package_manager_bootstrap;
        Self::build(
            config,
            &bootstrap.proxy,
            &bootstrap.tls,
            &bootstrap.tls_by_uri,
            bootstrap.resolved_registries(),
            Arc::clone(&bootstrap.auth_headers),
        )
    }

    fn build(
        config: &Config,
        proxy: &pacquet_network::ProxyConfig,
        tls: &pacquet_network::TlsConfig,
        tls_by_uri: &pacquet_network::PerRegistryTls,
        registries: std::collections::BTreeMap<String, String>,
        auth_headers: Arc<pacquet_network::AuthHeaders>,
    ) -> Result<Self> {
        let http_client = Arc::new(
            ThrottledClient::for_installs(
                proxy,
                tls,
                tls_by_uri,
                &NetworkSettings {
                    network_concurrency: config.network_concurrency,
                    fetch_timeout: Duration::from_millis(config.fetch_timeout),
                    user_agent: config.user_agent.clone(),
                },
            )
            .into_diagnostic()
            .wrap_err("create the network client for env-installer dependencies")?,
        );

        let registries: HashMap<String, String> = registries.into_iter().collect();
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
            auth_headers: Arc::clone(&auth_headers),
            meta_cache: Arc::new(InMemoryPackageMetaCache::default()),
            fetch_locker: shared_packument_fetch_locker(),
            picked_manifest_cache: shared_picked_manifest_cache(),
            cache_dir: Some(config.cache_dir.clone()),
            offline: config.offline,
            prefer_offline: config.prefer_offline,
            ignore_missing_time_field: config.minimum_release_age_ignore_missing_time,
            full_metadata: false,
            filter_metadata: false,
            retry_opts,
        };

        Ok(Self {
            http_client,
            auth_headers,
            registries,
            retry_opts,
            store_dir: Box::leak(Box::new(config.store_dir.clone())),
            node_version: detect_node_version().unwrap_or_else(|| "0.0.0".to_string()),
            verify_store_integrity: config.verify_store_integrity,
            offline: config.offline,
            package_import_method: config.package_import_method,
            resolver,
        })
    }

    fn options<'a>(
        &'a self,
        root_dir: &'a Path,
        frozen_lockfile: bool,
    ) -> ConfigDepsInstallOptions<'a> {
        ConfigDepsInstallOptions {
            root_dir,
            store_dir: self.store_dir,
            http_client: &self.http_client,
            auth_headers: &self.auth_headers,
            registries: &self.registries,
            verify_store_integrity: self.verify_store_integrity,
            offline: self.offline,
            package_import_method: self.package_import_method,
            retry_opts: self.retry_opts,
            frozen_lockfile,
            supported_architectures: None,
            current_node_version: &self.node_version,
            current_os: host_platform(),
            current_cpu: host_arch(),
            current_libc: host_libc(),
        }
    }
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
/// clobbered. The `catalog:`/`catalogs:` blocks — which pacquet models
/// outside `WorkspaceSettings` — are seeded into the hook input and, when
/// a hook changes them, captured into [`Config::catalogs`] for the install
/// to use.
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
    let mut input = serde_json::to_value(&settings)
        .into_diagnostic()
        .wrap_err("serialize workspace settings for updateConfig hooks")?;
    // Seed the hook input with the catalogs read from the workspace
    // manifest (`catalog:` + `catalogs:`), which `WorkspaceSettings`
    // doesn't carry, so a hook can read and extend them.
    let workspace_manifest =
        pacquet_workspace::read_workspace_manifest(root_dir).into_diagnostic()?;
    let yaml_catalogs = get_catalogs_from_workspace_manifest(workspace_manifest.as_ref())
        .into_diagnostic()
        .wrap_err("reading catalogs for updateConfig hooks")?;
    if let Some(object) = input.as_object_mut() {
        object.insert(
            "catalogs".to_string(),
            serde_json::to_value(&yaml_catalogs).into_diagnostic()?,
        );
    }

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

    // Adopt the hook output's catalogs wholesale into `Config::catalogs`
    // (the install prefers it over re-reading the manifest). Because the
    // input was seeded with the manifest's catalogs, the output is the
    // authoritative post-`updateConfig` set: a hook that *added*,
    // *replaced*, or *removed* an entry is all reflected — a removed key
    // (absent from the output) maps to an empty set rather than silently
    // falling back to the manifest. At least one pnpmfile ran (the empty
    // case returned early above), so this mirrors pnpm using the
    // post-hook `config.catalogs`.
    config.catalogs = Some(
        current
            .get("catalogs")
            .cloned()
            .map(serde_json::from_value)
            .transpose()
            .into_diagnostic()
            .wrap_err("the updateConfig hook produced an invalid catalogs value")?
            .unwrap_or_default(),
    );

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
