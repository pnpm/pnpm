//! Materialization of pinned runtimes into the trusted global virtual
//! store, under configuration a project cannot influence.

use crate::{State, cli_args::add::add_package};
use miette::{Context, IntoDiagnostic};
use pacquet_config::{Config, Host, default_state_dir};
use pacquet_crypto_hash::create_hex_hash;
use pacquet_fs::DirLock;
use pacquet_package_manifest::DependencyGroup;
use pacquet_registry::RangeSpecStyle;
use pacquet_reporter::SilentReporter;
use serde_json::Value;
use std::{
    fs,
    io::{self, Read as _},
    path::{Path, PathBuf},
    time::Duration,
};

pub(super) const RUNTIME_ENVS_DIR_NAME: &str = "global-shim-runtimes";
const RUNTIME_LAUNCHERS_DIR_NAME: &str = "launchers-v1";
const RUNTIME_LAUNCHER_SCHEMA: u8 = 1;
const MAX_RUNTIME_LAUNCHER_SIZE: u64 = 16 * 1024;

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RuntimeLauncher {
    schema: u8,
    launcher_key: String,
    environment_key: String,
    global_virtual_store_dir: PathBuf,
}

/// Resolve a warm runtime through machine-local provenance without loading the
/// package-manager configuration. The record is only an index: a hit still
/// proves the exact opaque runtime identity, derives the environment directory
/// from collision-resistant keys, and applies [`managed_runtime_bin`]'s store,
/// package-name, manifest-bin, and containment checks before returning code to
/// execute. Every read or validation failure is a cache miss.
pub(super) fn cached_runtime_bin(name: &str, version_spec: &str) -> Option<PathBuf> {
    let environments_dir = default_state_dir::<Host>()?.join(RUNTIME_ENVS_DIR_NAME);
    cached_runtime_bin_at(&environments_dir, name, version_spec)
}

fn cached_runtime_bin_at(
    environments_dir: &Path,
    name: &str,
    version_spec: &str,
) -> Option<PathBuf> {
    let launcher_key = runtime_launcher_key(name, version_spec);
    let launcher_path = runtime_launcher_path(environments_dir, &launcher_key);
    let bytes = read_runtime_launcher(&launcher_path)?;
    let launcher: RuntimeLauncher = serde_json::from_slice(&bytes).ok()?;
    if launcher.schema != RUNTIME_LAUNCHER_SCHEMA
        || launcher.launcher_key != launcher_key
        || !launcher.global_virtual_store_dir.is_absolute()
        || !is_hex_hash(&launcher.environment_key)
        || launcher.environment_key
            != runtime_environment_key(name, version_spec, &launcher.global_virtual_store_dir)
    {
        return None;
    }
    managed_runtime_bin(
        &environments_dir.join(launcher.environment_key),
        name,
        &launcher.global_virtual_store_dir,
    )
}

fn read_runtime_launcher(path: &Path) -> Option<Vec<u8>> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;
        options.custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options.open(path).ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || metadata.len() > MAX_RUNTIME_LAUNCHER_SIZE {
        return None;
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_RUNTIME_LAUNCHER_SIZE + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() as u64 <= MAX_RUNTIME_LAUNCHER_SIZE).then_some(bytes)
}

fn runtime_launcher_key(name: &str, version_spec: &str) -> String {
    create_hex_hash(&format!(
        "global-shim-runtime-launcher-v1\0{name}\0{version_spec}\0{}\0{}\0{}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        target_environment(),
    ))
}

fn target_environment() -> &'static str {
    if cfg!(target_env = "musl") {
        "musl"
    } else if cfg!(target_env = "gnu") {
        "gnu"
    } else if cfg!(target_env = "msvc") {
        "msvc"
    } else {
        "other"
    }
}

fn runtime_environment_key(
    name: &str,
    version_spec: &str,
    global_virtual_store_dir: &Path,
) -> String {
    create_hex_hash(&format!(
        "runtime\0{name}\0{version_spec}\0{}",
        global_virtual_store_dir.display(),
    ))
}

fn runtime_launcher_path(environments_dir: &Path, launcher_key: &str) -> PathBuf {
    environments_dir.join(RUNTIME_LAUNCHERS_DIR_NAME).join(format!("{launcher_key}.json"))
}

