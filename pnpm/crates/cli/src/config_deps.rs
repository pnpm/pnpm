//! Resolve and install configurational dependencies before the main
//! install runs.
//!
//! Config dependencies are materialized at config-finalization time, so
//! the env lockfile (the first YAML document of `pnpm-lock.yaml`) is
//! written before the regular install reads or rewrites the wanted
//! lockfile. Plugin-hook loading (the `updateConfig` half) is wired in
//! separately.

pub use hooks::{load_before_packing_hooks, prepare_config, run_update_config_hooks};

use crate::config_overrides::apply_store_dir_override;

use miette::{IntoDiagnostic, Result, WrapErr};
use pnpm_catalogs_config::get_catalogs_from_workspace_manifest;
use pnpm_config::{
    Config, Host, PNPM_VERSION, WorkspaceSettings, default_state_dir,
    known_settings::is_known_setting_key, resolve_configured_state_dir,
};
use pnpm_env_installer::{
    ConfigDepsInstallOptions, pnpm_engine_packages, resolve_and_install_config_deps,
    resolve_package_manager_integrities,
};
use pnpm_graph_hasher::{detect_node_version, host_arch, host_libc, host_platform};
use pnpm_hooks::{HookContext, LogFn, PnpmfileHooks, finder};
use pnpm_lockfile::EnvLockfile;
use pnpm_network::{RetryOpts, ThrottledClient};
use pnpm_reporter::{HookLog, LogEvent, LogLevel, PnpmLog, Reporter};
use pnpm_resolving_npm_resolver::{
    InMemoryPackageMetaCache, NpmResolver, shared_packument_fetch_locker,
    shared_picked_manifest_cache,
};
use pnpm_resolving_resolver_base::{ResolveOptions, Resolver, WantedDependency};
use pnpm_store_dir::StoreDir;
use pnpm_workspace_state::ConfigDependency;
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    sync::Arc,
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

/// Install the project's `configDependencies` and run their `updateConfig`
/// hooks — the pair every install-family pipeline opens with.
///
/// Both happen before the pipeline builds its state: the env lockfile must
/// land at the top of `pnpm-lock.yaml` before the wanted lockfile is read,
/// and `updateConfig` must mutate `config` before the install reads it.
///
/// The package-manager pin is recorded earlier, by the pre-command checks,
/// for every command rather than only for this family.
pub async fn prepare<Reporter: self::Reporter>(
    config: &mut Config,
    root_dir: &Path,
    frozen_lockfile: bool,
) -> Result<()> {
    install_config_deps::<Reporter>(config, root_dir, frozen_lockfile).await?;
    run_update_config_hooks::<Reporter>(config, root_dir).await?;
    Ok(())
}

/// Resolve pnpm's own engine dependencies into the env lockfile's
/// `packageManagerDependencies` block before the wanted lockfile is
/// loaded. `force_resync` discards recorded entries and re-resolves them
/// even when they look up to date.
pub async fn sync_package_manager_dependencies(
    config: &Config,
    root_dir: &Path,
    wanted_specifier: &str,
    pnpm_version: &str,
    frozen_lockfile: bool,
    force_resync: bool,
) -> Result<EnvLockfile> {
    sync_engine_dependencies(
        config,
        root_dir,
        pnpm_engine_packages(pnpm_version),
        wanted_specifier,
        pnpm_version,
        frozen_lockfile,
        force_resync,
    )
    .await
}

/// Resolve the packages a package manager is installed from into the env
/// lockfile at `root_dir`, so its bytes are pinned by integrity before any
/// of them are downloaded or executed. Returns that env lockfile, which the
/// engine installer reads the closure from.
pub async fn sync_engine_dependencies(
    config: &Config,
    root_dir: &Path,
    packages: &[&str],
    wanted_specifier: &str,
    version: &str,
    frozen_lockfile: bool,
    force_resync: bool,
) -> Result<EnvLockfile> {
    let context = EnvInstallerContext::for_package_manager(config)?;
    let options = context.options(root_dir, frozen_lockfile);
    resolve_package_manager_integrities(
        packages,
        wanted_specifier,
        version,
        &context.resolver,
        &options,
        force_resync,
    )
    .await
    .map_err(miette::Report::new)
    .wrap_err("resolve package manager dependencies")
}

