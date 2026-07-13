//! Resolve and install configurational dependencies before the main
//! install runs.
//!
//! Config dependencies are materialized at config-finalization time, so
//! the env lockfile (the first YAML document of `pnpm-lock.yaml`) is
//! written before the regular install reads or rewrites the wanted
//! lockfile. Plugin-hook loading (the `updateConfig` half) is wired in
//! separately.

use crate::config_overrides::apply_store_dir_override;
use miette::{IntoDiagnostic, Result, WrapErr};
use pacquet_catalogs_config::get_catalogs_from_workspace_manifest;
use pacquet_config::{Config, Host, WorkspaceSettings};
use pacquet_env_installer::{
    ConfigDepsInstallOptions, resolve_and_install_config_deps, resolve_package_manager_integrities,
};
use pacquet_graph_hasher::{detect_node_version, host_arch, host_libc, host_platform};
use pacquet_hooks::{HookContext, LogFn, PnpmfileHooks, finder};
use pacquet_network::{NetworkSettings, RetryOpts, ThrottledClient};
use pacquet_reporter::{HookLog, LogEvent, LogLevel, Reporter};
use pacquet_resolving_npm_resolver::{
    InMemoryPackageMetaCache, NpmResolver, shared_packument_fetch_locker,
    shared_picked_manifest_cache,
};
use pacquet_resolving_resolver_base::{ResolveOptions, Resolver, WantedDependency};
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

/// Resolve the package-manager engine dependencies into the env lockfile's
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

/// The version `pnpm self-update` resolved a specifier to, plus whether
/// the pick violated the active maturity/trust policy.
#[derive(Debug)]
pub struct ResolvedPnpm {
    pub version: String,
    /// `true` when the resolver picked a version despite the maturity
    /// (`minimumReleaseAge`) or `trustPolicy` gate. Self-update fails
    /// closed on this under strict resolution.
    pub policy_violation: bool,
}

/// Resolve `pnpm@<bare_specifier>` against the trusted package-manager
/// bootstrap registry (never the repository-controlled project
/// registries), applying the same `minimumReleaseAge` and `trustPolicy`
/// gates the install path uses. Returns `None` when the specifier cannot
/// be resolved. Backs `pacquet self-update`'s "check for updates" probe.
///
/// The metadata mode follows [`Config::requires_full_metadata_for_resolution`]
/// (via [`EnvInstallerContext`]), so under `trustPolicy=no-downgrade` or
/// `resolutionMode=time-based` the probe fetches the full packument the
/// trust and maturity checks need — the same resolver behaviour as a
/// regular install, rather than a self-update-specific abbreviated-metadata
/// path that would fail closed with "missing time".
pub async fn resolve_pnpm_version(
    config: &Config,
    bare_specifier: &str,
) -> Result<Option<ResolvedPnpm>> {
    let context = EnvInstallerContext::for_package_manager(config)?;

    // `minimumReleaseAge` cutoff, computed the same way as the install
    // path's `PickPolicy::from_config`. When the age is configured, a
    // failure to compute the cutoff fails closed rather than silently
    // disabling the maturity gate — self-update is security-sensitive.
    let published_by = match config.resolved_minimum_release_age() {
        Some(minutes) => {
            let minutes = i64::try_from(minutes)
                .into_diagnostic()
                .wrap_err("convert minimumReleaseAge to minutes")?;
            let duration = chrono::Duration::try_minutes(minutes)
                .ok_or_else(|| miette::miette!("minimumReleaseAge is too large"))?;
            Some(
                chrono::Utc::now()
                    .checked_sub_signed(duration)
                    .ok_or_else(|| miette::miette!("minimumReleaseAge cutoff is out of range"))?,
            )
        }
        None => None,
    };
    let published_by_exclude = config
        .minimum_release_age_exclude
        .as_deref()
        .filter(|patterns| !patterns.is_empty())
        .map(pacquet_config::version_policy::create_package_version_policy)
        .transpose()
        .into_diagnostic()
        .wrap_err("compile the minimum-release-age-exclude policy")?;
    let trust_policy = match config.trust_policy {
        pacquet_config::TrustPolicy::Off => None,
        pacquet_config::TrustPolicy::NoDowngrade => Some(pacquet_config::TrustPolicy::NoDowngrade),
    };
    let trust_policy_exclude = config
        .trust_policy_exclude
        .as_deref()
        .filter(|patterns| !patterns.is_empty())
        .map(pacquet_config::version_policy::create_package_version_policy)
        .transpose()
        .into_diagnostic()
        .wrap_err("compile the trust-policy-exclude policy")?;

    let wanted = WantedDependency {
        alias: Some("pnpm".to_string()),
        bare_specifier: Some(bare_specifier.to_string()),
        ..WantedDependency::default()
    };
    let opts = ResolveOptions {
        default_tag: Some("latest".to_string()),
        published_by,
        published_by_exclude,
        trust_policy,
        trust_policy_exclude,
        trust_policy_ignore_after: config.trust_policy_ignore_after,
        ..ResolveOptions::default()
    };
    let result = context
        .resolver
        .resolve(&wanted, &opts)
        .await
        .map_err(|error| miette::miette!("{error}"))
        .wrap_err_with(|| format!("resolve pnpm@{bare_specifier}"))?;
    let Some(result) = result else {
        return Ok(None);
    };
    let Some(name_ver) = result.name_ver else {
        return Ok(None);
    };
    // Fail closed if the specifier resolved to something other than `pnpm`
    // (e.g. an `npm:other-pkg@x` alias): otherwise the maturity/trust
    // policy decision would be made against the wrong package's metadata
    // while self-update still installs `pnpm@<version>`.
    if name_ver.name.to_string() != "pnpm" {
        return Ok(None);
    }
    Ok(Some(ResolvedPnpm {
        version: name_ver.suffix.to_string(),
        policy_violation: result.policy_violation.is_some(),
    }))
}