fn is_hex_hash(value: &str) -> bool {
    value.len() == 64
        && value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn write_runtime_launcher(
    environments_dir: &Path,
    name: &str,
    version_spec: &str,
    environment_key: &str,
    global_virtual_store_dir: &Path,
) -> io::Result<()> {
    if !global_virtual_store_dir.is_absolute()
        || !is_hex_hash(environment_key)
        || environment_key != runtime_environment_key(name, version_spec, global_virtual_store_dir)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "runtime launcher paths must be absolute and content-addressed",
        ));
    }
    let launcher_key = runtime_launcher_key(name, version_spec);
    let launcher = RuntimeLauncher {
        schema: RUNTIME_LAUNCHER_SCHEMA,
        launcher_key: launcher_key.clone(),
        environment_key: environment_key.to_string(),
        global_virtual_store_dir: global_virtual_store_dir.to_path_buf(),
    };
    let bytes = serde_json::to_vec(&launcher).map_err(io::Error::other)?;
    if bytes.len() as u64 > MAX_RUNTIME_LAUNCHER_SIZE {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "runtime launcher is too large"));
    }
    pacquet_fs::write_atomic(&runtime_launcher_path(environments_dir, &launcher_key), &bytes)
}

fn publish_runtime_launcher(
    environments_dir: &Path,
    name: &str,
    version_spec: &str,
    environment_key: &str,
    global_virtual_store_dir: &Path,
) {
    if let Err(error) = write_runtime_launcher(
        environments_dir,
        name,
        version_spec,
        environment_key,
        global_virtual_store_dir,
    ) {
        eprintln!("pnpm: failed to cache the managed runtime launcher: {error}");
    }
}

/// The configuration the managed runtime installs under. The runtime's
/// store, mirrors, and registry selection must not be
/// project-controllable: a repository that redirects `storeDir` could
/// pre-seed a poisoned global-virtual-store slot for the executable a
/// dispatch is about to run. The configuration is therefore anchored
/// inside pnpm's own state dir — the seeded empty workspace manifest
/// stops ancestor discovery, so only the global `config.yaml`, user
/// files, and the environment contribute.
pub(super) fn trusted_runtime_config(environments_dir: &Path) -> miette::Result<Config> {
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
pub(super) async fn materialize_runtime(
    name: String,
    version_spec: String,
) -> miette::Result<PathBuf> {
    let state_dir = default_state_dir::<Host>()
        .ok_or_else(|| miette::miette!("the pnpm state directory could not be resolved"))?;
    let environments_dir = state_dir.join(RUNTIME_ENVS_DIR_NAME);
    let config = trusted_runtime_config(&environments_dir)?;
    let global_virtual_store_dir = config.store_dir.links();
    let key = runtime_environment_key(&name, &version_spec, &global_virtual_store_dir);
    let environment_dir = environments_dir.join(&key);
    if let Some(bin) = managed_runtime_bin(&environment_dir, &name, &global_virtual_store_dir) {
        publish_runtime_launcher(
            &environments_dir,
            &name,
            &version_spec,
            &key,
            &global_virtual_store_dir,
        );
        return Ok(bin);
    }

    const WAIT: Duration = Duration::from_mins(5);
    const ABANDONED_AFTER: Duration = Duration::from_mins(30);
    let lock_path = environments_dir.join(format!("{key}.lock"));
    let _lock = DirLock::acquire(lock_path.clone(), WAIT, ABANDONED_AFTER)
        .into_diagnostic()
        .wrap_err_with(|| format!("lock the managed runtime at {}", lock_path.display()))?;
    if let Some(bin) = managed_runtime_bin(&environment_dir, &name, &global_virtual_store_dir) {
        publish_runtime_launcher(
            &environments_dir,
            &name,
            &version_spec,
            &key,
            &global_virtual_store_dir,
        );
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
    let bin = managed_runtime_bin(&environment_dir, &name, &install_config.global_virtual_store_dir)
        .ok_or_else(|| {
            miette::miette!(
                "the installed {name} executable is not in the global virtual store at {global_virtual_store_dir_display}"
            )
        })?;
    publish_runtime_launcher(
        &environments_dir,
        &name,
        &version_spec,
        &key,
        &install_config.global_virtual_store_dir,
    );
    Ok(bin)
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

#[cfg(test)]
mod tests;