/// The version a package-manager specifier resolved to, plus whether the
/// pick violated the active maturity/trust policy.
#[derive(Debug)]
pub struct ResolvedEngine {
    pub version: String,
    /// The resolved package's manifest, when the resolver returned one.
    /// `pnpm shim` reads the `bin` field from it.
    pub manifest: Option<Arc<Value>>,
    /// Set when the resolver picked a version despite the maturity
    /// (`minimumReleaseAge`) or `trustPolicy` gate. Self-update fails
    /// closed on this under strict resolution; the code tells the two
    /// gates apart, and the reason is the user-facing explanation.
    pub policy_violation: Option<EnginePolicyViolation>,
}

/// Why the resolver's pick violates a policy.
#[derive(Debug)]
pub struct EnginePolicyViolation {
    pub code: &'static str,
    pub reason: String,
}

/// Resolve `<package>@<bare_specifier>` against the trusted
/// package-manager bootstrap registry (never the repository-controlled
/// project registries), applying the same `minimumReleaseAge` and
/// `trustPolicy` gates the install path uses. Returns `None` when the
/// specifier cannot be resolved. Backs `pacquet self-update`'s "check for
/// updates" probe and every package-manager provisioning path.
///
/// The metadata mode follows [`Config::requires_full_metadata_for_resolution`]
/// (via [`EnvInstallerContext`]), so under `trustPolicy=no-downgrade` or
/// `resolutionMode=time-based` the probe fetches the full packument the
/// trust and maturity checks need — the same resolver behaviour as a
/// regular install, rather than a self-update-specific abbreviated-metadata
/// path that would fail closed with "missing time".
pub async fn resolve_engine_version(
    config: &Config,
    package: &str,
    bare_specifier: &str,
) -> Result<Option<ResolvedEngine>> {
    let context = EnvInstallerContext::for_package_manager(config)?;

    let wanted = WantedDependency {
        alias: Some(package.to_string()),
        bare_specifier: Some(bare_specifier.to_string()),
        ..WantedDependency::default()
    };
    let opts = engine_resolve_options(config)?;
    let result = context
        .resolver
        .resolve(&wanted, &opts)
        .await
        .map_err(|error| miette::miette!("{error}"))
        .wrap_err_with(|| format!("resolve {package}@{bare_specifier}"))?;
    let Some(result) = result else {
        return Ok(None);
    };
    let Some(name_ver) = result.name_ver else {
        return Ok(None);
    };
    // Fail closed if the specifier resolved to a different package (e.g. an
    // `npm:other-pkg@x` alias): otherwise the maturity/trust policy decision
    // would be made against the wrong package's metadata while the caller
    // still installs `<package>@<version>`.
    if name_ver.name.to_string() != package {
        return Ok(None);
    }
    Ok(Some(ResolvedEngine {
        version: name_ver.suffix.to_string(),
        manifest: result.manifest.clone(),
        policy_violation: result.policy_violation.map(|violation| EnginePolicyViolation {
            code: violation.code,
            reason: violation.reason,
        }),
    }))
}

/// The resolve options carrying the maturity and trust policies of the
/// install path.
fn engine_resolve_options(config: &Config) -> Result<ResolveOptions> {
    let published_by = engine_release_cutoff(config)?;
    // The running version is already on this machine, so hiding it behind the
    // maturity cutoff protects nothing — it only makes a dist-tag that points
    // at it fall back to an older release, downgrading the user
    // (pnpm/pnpm#13883).
    let mut exclude_patterns = config.minimum_release_age_exclude.clone().unwrap_or_default();
    exclude_patterns.push(format!("pnpm@{PNPM_VERSION}"));
    let published_by_exclude =
        pnpm_config::version_policy::create_package_version_policy(&exclude_patterns)
            .into_diagnostic()
            .wrap_err("compile the minimum-release-age-exclude policy")
            .map(Some)?;
    let trust_policy = match config.trust_policy {
        pnpm_config::TrustPolicy::Off => None,
        pnpm_config::TrustPolicy::NoDowngrade => Some(pnpm_config::TrustPolicy::NoDowngrade),
    };
    let compiled_exclude = config
        .trust_policy_exclude
        .as_deref()
        .filter(|patterns| !patterns.is_empty())
        .map(pnpm_config::version_policy::create_package_version_policy)
        .transpose();
    let trust_policy_exclude =
        compiled_exclude.into_diagnostic().wrap_err("compile the trust-policy-exclude policy")?;

    Ok(ResolveOptions {
        default_tag: Some("latest".to_string()),
        published_by,
        published_by_exclude,
        trust_policy,
        trust_policy_exclude,
        trust_policy_ignore_after: config.trust_policy_ignore_after,
        ..ResolveOptions::default()
    })
}

