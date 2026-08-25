//! Install a registry-published package-manager engine into the shared
//! global virtual store.
//!
//! The engine lands in `<store>/links/...` (shared across invocations and,
//! unlike `self-update`, not registered in the global packages directory,
//! so `pnpm ls -g` does not see it). A genuine download has its registry
//! signature checked before the engine runs; where no signature can be
//! obtained and the release was resolved through the user's own trusted
//! configuration, the install proceeds on the lockfile's integrity pin
//! with a warning (see [`crate::cli_args::self_update::verify_engine`]).
//! Native target installs have their platform binary linked, and the
//! package bins are linked into a `bin/` directory the caller prepends to
//! `PATH`.

use miette::{Context, IntoDiagnostic};
use pnpm_cmd_shim::{
    Host as CmdShimHost, LinkBinsOptions, PackageBinSource, link_bins_of_packages,
};
use pnpm_config::Config;
use pnpm_fs::DirLock;
use pnpm_graph_hasher::{detect_node_major, engine_name};
use pnpm_lockfile::{EnvLockfile, PackageKey};
use pnpm_package_manager::{AllowBuildPolicy, VirtualStoreLayout};
use pnpm_package_manifest::parse_manifest;
use pnpm_reporter::{LogEvent, LogLevel, PnpmLog, Reporter};
use pnpm_store_dir::StoreDir;
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use crate::{
    cli_args::self_update::{
        install_pnpm::{link_exe_platform_binary, package_dir, run_install},
        verify_engine::{EngineToVerify, PlatformBinaries, verify_engine_identity},
    },
    config_deps,
    engine_pm::{
        channel::{EnginePackages, PackageManager},
        error::EngineError,
    },
};

/// Install the `pm` engine for `version` into the global virtual store and
/// return the directory holding its linked bins.
///
/// `env_root` is where the engine's env lockfile (its resolved package
/// closure) is written, under the pnpm home directory. `spec` is the
/// user's bare specifier (a version, range, or dist-tag) and `version` the
/// exact version it resolved to. `force_resync` and `frozen_lockfile` are
/// the [`resolve_package_manager_integrities`] flags of the same name; only
/// a caller whose `env_root` is the project itself passes the latter, since
/// a global env lockfile is not what `--frozen-lockfile` freezes.
///
/// [`resolve_package_manager_integrities`]: pnpm_env_installer::resolve_package_manager_integrities
pub(crate) async fn install_engine_to_store<Reporter: self::Reporter + 'static>(
    config: &'static Config,
    pm: PackageManager,
    env_root: &Path,
    spec: &str,
    version: &str,
    frozen_lockfile: bool,
    force_resync: bool,
) -> miette::Result<PathBuf> {
    let packages = registry_engine_packages(pm, version)?;
    let config = package_manager_engine_config(config)?.leak();
    fs::create_dir_all(env_root).into_diagnostic().wrap_err_with(|| {
        format!("create the package-manager env directory at {}", env_root.display())
    })?;
    let env = {
        let _lock = package_manager_env_lock::<Reporter>(config).await?;
        // Resolve the package-manager closure into the env lockfile (a no-op
        // when this spec+version is already recorded there and a resync is
        // not forced).
        config_deps::sync_engine_dependencies(
            config,
            env_root,
            packages.pinned,
            spec,
            version,
            frozen_lockfile,
            force_resync,
        )
        .await?
    };
    install_engine_from_env::<Reporter>(config, pm, &env, version).await
}

pub(crate) async fn install_engine_from_env<Reporter: self::Reporter + 'static>(
    config: &'static Config,
    pm: PackageManager,
    env: &EnvLockfile,
    version: &str,
) -> miette::Result<PathBuf> {
    let config = package_manager_engine_config(config)?.leak();
    install_engine_from_env_with_config::<Reporter>(config, pm, env, version).await
}

/// The packages that make up `pm` at `version`. Errors for an engine that
/// ships as a platform archive instead of npm packages — the binary
/// channels never reach this installer.
fn registry_engine_packages(pm: PackageManager, version: &str) -> miette::Result<EnginePackages> {
    let name = pm.name();
    pm.engine_packages(version).ok_or_else(|| {
        EngineError::NotRegistryPublished { name, version: version.to_string() }.into()
    })
}

