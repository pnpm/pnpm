use super::{SelfUpdateError, install_pnpm};
use miette::{Context, IntoDiagnostic};
use pnpm_cmd_shim::{Host as CmdShimHost, LinkBinsOptions, link_bins_of_packages_with_excludes};
use pnpm_config::Config;
use pnpm_fs::{force_symlink_dir, remove_symlink_dir};
use pnpm_global::{
    create_global_cache_key, get_hash_link, read_installed_packages, scan_global_packages,
};
use std::{collections::HashSet, fs, io, path::Path};

/// A standalone pnpm executable copied into a directory on PATH. pnpm links
/// itself there as `pnpm` and `pnpm.cmd` shims, and Windows prefers a
/// `pnpm.exe` over `pnpm.cmd`, so a leftover one keeps running the old
/// version after an update.
const STANDALONE_EXECUTABLE: &str = "pnpm.exe";
const RETIRED_EXECUTABLE_SUFFIX: &str = ".retired";

/// Link the installed engine's bins into the global bin directory and
/// record its cache-keyed hash symlink (so `pnpm ls -g` and `store prune`
/// see it), replacing the group of the engine it switched from.
pub(super) fn link_into_global_bin(
    config: &Config,
    installed: &install_pnpm::InstallPnpmResult,
    version: &str,
) -> miette::Result<()> {
    let global_bin = config.global_bin.clone().ok_or(SelfUpdateError::NoGlobalDir)?;
    let global_pkg_dir = config.global_pkg_dir.clone().ok_or(SelfUpdateError::NoGlobalDir)?;
    let _global_bin_lock = crate::cli_args::global_bin_lock::acquire_global_bin_lock(&global_bin)?;

    if cfg!(windows) {
        retire_standalone_executable(&global_bin)?;
    }
    refresh_global_shims(&global_bin, installed, version)?;
    link_pnpm_bins(installed, &global_bin)?;

    let aliases = vec![installed.package_name.to_string()];
    let cache_hash = create_global_cache_key(&aliases, &registries_for_cache_key(config));
    let hash_link = get_hash_link(&global_pkg_dir, &cache_hash);
    force_symlink_dir(&installed.install_dir, &hash_link)
        .into_diagnostic()
        .wrap_err("link the global pnpm install directory")?;
    unlink_replaced_engine_groups(&global_pkg_dir, &cache_hash)
}

pub(super) const LEGACY_HOME_DIR_WARNING: &str = "Detected a pnpm v10 installation layout at \
    PNPM_HOME. The pnpm executable there was replaced with shims of the new version, but pnpm \
    expects bins in PNPM_HOME/bin. Run \"pnpm setup\" to add PNPM_HOME/bin to your PATH.";

/// pnpm v10 put the standalone executable straight into `PNPM_HOME` and that
/// directory on PATH, while an update links into `PNPM_HOME/bin`. Replace
/// that executable with shims of the updated pnpm, so PATH reaches the
/// update. Returns whether `pnpm_home_dir` had that layout.
pub(super) fn link_into_legacy_home_dir(
    pnpm_home_dir: &Path,
    installed: &install_pnpm::InstallPnpmResult,
) -> miette::Result<bool> {
    if !retire_standalone_executable(pnpm_home_dir)? {
        return Ok(false);
    }
    link_pnpm_bins(installed, pnpm_home_dir)?;
    Ok(true)
}

fn link_pnpm_bins(
    installed: &install_pnpm::InstallPnpmResult,
    bin_dir: &Path,
) -> miette::Result<()> {
    let pkgs = read_installed_packages(&installed.install_dir);
    link_bins_of_packages_with_excludes::<CmdShimHost>(
        &pkgs,
        bin_dir,
        &HashSet::new(),
        &LinkBinsOptions::default(),
    )
    .map_err(miette::Report::new)
    .wrap_err_with(|| format!("link the updated pnpm bins into {}", bin_dir.display()))
}