/// Add a single config dependency: resolve + install it (merged with any
/// already-declared config deps), then write the clean specifier into
/// `pnpm-workspace.yaml`'s `configDependencies` block. Backs
/// `pacquet add --config`.
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
            .wrap_err("create the network client for env-installer dependencies")?
            .with_max_sockets_per_host(config.max_sockets),
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
            // Derive the metadata mode from config exactly as the install
            // resolver does (via `PickPolicy`), so resolving the pnpm engine
            // (or a config dependency) under `resolutionMode=time-based` /
            // `trustPolicy=no-downgrade` fetches the full packument the
            // `minimumReleaseAge` and trust checks need — instead of failing
            // closed on abbreviated metadata that omits `time`.
            full_metadata: config.requires_full_metadata_for_resolution(),
            filter_metadata: config.requires_full_metadata_for_resolution(),
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
/// `config`. Plugin pnpmfiles run before the project pnpmfile, each
/// transforming the config object in turn.
///
/// Config round-trips through [`WorkspaceSettings`], so any settings key
/// a hook changes is applied back the same way `pnpm-workspace.yaml` is.
/// Only the keys a hook actually changed are applied, so values resolved
/// from `.npmrc` / CLI flags that the hooks leave untouched are not
/// clobbered. The `catalog:`/`catalogs:` blocks — which pacquet models
/// outside `WorkspaceSettings` — are seeded into the hook input and, when
/// a hook changes them, captured into [`Config::catalogs`] for the install
/// to use.
/// The pnpmfile paths that contribute hooks for `root_dir`, in
/// application order: config-dependency plugin pnpmfiles (lexical
/// order) first, then the workspace-root `.pnpmfile.{cjs,mjs}`. Shared
/// by the `updateConfig` install hook and the `beforePacking`
/// pack/publish hook so both apply the same pnpmfile set, matching
/// pnpm's single loaded hooks object.
#[must_use]
pub fn resolve_pnpmfile_paths(config: &Config, root_dir: &Path) -> Vec<PathBuf> {
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
    pnpmfiles
}

/// Load the pnpmfiles that contribute a `beforePacking` hook for
/// `root_dir` (see [`resolve_pnpmfile_paths`]), returning one shareable
/// hook handle per pnpmfile. A recursive pack loads them once and clones
/// the `Arc`s into each project so a pnpmfile's Node worker is spawned
/// once, not once per packed project.
#[must_use]
pub fn load_before_packing_hooks(config: &Config, root_dir: &Path) -> Vec<Arc<dyn PnpmfileHooks>> {
    resolve_pnpmfile_paths(config, root_dir).into_iter().map(finder::load_pnpmfile_at).collect()
}

pub async fn run_update_config_hooks<Reporter: self::Reporter>(
    config: &mut Config,
    root_dir: &Path,
) -> Result<()> {
    let pnpmfiles = resolve_pnpmfile_paths(config, root_dir);
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
        if let Some(store_dir) = config.explicit_settings.get("storeDir") {
            object.insert("storeDir".to_string(), store_dir.clone());
        }
        object.insert(
            "catalogs".to_string(),
            serde_json::to_value(&yaml_catalogs).into_diagnostic()?,
        );
    }

    let prefix = root_dir.to_string_lossy().into_owned();
    let mut current = input.clone();
    for pnpmfile in &pnpmfiles {
        let hooks = finder::load_pnpmfile_at(pnpmfile.clone());
        let ctx = HookContext { log: hook_logger::<Reporter>(pnpmfile, &prefix), dir: None };
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
    // case returned early above), so the post-hook catalogs are the
    // authoritative set.
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
    let changed_store_dir = delta.get("storeDir").and_then(Value::as_str).map(str::to_owned);
    let changed_virtual_store_dir = delta.get("virtualStoreDir").cloned();
    let changed_global_virtual_store_dir = delta.get("globalVirtualStoreDir").cloned();
    let virtual_store_dir_cleared = changed_virtual_store_dir.as_ref().is_some_and(Value::is_null);
    let delta_settings: WorkspaceSettings = serde_json::from_value(delta)
        .into_diagnostic()
        .wrap_err("deserialize the updateConfig hook result")?;
    delta_settings.apply_to(config, &base_dir);
    if virtual_store_dir_cleared {
        config.virtual_store_dir = base_dir.join("node_modules/.pnpm");
    }
    for (key, value) in [
        ("virtualStoreDir", changed_virtual_store_dir),
        ("globalVirtualStoreDir", changed_global_virtual_store_dir),
    ] {
        match value {
            Some(Value::Null) => {
                config.explicit_settings.remove(key);
            }
            Some(value) => {
                config.explicit_settings.insert(key.to_string(), value);
            }
            None => {}
        }
    }
    if let Some(store_dir) = changed_store_dir {
        apply_store_dir_override::<Host>(config, Path::new(&store_dir), &base_dir)?;
    } else {
        let virtual_store_dir_explicit = config.explicit_settings.contains_key("virtualStoreDir");
        let global_virtual_store_dir_explicit =
            config.explicit_settings.contains_key("globalVirtualStoreDir");
        config.apply_global_virtual_store_derivation(
            virtual_store_dir_explicit,
            global_virtual_store_dir_explicit,
        );
    }
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

#[cfg(test)]
mod tests;