async fn install_engine_from_env_with_config<Reporter: self::Reporter + 'static>(
    config: &'static Config,
    pm: PackageManager,
    env: &EnvLockfile,
    version: &str,
) -> miette::Result<PathBuf> {
    let package = registry_engine_packages(pm, version)?;
    let package_name = package.wrapper;
    // Cache hit: when the engine already sits in its GVS slot, skip both
    // the signature check and the install — short-circuit on the engine's
    // `package.json` already existing. The slot is computed
    // with the same hashing the install pipeline uses, so a stale or wrong
    // computation merely misses the cache (the idempotent install below
    // then re-derives the slot from the install's own symlink).
    if let Some(slot) = compute_engine_slot(config, env, package, version) {
        let pkg_dir = package_dir(&slot, package_name);
        if pkg_dir.join("package.json").exists()
            && let Ok(bin_dir) =
                link_cached_engine_bins(&slot, package_name, package.links_native_binary)
        {
            return Ok(bin_dir);
        }
    }

    // The engine's global-virtual-store slot is shared by every process on
    // the host, and materializing it is destructive: a slot left carrying
    // an interrupted-build marker is removed and re-staged. A task runner
    // that pins `packageManager` spawns many `pnpm run` children at once,
    // all of which reach this point together on a cold cache; without the
    // lock, one clears the slot out from under another and the loser dies
    // looking for a binary that no longer exists.
    let _lock = engine_install_lock::<Reporter>(config, package_name, version);
    // The wait may have been for a process that installed the very engine
    // we want, so ask the cache again before paying for the download.
    if let Some(slot) = compute_engine_slot(config, env, package, version) {
        let pkg_dir = package_dir(&slot, package_name);
        if pkg_dir.join("package.json").exists()
            && let Ok(bin_dir) =
                link_cached_engine_bins(&slot, package_name, package.links_native_binary)
        {
            return Ok(bin_dir);
        }
    }

    // Genuine download: verify the engine's registry signature before
    // installing or executing it.
    let label = format!("{}@{version}", pm.name());
    let engine = EngineToVerify {
        label: &label,
        packages: package.pinned,
        platform_binaries: if package.links_native_binary {
            PlatformBinaries::PnpmExe
        } else {
            PlatformBinaries::None
        },
    };
    if let Some(warning) = verify_engine_identity(env, &engine, config)
        .await
        .map_err(miette::Report::new)
        .wrap_err("verify the package manager identity")?
    {
        Reporter::emit(&LogEvent::Pnpm(PnpmLog {
            level: LogLevel::Warn,
            message: warning,
            prefix: String::new(),
        }));
    }

    // Install into a throwaway directory with the global virtual store
    // enabled, so the engine itself materializes in `<store>/links/...`
    // and the temp directory holds only symlinks into it.
    let tmp_install_dir =
        config.store_dir.tmp().join(format!("{}-engine-{version}-{}", pm.name(), unique_suffix()));
    fs::create_dir_all(&tmp_install_dir)
        .into_diagnostic()
        .wrap_err("create the temporary package manager install directory")?;
    let slot = Box::pin(run_install::<Reporter>(
        config,
        &tmp_install_dir,
        package_name,
        version,
        config.supported_architectures.clone(),
        Some(package.pinned),
    ))
    .await
    .and_then(|()| resolve_slot(&tmp_install_dir, package_name));
    // The temp directory held only symlinks into the GVS, so removing it
    // does not touch the installed engine. Refuse to recurse through a
    // symlink at the temp path as defense-in-depth — even though the store
    // is within pnpm's trust domain — mirroring the guard `patch-commit`
    // applies before its own `remove_dir_all`.
    let _ = remove_dir_if_not_symlink(&tmp_install_dir);
    let slot = slot?;

    let pkg_dir = package_dir(&slot, package_name);
    let bin_dir = slot.join("bin");
    if package.links_native_binary {
        // Replicate the wrapper's preinstall (skipped because the engine is
        // installed with scripts disabled): link the host's native binary.
        link_exe_platform_binary(&slot, package_name)?;
    }
    link_bins(&pkg_dir, &bin_dir)?;
    Ok(bin_dir)
}