/// Add config dependencies: resolve + install them (merged with any
/// already-declared config deps), then write the clean specifiers into
/// `pnpm-workspace.yaml`'s `configDependencies` block. Backs
/// `pacquet add --config`.
pub async fn add_config_dependencies<Reporter: self::Reporter>(
    config: &Config,
    root_dir: &Path,
    added: &BTreeMap<String, String>,
) -> Result<()> {
    let mut config_dependencies = config.config_dependencies.clone().unwrap_or_default();
    for (name, specifier) in added {
        config_dependencies
            .insert(name.clone(), ConfigDependency::VersionWithIntegrity(specifier.clone()));
    }

    resolve_and_install::<Reporter>(config, &config_dependencies, root_dir, false).await?;

    pnpm_workspace_manifest_writer::set_config_dependencies(
        root_dir,
        added.iter().map(|(name, specifier)| (name.as_str(), specifier.as_str())),
    )
    .into_diagnostic()
    .wrap_err("recording the config dependencies in pnpm-workspace.yaml")
}

/// Build the resolver + install options from `config` and resolve +
/// install `config_dependencies`. Shared by [`install_config_deps`] and
/// [`add_config_dependencies`].
async fn resolve_and_install<Reporter: self::Reporter>(
    config: &Config,
    config_dependencies: &std::collections::BTreeMap<String, ConfigDependency>,
    root_dir: &Path,
    frozen_lockfile: bool,
) -> Result<()> {
    let context = EnvInstallerContext::new(config)?;
    context.http_client.set_warning_handler(pnpm_reporter::emit_global_warning::<Reporter>);
    let options = context.options(root_dir, frozen_lockfile);

    resolve_and_install_config_deps::<Reporter>(config_dependencies, &context.resolver, &options)
        .await
        .map_err(miette::Report::new)
        .wrap_err("install configurational dependencies")
}

struct EnvInstallerContext {
    http_client: Arc<ThrottledClient>,
    auth_headers: Arc<pnpm_network::AuthHeaders>,
    registries: HashMap<String, String>,
    retry_opts: RetryOpts,
    store_dir: &'static StoreDir,
    node_version: String,
    verify_store_integrity: bool,
    strict_store_pkg_content_check: bool,
    offline: bool,
    package_import_method: pnpm_config::PackageImportMethod,
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
    /// [`PackageManagerBootstrap`](pnpm_config::PackageManagerBootstrap)
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
        proxy: &pnpm_network::ProxyConfig,
        tls: &pnpm_network::TlsConfig,
        tls_by_uri: &pnpm_network::PerRegistryTls,
        registries: std::collections::BTreeMap<String, String>,
        auth_headers: Arc<pnpm_network::AuthHeaders>,
    ) -> Result<Self> {
        let http_client = Arc::new(
            ThrottledClient::for_installs(proxy, tls, tls_by_uri, &config.network_settings())
                .into_diagnostic()
                .wrap_err("create the network client for env-installer dependencies")?
                .with_max_sockets_per_host(config.max_sockets),
        );

        let registries: HashMap<String, String> = registries.into_iter().collect();
        let retry_opts = config.retry_opts();
        let resolver = NpmResolver {
            registries: registries.clone(),
            registries_by_prefix: HashMap::new(),
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
            needs_full_metadata_for: None,
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
            strict_store_pkg_content_check: config.strict_store_pkg_content_check,
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
            strict_store_pkg_content_check: self.strict_store_pkg_content_check,
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

#[cfg(test)]
mod tests;

/// Fail closed when the configured maturity cutoff cannot be represented.
fn engine_release_cutoff(config: &Config) -> Result<Option<chrono::DateTime<chrono::Utc>>> {
    Ok(match config.resolved_minimum_release_age() {
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
    })
}

mod hooks;
