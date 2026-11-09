//! Resolve and install configurational dependencies before the main
//! install runs.
//!
//! Config dependencies are materialized at config-finalization time, so
//! the env lockfile (the first YAML document of `pnpm-lock.yaml`) is
//! written before the regular install reads or rewrites the wanted
//! lockfile. Plugin-hook loading (the `updateConfig` half) is wired in
//! separately.

pub use declarations::{add_config_dependencies, update_config_dependencies};
pub use engine_policy::mature_pnpm_version_for_range;
pub use hooks::{
    apply_config_dependency_settings, load_before_packing_hooks, may_update_config, prepare_config,
    run_update_config_hooks,
};

pub(crate) use declarations::declared_specifier;

mod declarations;
mod env_installer_context;
mod network;
mod store;

use crate::{
    cli_args::config_warnings::emit_config_warning, config_overrides::apply_store_dir_override,
};
use engine_policy::engine_resolve_options;
use env_installer_context::EnvInstallerContext;

use miette::{IntoDiagnostic, Result, WrapErr};
use pnpm_catalogs_config::get_catalogs_from_workspace_manifest;
use pnpm_config::{
    Config, Host, PNPM_VERSION, TrustedSettings, WorkspaceSettings, default_state_dir,
    known_settings::is_known_setting_key, resolve_configured_state_dir,
};
use pnpm_env_installer::{
    ConfigDepUpdates, ConfigDepsInstallMode, pnpm_engine_packages, read_config_dependency_lockfile,
    resolve_and_install_config_deps_updating, resolve_package_manager_integrities,
    running_version_unpublished,
};
use pnpm_hooks::{HookContext, LogFn, PnpmfileHooks, finder};
use pnpm_lockfile::{EnvDocumentFormat, EnvLockfile};
use pnpm_reporter::{HookLog, LogEvent, LogLevel, PnpmLog, Reporter};
use pnpm_resolving_resolver_base::{
    ResolutionPolicyOptions, ResolveOptions, Resolver, WantedDependency,
};
use pnpm_workspace_state::ConfigDependency;
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
    sync::Arc,
};

/// Prepare the project's `configDependencies` according to `mode`.
/// `root_dir` is the lockfile directory;
/// `frozen_lockfile` forwards `--frozen-lockfile` so config deps refuse
/// to mutate an out-of-date env lockfile.
pub async fn install_config_deps<Reporter: self::Reporter>(
    config: &Config,
    root_dir: &Path,
    frozen_lockfile: bool,
    mode: ConfigDepsInstallMode,
) -> Result<()> {
    let empty = BTreeMap::new();
    let config_dependencies = config.config_dependencies.as_ref().unwrap_or(&empty);
    let updates = ConfigDepUpdates::default();
    resolve_and_install::<Reporter>(
        config,
        config_dependencies,
        root_dir,
        frozen_lockfile,
        &updates,
        mode,
    )
    .await?;
    Ok(())
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
    install_config_deps::<Reporter>(
        config,
        root_dir,
        frozen_lockfile,
        ConfigDepsInstallMode::Reconcile,
    )
    .await?;
    apply_config_dependency_settings(config)?;
    run_update_config_hooks::<Reporter>(config, root_dir).await?;
    Ok(())
}