/// Take the host-wide lock guarding this engine's install, or `None`
/// when it can't be taken. Losing the lock is not a reason to refuse to
/// run: the install below is the same one every other process is racing
/// to perform, so proceeding unserialized is exactly today's behavior.
fn engine_install_lock<Reporter: self::Reporter>(
    config: &Config,
    package_name: &str,
    version: &str,
) -> Option<DirLock> {
    let name = format!("{}@{version}.lock", package_name.replace('/', "+"));
    let path = config.store_dir.tmp().join("engine-locks").join(name);
    acquire_install_lock::<Reporter>(&path, "the package manager engine install")
}

async fn package_manager_env_lock<Reporter: self::Reporter>(
    config: &Config,
) -> miette::Result<Option<DirLock>> {
    let path = config.store_dir.tmp().join("engine-locks/package-manager-env.lock");
    tokio::task::spawn_blocking(move || {
        acquire_install_lock::<Reporter>(&path, "the package-manager environment")
    })
    .await
    .into_diagnostic()
    .wrap_err("wait for the package-manager environment lock")
}

fn acquire_install_lock<Reporter: self::Reporter>(path: &Path, subject: &str) -> Option<DirLock> {
    /// Long enough for a cold install of the engine over a slow link,
    /// short enough that a wedged host isn't hung on indefinitely.
    const WAIT: Duration = Duration::from_mins(5);
    /// Comfortably above `WAIT`, so a process still legitimately
    /// installing never has its lock stolen by one that gave up waiting.
    const ABANDONED_AFTER: Duration = Duration::from_mins(30);

    let error = match DirLock::acquire(path.to_path_buf(), WAIT, ABANDONED_AFTER) {
        Ok(lock) => return lock,
        Err(error) => error,
    };
    // Through the reporter rather than `tracing`, which is only wired to
    // a subscriber when `TRACE` is set and would drop this on every
    // ordinary run.
    Reporter::emit(&LogEvent::Pnpm(PnpmLog {
        level: LogLevel::Warn,
        message: format!(
            "Could not lock {subject} at {}: {error}. Installing without it, which is unsafe if another pnpm is installing concurrently.",
            path.display(),
        ),
        prefix: String::new(),
    }));
    None
}

fn package_manager_engine_config(config: &Config) -> miette::Result<Config> {
    let global_pkg_dir = config.global_pkg_dir.as_ref().ok_or(EngineError::NoGlobalDir)?;
    let mut config = config.clone();
    config.store_dir = StoreDir::new(package_manager_engine_store_root(global_pkg_dir));
    config.global_virtual_store_dir = config.store_dir.links();
    Ok(config)
}

fn package_manager_engine_store_root(global_pkg_dir: &Path) -> PathBuf {
    package_manager_home(global_pkg_dir).join("package-manager-store")
}

/// Where an engine's env lockfile lives — the file that pins the bytes of
/// every package the engine is installed from.
///
/// pnpm's own stays in the global packages directory, where earlier
/// releases already wrote it. The other package managers get a directory
/// each, so resolving one never rewrites another's pins.
pub(crate) fn engine_env_root(config: &Config, pm: PackageManager) -> miette::Result<PathBuf> {
    let global_pkg_dir = config.global_pkg_dir.as_ref().ok_or(EngineError::NoGlobalDir)?;
    if pm == PackageManager::Pnpm {
        return Ok(global_pkg_dir.clone());
    }
    Ok(package_manager_home(global_pkg_dir).join("package-manager-envs").join(pm.name()))
}

/// The pnpm home directory, derived from the versioned global packages
/// directory (`<home>/global/<version>`) it contains.
fn package_manager_home(global_pkg_dir: &Path) -> &Path {
    global_pkg_dir.parent().and_then(Path::parent).unwrap_or(global_pkg_dir)
}

