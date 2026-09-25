use super::{SelfUpdateError, install_pnpm};
use miette::{Context, IntoDiagnostic};
use pnpm_cmd_shim::{Host as CmdShimHost, LinkBinsOptions, link_bins_of_packages_with_excludes};
use pnpm_config::Config;
use pnpm_fs::{force_symlink_dir, remove_symlink_dir};
use pnpm_global::{
    create_global_cache_key, get_hash_link, read_installed_packages, scan_global_packages,
};
use std::{collections::HashSet, path::Path};

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

    refresh_global_shims(&global_bin, installed, version)?;

    let pkgs = read_installed_packages(&installed.install_dir);
    link_bins_of_packages_with_excludes::<CmdShimHost>(
        &pkgs,
        &global_bin,
        &HashSet::new(),
        &LinkBinsOptions::default(),
    )
    .map_err(miette::Report::new)
    .wrap_err("link the updated pnpm bins")?;

    let aliases = vec![installed.package_name.to_string()];
    let cache_hash = create_global_cache_key(&aliases, &registries_for_cache_key(config));
    let hash_link = get_hash_link(&global_pkg_dir, &cache_hash);
    force_symlink_dir(&installed.install_dir, &hash_link)
        .into_diagnostic()
        .wrap_err("link the global pnpm install directory")?;
    unlink_replaced_engine_groups(&global_pkg_dir, &cache_hash)
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
