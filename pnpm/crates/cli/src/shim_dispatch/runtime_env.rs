//! Materialization of pinned runtimes into the trusted global virtual
//! store, under configuration a project cannot influence.

use crate::{State, cli_args::add::add_package};
use miette::{Context, IntoDiagnostic};
use pnpm_config::{Config, Host};
use pnpm_crypto_hash::create_hex_hash;
use pnpm_fs::DirLock;
use pnpm_package_manifest::DependencyGroup;
use pnpm_registry::RangeSpecStyle;
use pnpm_reporter::SilentReporter;
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

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
pub(super) fn hardened_install_config(
    config: Config,
    environment_dir: &Path,
    global_virtual_store_dir: PathBuf,
) -> Config {
    let mut install_config = config;
    install_config.modules_dir = environment_dir.join("node_modules");
    install_config.virtual_store_dir = environment_dir.join("node_modules").join(".pnpm");
    install_config.enable_global_virtual_store = true;
    install_config.global_virtual_store_dir = global_virtual_store_dir;
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

/// Materialize a runtime into the configured global virtual store and return
/// its real executable. The small environment under pnpm's state directory
/// contains only the lockfile and symlinks required to address the GVS slot;
/// project `node_modules` is never consulted.
pub(crate) async fn materialize_runtime(
    state_dir: &Path,
    name: String,
    version_spec: String,
) -> miette::Result<PathBuf> {
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
        return Ok(bin);
    }

    const WAIT: Duration = Duration::from_mins(5);
    const ABANDONED_AFTER: Duration = Duration::from_mins(30);
    let lock_path = environments_dir.join(format!("{key}.lock"));
    let _lock = DirLock::acquire(lock_path.clone(), WAIT, ABANDONED_AFTER)
        .into_diagnostic()
        .wrap_err_with(|| format!("lock the managed runtime at {}", lock_path.display()))?;
    if let Some(bin) = managed_runtime_bin(&environment_dir, &name, &global_virtual_store_dir) {
        return Ok(bin);
    }

    remove_dir_if_not_symlink(&environment_dir)
        .into_diagnostic()
        .wrap_err_with(|| format!("reset {}", environment_dir.display()))?;
    fs::create_dir_all(&environment_dir)
        .into_diagnostic()
        .wrap_err_with(|| format!("create {}", environment_dir.display()))?;

    let install_config =
        Config::leak(hardened_install_config(config, &environment_dir, global_virtual_store_dir));
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
    .wrap_err("install the managed runtime into the global virtual store")?;

    let global_virtual_store_dir_display = install_config.global_virtual_store_dir.display();
    managed_runtime_bin(&environment_dir, &name, &install_config.global_virtual_store_dir).ok_or_else(
        || {
            miette::miette!(
                "the installed {name} executable is not in the global virtual store at {global_virtual_store_dir_display}"
            )
        },
    )
}

pub(super) fn managed_runtime_bin(
    environment_dir: &Path,
    name: &str,
    global_virtual_store_dir: &Path,
) -> Option<PathBuf> {
    let package_dir = dunce::canonicalize(environment_dir.join("node_modules").join(name)).ok()?;
    let store_dir = dunce::canonicalize(global_virtual_store_dir).ok()?;
    if !package_dir.starts_with(&store_dir) {
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