fn link_cached_engine_bins(
    slot: &Path,
    package_name: &str,
    links_native_binary: bool,
) -> miette::Result<PathBuf> {
    let pkg_dir = package_dir(slot, package_name);
    let bin_dir = slot.join("bin");
    if links_native_binary {
        link_exe_platform_binary(slot, package_name)?;
    }
    link_bins(&pkg_dir, &bin_dir)?;
    Ok(bin_dir)
}

/// The global-virtual-store slot the selected engine wrapper resolves to, or `None`
/// when it can't be derived (e.g. the engine snapshot is missing or the
/// allow-build policy fails to compile). Drives only the cache-hit
/// short-circuit, so `None` is a safe "treat as a miss".
fn compute_engine_slot(
    config: &Config,
    env: &EnvLockfile,
    packages: EnginePackages,
    version: &str,
) -> Option<PathBuf> {
    let wanted: PackageKey = format!("{}@{version}", packages.wrapper).parse().ok()?;
    let key = env.snapshots.keys().find(|key| key.without_peer() == wanted)?.clone();

    let mut cfg = config.clone();
    cfg.enable_global_virtual_store = true;
    cfg.global_virtual_store_dir = config.store_dir.links();
    cfg.allow_builds.clear();
    for name in packages.pinned {
        cfg.allow_builds.insert((*name).to_string(), true);
    }
    let policy = AllowBuildPolicy::from_config(&cfg).ok()?;
    let engine = detect_node_major().map(|major| engine_name(major, None, None));
    let layout = VirtualStoreLayout::new(
        &cfg,
        engine.as_deref(),
        Some(&env.snapshots),
        Some(&env.packages),
        Some(&policy),
        // No lockfile directory: this lockfile only ever holds the engine
        // packages and their registry dependencies, and the install that
        // materializes the slot runs from a throwaway directory that
        // differs on every run.
        None,
    );
    Some(layout.slot_dir(&key))
}

/// Derive the engine's GVS slot from the install's own wrapper symlink. This
/// is the ground truth after an install, independent of any hash
/// recomputation.
fn resolve_slot(install_dir: &Path, package_name: &str) -> miette::Result<PathBuf> {
    let link = package_dir(install_dir, package_name);
    let real = fs::canonicalize(&link)
        .into_diagnostic()
        .wrap_err_with(|| format!("resolve the installed {package_name} at {}", link.display()))?;
    slot_from_package_dir(&real, package_name).ok_or_else(|| {
        miette::miette!("could not locate the {package_name} global-virtual-store slot")
    })
}

pub(crate) fn slot_from_package_dir(package_dir: &Path, package_name: &str) -> Option<PathBuf> {
    let mut slot = package_dir;
    for _ in package_name.split('/') {
        slot = slot.parent()?;
    }
    slot.parent().map(Path::to_path_buf)
}

/// Link the wrapper's declared bins into `bin_dir` after the engine
/// install.
fn link_bins(pkg_dir: &Path, bin_dir: &Path) -> miette::Result<()> {
    let manifest_path = pkg_dir.join("package.json");
    let text = fs::read_to_string(&manifest_path)
        .into_diagnostic()
        .wrap_err_with(|| format!("read {}", manifest_path.display()))?;
    let manifest: Value = parse_manifest(&text)
        .into_diagnostic()
        .wrap_err_with(|| format!("parse {}", manifest_path.display()))?;
    let source = PackageBinSource::new(pkg_dir.to_path_buf(), Arc::new(manifest));
    link_bins_of_packages::<CmdShimHost>(&[source], bin_dir, &LinkBinsOptions::default())
        .map_err(miette::Report::new)
        .wrap_err("link the package manager bins")
}

/// Remove `path` and its contents, refusing to recurse through a symlink
/// at `path` itself. A missing path is success. Mirrors the symlink guard
/// `patch-commit` applies before `remove_dir_all`.
fn remove_dir_if_not_symlink(path: &Path) -> std::io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "temporary directory must not be a symbolic link",
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    }
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// A best-effort unique component for the temporary install directory
/// name, so concurrent `pnpm with` invocations don't collide.
fn unique_suffix() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos =
        SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_nanos());
    format!("{}-{nanos}", std::process::id())
}

#[cfg(test)]
mod tests;
