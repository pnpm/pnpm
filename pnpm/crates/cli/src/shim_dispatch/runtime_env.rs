//! Materialization of pinned runtimes into the trusted global virtual
//! store, under configuration a project cannot influence.
//!
//! A runtime's slot is entered only under its [`crate::slot_lock`]. When
//! another process holds that lock, the runtime is installed into a
//! [`PrivateInstall`] of this process's own instead, and runs from there.

use crate::{State, cli_args::add::add_package, slot_lock};
use miette::{Context, IntoDiagnostic};
use pnpm_config::{Config, Host, NodeLinker};
use pnpm_crypto_hash::create_hex_hash;
use pnpm_fs::DirLock;
use pnpm_package_manifest::DependencyGroup;
use pnpm_registry::RangeSpecStyle;
use pnpm_reporter::SilentReporter;
use pnpm_store_dir::PrivateInstall;
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
};

/// A runtime installed and ready to run.
pub(crate) struct MaterializedRuntime {
    /// The runtime's real executable.
    pub(crate) bin: PathBuf,
    /// The private directory the runtime was installed into when its
    /// shared slot was held. It is removed when dropped, and the runtime
    /// runs from it, so it lives for as long as the runtime may run.
    pub(crate) private_install: Option<PrivateInstall>,
}

impl MaterializedRuntime {
    fn shared(bin: PathBuf) -> Self {
        Self { bin, private_install: None }
    }
}

pub(super) const RUNTIME_ENVS_DIR_NAME: &str = "global-shim-runtimes";

/// Where package-manager provisioning anchors its configuration, for the
/// same reason [`RUNTIME_ENVS_DIR_NAME`] exists: a seeded workspace
/// manifest there stops ancestor discovery, so no project can steer the
/// install.
pub(super) const PACKAGE_MANAGER_ENVS_DIR_NAME: &str = "global-shim-package-managers";

/// The configuration the managed runtime installs under. The runtime's
/// store, mirrors, and registry selection must not be
/// project-controllable: a repository that redirects `storeDir` could
/// pre-seed a poisoned global-virtual-store slot for the executable a
/// dispatch is about to run. The configuration is therefore anchored
/// inside pnpm's own state dir — the seeded empty workspace manifest
/// stops ancestor discovery, so only the global `config.yaml`, user
/// files, and the environment contribute.
pub(crate) fn trusted_runtime_config(environments_dir: &Path) -> miette::Result<Config> {
    fs::create_dir_all(environments_dir)
        .into_diagnostic()
        .wrap_err_with(|| format!("create {}", environments_dir.display()))?;
    let workspace_manifest = environments_dir.join("pnpm-workspace.yaml");
    if !workspace_manifest.is_file() {
        fs::write(&workspace_manifest, "{}\n")
            .into_diagnostic()
            .wrap_err_with(|| format!("seed {}", workspace_manifest.display()))?;
    }
    Config::default()
        .current::<Host>(environments_dir)
        .map_err(miette::Report::new)
        .wrap_err("load configuration for the managed runtime")
}

/// Point `config` into the runtime environment and strip everything a
/// dependency install may not do on the dispatcher's behalf: lifecycle
/// scripts, build approvals, and project dependency rewrites are
/// disabled, and the install always targets the host architecture — the
/// dispatched runtime executes on this host, and honoring a
/// `supportedArchitectures.libc: [musl]` override on a glibc host would
/// select an unofficial-builds artifact that the promptless policy's
/// host-libc check did not account for.
///
/// The runtime materializes in the global virtual store at
/// `global_virtual_store_dir`, or self-contained inside the environment
/// when that is `None` (a private install).
///
/// The linker is pinned to isolated for the same reason: a `hoisted`
/// setting inherited from the environment or the global config would
/// materialize the runtime inside the environment directory, where
/// [`managed_runtime_bin`] does not accept it for a shared install.
pub(super) fn hardened_install_config(
    config: Config,
    environment_dir: &Path,
    global_virtual_store_dir: Option<PathBuf>,
) -> Config {
    let mut install_config = config;
    install_config.modules_dir = environment_dir.join("node_modules");
    install_config.virtual_store_dir = environment_dir.join("node_modules").join(".pnpm");
    install_config.enable_global_virtual_store = global_virtual_store_dir.is_some();
    if let Some(global_virtual_store_dir) = global_virtual_store_dir {
        install_config.global_virtual_store_dir = global_virtual_store_dir;
    }
    install_config.node_linker = NodeLinker::Isolated;
    install_config.workspace_dir = Some(environment_dir.to_path_buf());
    install_config.lockfile = true;
    install_config.frozen_lockfile = Some(false);
    install_config.prefer_frozen_lockfile = false;
    install_config.ignore_scripts = true;
    install_config.dangerously_allow_all_builds = false;
    install_config.strict_dep_builds = false;
    install_config.supported_architectures = None;
    install_config.allow_builds.clear();
    install_config.overrides = None;
    install_config.package_extensions = None;
    install_config.catalogs = None;
    install_config.patched_dependencies = None;
    install_config
}