/// Record the pin of the pnpm already running, warning instead of failing
/// when [`running_version_unpublished`] holds. The pin stays in the
/// manifest, and a later run records it.
///
/// A caller that reads back what it recorded, to install or verify it, wants
/// [`sync_package_manager_dependencies`] instead.
pub async fn record_running_package_manager_pin(
    config: &Config,
    root_dir: &Path,
    wanted_specifier: &str,
    pnpm_version: &str,
    frozen_lockfile: bool,
    force_resync: bool,
) -> Result<()> {
    let boot_config = config.for_boot();
    let config = &*boot_config;
    let context = EnvInstallerContext::for_package_manager(config)?;
    let options =
        context.options(root_dir, frozen_lockfile, EnvDocumentFormat::for_pnpm(pnpm_version));
    let resolved = resolve_package_manager_integrities(
        pnpm_engine_packages(pnpm_version),
        wanted_specifier,
        pnpm_version,
        &context.resolver,
        &options,
        force_resync,
    )
    .await;
    match resolved {
        Ok(_) => Ok(()),
        Err(error) if running_version_unpublished(&error, pnpm_version) => {
            emit_config_warning(&format!(
                "The registry publishes no pnpm {pnpm_version}, so the packageManager pin was \
                 not recorded in pnpm-lock.yaml. Continuing with the running pnpm.",
            ));
            Ok(())
        }
        Err(error) => {
            Err(miette::Report::new(error)).wrap_err("resolve package manager dependencies")
        }
    }
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
        &config.for_boot(),
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
    let options = context.options(root_dir, frozen_lockfile, EnvDocumentFormat::for_pnpm(version));
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
/// regular install.
pub async fn resolve_engine_version(
    config: &Config,
    package: &str,
    bare_specifier: &str,
) -> Result<Option<ResolvedEngine>> {
    resolve_engine(config, package, bare_specifier, false).await
}

/// Resolve as [`resolve_engine_version`], but without the `minimumReleaseAge`
/// cutoff. `self-update` uses this to tell a lagging `latest` dist-tag
/// from a pick that is older only because the true latest is still within
/// the cutoff (pnpm/pnpm#12006).
pub async fn resolve_engine_version_ignoring_maturity(
    config: &Config,
    package: &str,
    bare_specifier: &str,
) -> Result<Option<ResolvedEngine>> {
    resolve_engine(config, package, bare_specifier, true).await
}

async fn resolve_engine(
    config: &Config,
    package: &str,
    bare_specifier: &str,
    ignore_maturity: bool,
) -> Result<Option<ResolvedEngine>> {
    let boot_config = config.for_boot();
    let config = &*boot_config;
    let context = EnvInstallerContext::for_package_manager(config)?;
    let opts = engine_resolve_options(config, ignore_maturity)?;
    resolve_engine_with(&context, package, bare_specifier, &opts).await
}

async fn resolve_engine_with(
    context: &EnvInstallerContext,
    package: &str,
    bare_specifier: &str,
    opts: &ResolveOptions,
) -> Result<Option<ResolvedEngine>> {
    let wanted = WantedDependency {
        alias: Some(package.to_string()),
        bare_specifier: Some(bare_specifier.to_string()),
        ..WantedDependency::default()
    };
    let result = context.resolver
        .resolve(&wanted, opts)
        .await
        .map_err(|error| miette::miette!("{error}"))
        .wrap_err_with(|| format!("resolve {package}@{bare_specifier}"))?;
    let Some(result) = result else {
        return Ok(None);
    };
    let Some(name_ver) = result.package.name_ver else {
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
        manifest: result.package.manifest.clone(),
        policy_violation: result.policy_violation.map(|violation| EnginePolicyViolation {
            code: violation.code,
            reason: violation.reason,
        }),
    }))
}

/// Build the resolver + install options from the boot policy of `config`
/// (see [`Config::for_boot`]) and resolve + install `config_dependencies`.
async fn resolve_and_install<Reporter: self::Reporter>(
    config: &Config,
    config_dependencies: &std::collections::BTreeMap<String, ConfigDependency>,
    root_dir: &Path,
    frozen_lockfile: bool,
    updates: &ConfigDepUpdates,
    mode: ConfigDepsInstallMode,
) -> Result<BTreeMap<String, String>> {
    let Some(env_lockfile) =
        read_config_dependency_lockfile(Some(config_dependencies), root_dir, mode)?
    else {
        return Ok(BTreeMap::new());
    };
    let boot_config = config.for_boot();
    let config = &*boot_config;
    let _store_lock = store::lock_config_dependency_store::<Reporter>(config)?;

    let context = EnvInstallerContext::new(config)?;
    context.network.http_client.set_warning_handler(pnpm_reporter::emit_global_warning::<Reporter>);
    // The running pnpm installs the config dependencies, and after a version
    // switch it is the pinned one, so its own version picks the format.
    let mut options =
        context.options(root_dir, frozen_lockfile, EnvDocumentFormat::for_pnpm(PNPM_VERSION));
    options.mode = mode;
    // Sampled before the verifiers so the resolution cutoff is never later
    // than the verification cutoff.
    options.verification.resolution_policy = release_age_policy(config)?;
    options.verification.resolution_verifiers = context.resolution_verifiers(config)?;
    let store_index =
        store::StoreIndexSession::attach(&mut options.store_index, context.store.dir, config).await;
    let result = resolve_and_install_config_deps_updating::<Reporter>(
        env_lockfile,
        config_dependencies,
        updates,
        &context.resolver,
        &options,
    )
    .await
    .map_err(miette::Report::new)
    .wrap_err("install configurational dependencies");
    drop(options);
    store_index.drain().await;
    result
}

/// The `minimumReleaseAge` cutoff that the install's resolution verifiers
/// enforce on config dependencies, for resolving them.
fn release_age_policy(config: &Config) -> Result<ResolutionPolicyOptions> {
    let policy = pnpm_package_manager::PickPolicy::from_config(config).into_diagnostic()?;
    Ok(ResolutionPolicyOptions {
        published_by: policy.published_by,
        published_by_exclude: policy.published_by_exclude,
        ..ResolutionPolicyOptions::default()
    })
}

#[cfg(test)]
mod tests;

mod hooks;

mod engine_policy;