/// Move a [`STANDALONE_EXECUTABLE`] in `dir` out of the way. Windows refuses
/// to delete an executable while it runs but lets it be renamed, so it is
/// renamed first; one that is still running is removed by the next update.
/// A native shim named `pnpm` is left to [`refresh_global_shims`]. Returns
/// whether there was an executable to retire.
pub(super) fn retire_standalone_executable(dir: &Path) -> miette::Result<bool> {
    remove_retired_executables(dir)
        .into_diagnostic()
        .wrap_err_with(|| format!("remove retired pnpm executables from {}", dir.display()))?;
    let executable = dir.join(STANDALONE_EXECUTABLE);
    if !executable.is_file()
        || crate::shim_dispatch::native_shim_target(dir, "pnpm").into_diagnostic()?.is_some()
    {
        return Ok(false);
    }
    let retired = dir.join(format!(
        ".{STANDALONE_EXECUTABLE}.{}{RETIRED_EXECUTABLE_SUFFIX}",
        std::process::id(),
    ));
    fs::rename(&executable, &retired)
        .and_then(|()| remove_retired_executable(&retired))
        .into_diagnostic()
        .wrap_err_with(|| {
            format!("move the old pnpm executable at {} aside", executable.display())
        })?;
    Ok(true)
}

fn remove_retired_executables(dir: &Path) -> io::Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    for entry in entries {
        let entry = entry?;
        let is_retired = entry
            .file_name()
            .to_str()
            .is_some_and(|name| {
                name.starts_with(&format!(".{STANDALONE_EXECUTABLE}."))
                    && name.ends_with(RETIRED_EXECUTABLE_SUFFIX)
            });
        if is_retired {
            remove_retired_executable(&entry.path())?;
        }
    }
    Ok(())
}

/// Windows reports a retired executable that is still running as access
/// denied.
fn remove_retired_executable(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied,
            ) =>
        {
            Ok(())
        }
        result => result,
    }
}

/// Unlink every other group that holds nothing but a pnpm engine. The engine
/// switched from may be installed under the other alias, whose group the new
/// hash link does not overwrite (pnpm/pnpm#14709). The install directories are
/// left to [`pnpm_global::clean_orphaned_install_dirs`], as the running pnpm
/// may still execute from one of them.
fn unlink_replaced_engine_groups(global_pkg_dir: &Path, kept_hash: &str) -> miette::Result<()> {
    let groups =
        scan_global_packages(global_pkg_dir).into_diagnostic().wrap_err("scan global packages")?;
    for group in groups {
        if group.hash == kept_hash
            || !group.dependencies
                .iter()
                .all(|(alias, _)| install_pnpm::ENGINE_ALIASES.contains(&alias.as_str()))
        {
            continue;
        }
        let hash_link = get_hash_link(global_pkg_dir, &group.hash);
        remove_symlink_dir(&hash_link)
            .into_diagnostic()
            .wrap_err_with(|| format!("unlink the replaced pnpm at {}", hash_link.display()))?;
    }
    Ok(())
}

pub(super) fn refresh_global_shims(
    global_bin: &Path,
    installed: &install_pnpm::InstallPnpmResult,
    version: &str,
) -> miette::Result<()> {
    // Named native shims first shipped in pnpm 12.3. An older engine
    // cannot interpret their sidecars, so leave the working shim engine
    // in place when downgrading.
    if !node_semver::Version::parse(version)
        .is_ok_and(|version| (version.major, version.minor) >= (12, 3))
    {
        return Ok(());
    }
    let executable =
        install_pnpm::pnpm_executable_path(&installed.install_dir, installed.package_name);
    crate::shim_dispatch::refresh_native_shims(&executable, global_bin)
        .into_diagnostic()
        .wrap_err("refresh the global shims")
}

/// Build the registry map (`{ default, ...scoped }`) hashed into the
/// global cache key, from the trusted package-manager bootstrap registries
/// — never the repo-controlled project registries, so a project `.npmrc`
/// can't change the hash-symlink name (which would create duplicate global
/// `pnpm` groups that `find_global_package` resolves non-deterministically).
fn registries_for_cache_key(config: &Config) -> Vec<(String, String)> {
    let bootstrap = &config.package_manager_bootstrap;
    let mut registries = vec![("default".to_string(), bootstrap.registry.clone())];
    registries.extend(
        bootstrap.registries
            .iter()
            .map(|(key, value)| (key.clone(), value.clone())),
    );
    registries
}