/// Materialize a runtime into the configured global virtual store, or
/// privately when its slot is held, and return its real executable. The
/// small environment under pnpm's state directory contains only the
/// lockfile and symlinks required to address the GVS slot; project
/// `node_modules` is never consulted.
pub(crate) async fn materialize_runtime(
    state_dir: &Path,
    name: String,
    version_spec: String,
) -> miette::Result<MaterializedRuntime> {
    if state_dir.as_os_str().is_empty() {
        return Err(miette::miette!("the pnpm state directory could not be resolved"));
    }
    let environments_dir = state_dir.join(RUNTIME_ENVS_DIR_NAME);
    let config = trusted_runtime_config(&environments_dir)?;
    let global_virtual_store_dir = config.store_dir.links();
    let key = create_hex_hash(&format!(
        "runtime\0{name}\0{version_spec}\0{}",
        global_virtual_store_dir.display(),
    ));
    let environment_dir = environments_dir.join(&key);
    if let Some(bin) = managed_runtime_bin(&environment_dir, &name, &global_virtual_store_dir) {
        return Ok(MaterializedRuntime::shared(bin));
    }

    let Some(_lock) = runtime_slot_lock(&environments_dir, &key) else {
        return install_runtime_privately(config, &name, &version_spec).await;
    };
    if let Some(bin) = managed_runtime_bin(&environment_dir, &name, &global_virtual_store_dir) {
        return Ok(MaterializedRuntime::shared(bin));
    }

    reset_environment(&environment_dir)?;
    let bin = Box::pin(install_runtime(
        config,
        &environment_dir,
        Some(global_virtual_store_dir),
        &name,
        &version_spec,
    ))
    .await?;
    Ok(MaterializedRuntime::shared(bin))
}

/// Take the lock guarding the runtime's slot, or `None` when another
/// process holds it: the runtime is then installed privately. A lock that
/// cannot be established at all is treated the same, since the private
/// install needs none.
fn runtime_slot_lock(environments_dir: &Path, key: &str) -> Option<DirLock> {
    let lock_path = environments_dir.join(format!("{key}.lock"));
    match slot_lock::acquire(lock_path.clone()) {
        Ok(lock) => lock,
        Err(error) => {
            tracing::warn!(
                target: "pacquet::shim_dispatch",
                path = %lock_path.display(),
                %error,
                "could not lock the managed runtime slot; installing it privately",
            );
            None
        }
    }
}

/// Install the runtime into a private directory of this process's own,
/// self-contained rather than linked into the global virtual store, for
/// when another process holds the slot lock.
async fn install_runtime_privately(
    config: Config,
    name: &str,
    version_spec: &str,
) -> miette::Result<MaterializedRuntime> {
    let private_install = config.store_dir
        .create_private_install(&name.replace('/', "+"))
        .map_err(miette::Report::new)
        .wrap_err("create the private runtime install directory")?;
    let bin =
        Box::pin(install_runtime(config, private_install.dir(), None, name, version_spec)).await?;
    Ok(MaterializedRuntime { bin, private_install: Some(private_install) })
}

/// The runtime's real executable, when the package installed as `name`
/// in the environment resolves inside `trusted_root`: the global virtual
/// store for a shared install, the environment itself for a private one.
pub(super) fn managed_runtime_bin(
    environment_dir: &Path,
    name: &str,
    trusted_root: &Path,
) -> Option<PathBuf> {
    let package_dir = dunce::canonicalize(environment_dir.join("node_modules").join(name)).ok()?;
    let trusted_root = dunce::canonicalize(trusted_root).ok()?;
    if !package_dir.starts_with(&trusted_root) {
        return None;
    }
    let manifest: Value =
        serde_json::from_slice(&fs::read(package_dir.join("package.json")).ok()?).ok()?;
    if manifest.get("name").and_then(Value::as_str) != Some(name) {
        return None;
    }
    let bin_path = match manifest.get("bin")? {
        Value::String(path) => path.as_str(),
        Value::Object(bins) => bins.get(name)?.as_str()?,
        _ => return None,
    };
    let bin = dunce::canonicalize(package_dir.join(bin_path)).ok()?;
    (bin.starts_with(&package_dir) && bin.is_file()).then_some(bin)
}

pub(super) fn remove_dir_if_not_symlink(path: &Path) -> std::io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "managed runtime environment must not be a symbolic link",
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    }
    fs::remove_dir_all(path)
}

fn reset_environment(environment_dir: &Path) -> miette::Result<()> {
    remove_dir_if_not_symlink(environment_dir)
        .into_diagnostic()
        .wrap_err_with(|| format!("reset {}", environment_dir.display()))?;
    fs::create_dir_all(environment_dir)
        .into_diagnostic()
        .wrap_err_with(|| format!("create {}", environment_dir.display()))
}

/// Install `name` into the existing, empty `environment_dir` and return
/// its executable.
async fn install_runtime(
    config: Config,
    environment_dir: &Path,
    global_virtual_store_dir: Option<PathBuf>,
    name: &str,
    version_spec: &str,
) -> miette::Result<PathBuf> {
    let trusted_root =
        global_virtual_store_dir.clone().unwrap_or_else(|| environment_dir.to_path_buf());
    let install_config =
        Config::leak(hardened_install_config(config, environment_dir, global_virtual_store_dir));
    let state = State::init(environment_dir.join("package.json"), install_config, false)
        .wrap_err("initialize the managed runtime environment")?;
    add_package::<SilentReporter, _>(
        state,
        &format!("{name}@runtime:{version_spec}"),
        RangeSpecStyle::Patch,
        None,
        false,
        install_config.supported_architectures.clone(),
        [DependencyGroup::Prod],
    )
    .await
    .wrap_err("install the managed runtime")?;

    managed_runtime_bin(environment_dir, name, &trusted_root)
        .ok_or_else(|| {
            let trusted_root_display = trusted_root.display();
            miette::miette!("the installed {name} executable is not under {trusted_root_display}")
        })
}
