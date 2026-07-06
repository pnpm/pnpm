//! Install a specific pnpm version into the shared global virtual store
//! for `pnpm with`.
//!
//! The engine lands in `<store>/links/...` (shared across `pnpm with`
//! invocations and, unlike `self-update`, not registered in the global
//! packages directory, so `pnpm ls -g` does not see it), its registry
//! signature is verified on a genuine download, native target installs have
//! their platform binary linked, and the package bins are linked into a
//! `bin/` directory the caller prepends to `PATH`.

use miette::{Context, IntoDiagnostic};
use pacquet_cmd_shim::{Host as CmdShimHost, PackageBinSource, link_bins_of_packages};
use pacquet_config::Config;
use pacquet_graph_hasher::{detect_node_major, engine_name};
use pacquet_lockfile::{EnvLockfile, PackageKey};
use pacquet_package_manager::{AllowBuildPolicy, VirtualStoreLayout};
use pacquet_reporter::Reporter;
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::{
    cli_args::self_update::{
        install_pnpm::{
            PNPM_ALLOW_BUILDS, link_exe_platform_binary, package_dir, pnpm_package_to_install,
            run_install,
        },
        verify_engine::verify_pnpm_engine_identity,
    },
    config_deps,
};

/// Install the pnpm engine for `version` into the global virtual store and
/// return the directory holding the linked `pnpm` binary.
///
/// `env_root` is where the package-manager env lockfile (the resolved
/// `pnpm` + `@pnpm/exe` closure) is written, under the pnpm home
/// directory. `spec` is the user's bare specifier (a version, range,
/// or dist-tag) and `version` the exact version it resolved to.
pub(crate) async fn install_pnpm_to_store<Reporter: self::Reporter + 'static>(
    config: &'static Config,
    env_root: &Path,
    spec: &str,
    version: &str,
) -> miette::Result<PathBuf> {
    fs::create_dir_all(env_root).into_diagnostic().wrap_err_with(|| {
        format!("create the package-manager env directory at {}", env_root.display())
    })?;
    // Resolve the package-manager closure into the env lockfile (a no-op
    // when this spec+version is already recorded there).
    config_deps::sync_package_manager_dependencies(config, env_root, spec, version, false).await?;
    let env = EnvLockfile::read(env_root)
        .map_err(miette::Report::new)
        .wrap_err("read the package-manager env lockfile")?
        .ok_or_else(|| {
            miette::miette!("the package-manager env lockfile is missing after resolution")
        })?;
    install_pnpm_from_env::<Reporter>(config, &env, version).await
}

pub(crate) async fn install_pnpm_from_env<Reporter: self::Reporter + 'static>(
    config: &'static Config,
    env: &EnvLockfile,
    version: &str,
) -> miette::Result<PathBuf> {
    let package = pnpm_package_to_install(version);
    let package_name = package.name;
    // Cache hit: when the engine already sits in its GVS slot, skip both
    // the signature check and the install — short-circuit on the engine's
    // `package.json` already existing. The slot is computed
    // with the same hashing the install pipeline uses, so a stale or wrong
    // computation merely misses the cache (the idempotent install below
    // then re-derives the slot from the install's own symlink).
    if let Some(slot) = compute_engine_slot(config, env, package_name, version) {
        let pkg_dir = package_dir(&slot, package_name);
        let bin_dir = slot.join("bin");
        if pkg_dir.join("package.json").exists() {
            if package.links_native_binary {
                link_exe_platform_binary(&slot, package_name)?;
            }
            if !bin_dir.exists() {
                link_bins(&pkg_dir, &bin_dir)?;
            }
            return Ok(bin_dir);
        }
    }

    // Genuine download: verify the engine's registry signature before
    // installing or executing it.
    verify_pnpm_engine_identity(env, version, config)
        .await
        .map_err(miette::Report::new)
        .wrap_err("verify the pnpm engine identity")?;

    // Install into a throwaway directory with the global virtual store
    // enabled, so the engine itself materializes in `<store>/links/...`
    // and the temp directory holds only symlinks into it.
    let tmp_install_dir =
        config.store_dir.tmp().join(format!("pnpm-with-{version}-{}", unique_suffix()));
    fs::create_dir_all(&tmp_install_dir)
        .into_diagnostic()
        .wrap_err("create the temporary pnpm install directory")?;
    let slot = Box::pin(run_install::<Reporter>(
        config,
        &tmp_install_dir,
        package_name,
        version,
        config.supported_architectures.clone(),
        true,
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

/// The global-virtual-store slot the selected engine wrapper resolves to, or `None`
/// when it can't be derived (e.g. the engine snapshot is missing or the
/// allow-build policy fails to compile). Drives only the cache-hit
/// short-circuit, so `None` is a safe "treat as a miss".
fn compute_engine_slot(
    config: &Config,
    env: &EnvLockfile,
    package_name: &str,
    version: &str,
) -> Option<PathBuf> {
    let wanted: PackageKey = format!("{package_name}@{version}").parse().ok()?;
    let key = env.snapshots.keys().find(|key| key.without_peer() == wanted)?.clone();

    let mut cfg = config.clone();
    cfg.enable_global_virtual_store = true;
    cfg.global_virtual_store_dir = config.store_dir.links();
    cfg.allow_builds.clear();
    for name in PNPM_ALLOW_BUILDS {
        cfg.allow_builds.insert(name.to_string(), true);
    }
    let policy = AllowBuildPolicy::from_config(&cfg).ok()?;
    let engine = detect_node_major().map(|major| engine_name(major, None, None));
    let layout = VirtualStoreLayout::new(
        &cfg,
        engine.as_deref(),
        Some(&env.snapshots),
        Some(&env.packages),
        Some(&policy),
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
        .wrap_err_with(|| format!("resolve the installed pnpm at {}", link.display()))?;
    slot_from_package_dir(&real, package_name)
        .ok_or_else(|| miette::miette!("could not locate the pnpm global-virtual-store slot"))
}

pub(super) fn slot_from_package_dir(package_dir: &Path, package_name: &str) -> Option<PathBuf> {
    let mut slot = package_dir;
    for _ in package_name.split('/') {
        slot = slot.parent()?;
    }
    slot.parent().map(Path::to_path_buf)
}

/// Link `pnpm`'s declared bins into `bin_dir` after the engine install.
fn link_bins(pkg_dir: &Path, bin_dir: &Path) -> miette::Result<()> {
    let manifest_path = pkg_dir.join("package.json");
    let text = fs::read_to_string(&manifest_path)
        .into_diagnostic()
        .wrap_err_with(|| format!("read {}", manifest_path.display()))?;
    let manifest: Value = serde_json::from_str(&text)
        .into_diagnostic()
        .wrap_err_with(|| format!("parse {}", manifest_path.display()))?;
    let source = PackageBinSource::new(pkg_dir.to_path_buf(), Arc::new(manifest));
    link_bins_of_packages::<CmdShimHost>(&[source], bin_dir)
        .map_err(miette::Report::new)
        .wrap_err("link the pnpm bins")
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
