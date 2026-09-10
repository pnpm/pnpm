//! Global package install command handlers (`add -g`, `update -g`,
//! `remove -g`).
//!
//! Each space-separated CLI param is its own isolated install group (a
//! comma splits a group; local paths / URLs are kept whole). A group
//! installs into a fresh directory under the global packages dir, then a
//! hash symlink and the global bins are pointed at it.

pub use builds::approve_global_builds;
pub use remove::handle_global_remove;
pub use selectors::{has_pnpm_cli_dependency, selects_pnpm_cli};

mod activation;

use self::activation::{
    ArtifactCleanupError, FsRename, activate_global_install_with_extra_bin_names,
    get_actual_bin_names, hash_linked_packages, replace_global_bin_slots,
};
use crate::{
    State,
    cli_args::{
        add::{add_packages, apply_allow_build},
        approve_builds::{
            ApproveBuildsArgs, clear_decided_ignored_builds, write_approval_settings,
        },
        global_bin_lock::acquire_global_bin_lock,
        ignored_builds::{IgnoredBuildsScan, get_automatically_ignored_builds},
        rebuild::run_rebuild,
        shim::{
            record_package_manager_shims, virtual_shim_bins_to_restore, virtual_shim_owner,
            virtual_shim_restoration_owners,
        },
    },
    engine_pm::selector::tool_install_selector,
    shim_dispatch::{ShimTarget, install_native_shim, migrate_legacy_shims, remove_native_shim},
};

use builds::prompt_approve_global_builds;
use cleanup::discard_install_dir_on_error;
use derive_more::{Display, Error};
use install::{
    GlobalInstallTarget, GroupActivation, GroupInstall, global_group_config, is_plain_version_spec,
    pins_for_downgrades, run_group_install,
};
use miette::{Context, Diagnostic, IntoDiagnostic};
use node_semver::Version;
use pnpm_cmd_shim::{
    Host as CmdShimHost, LinkBinsOptions, PackageBinSource, choose_bins,
    link_bins_of_packages_with_excludes, remove_bin as remove_cmd_shim,
};
use pnpm_config::{
    CatalogMode, Config, GlobalShims, WorkspaceSettings, check_global_bin_dir, decided_allow_builds,
};
use pnpm_fs::{is_subdir, lexical_normalize, remove_symlink_dir, symlink_dir};
use pnpm_global::{
    GlobalPackageInfo, check_global_bin_conflicts, clean_orphaned_install_dirs,
    create_global_cache_key, create_install_dir, find_global_package, get_hash_link,
    get_installed_bin_names, installed_versions, read_direct_dependencies, read_installed_packages,
    scan_global_packages,
};
use pnpm_lockfile::{ImporterDepVersion, Lockfile};
use pnpm_package_is_installable::SupportedArchitectures;
use pnpm_package_manifest::{DependencyGroup, safe_read_package_json_from_dir};
use pnpm_package_name::is_valid_old_npm_package_name;
use pnpm_registry::RangeSpecStyle;
use pnpm_reporter::{GlobalLog, LogEvent, LogLevel, Reporter};
use pnpm_resolving_parse_wanted_dependency::parse_wanted_dependency;

use remove::{
    FsGlobalRemoval, GlobalPackageBinSnapshot, cleanup_replaced_global_installs,
    collect_existing_global_installs, snapshot_global_package,
};
use selectors::{
    groups_matching_params, infer_local_package_alias, replacement_aliases,
    should_replace_existing_package, split_into_groups, tool_install_selectors, update_selectors,
};

use shims::{
    ReplacedGlobalBinPlan, bin_names_of_other_groups, check_virtual_shim_conflicts,
    link_global_bins, plan_replaced_global_bins, restore_virtual_shims, unprotected_bin_names,
    virtual_shims_to_restore,
};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    fs, io,
    path::{Path, PathBuf},
};

/// Errors specific to global package management, carrying the
/// `ERR_PNPM_`-prefixed codes.
#[derive(Debug, Display, Error, Diagnostic)]
pub enum GlobalError {
    #[display("Unable to find the global bin directory")]
    #[diagnostic(
        code(ERR_PNPM_NO_GLOBAL_BIN_DIR),
        help(
            r#"Run "pnpm setup" to create it automatically, or set the global-bin-dir setting, or the PNPM_HOME env variable. The global bin directory should be in the PATH."#
        )
    )]
    NoGlobalBinDir,

    /// The global packages directory could not be resolved (no `PNPM_HOME`
    /// and no determinable data dir), matching pnpm's `prefix` handler.
    #[display("The global package directory could not be resolved.")]
    #[diagnostic(code(ERR_PNPM_MISSING_GLOBAL_PACKAGE_DIR))]
    MissingGlobalPackageDir,

    #[display(r#"Use the "pnpm self-update" command to install or update pnpm"#)]
    #[diagnostic(code(ERR_PNPM_GLOBAL_PNPM_INSTALL))]
    GlobalPnpmInstall,

    #[display("Cannot remove '{param}': not found in global packages")]
    #[diagnostic(code(ERR_PNPM_GLOBAL_PKG_NOT_FOUND))]
    PkgNotFound { param: String },

    #[display(r#"Invalid package name "{name}"."#)]
    #[diagnostic(code(ERR_PNPM_INVALID_PACKAGE_NAME))]
    InvalidPackageName { name: String },

    #[display(
        r#"Cannot install {packages}: binary "{bin}" is reserved by the project-aware shim for "{shim_package}""#
    )]
    #[diagnostic(
        code(ERR_PNPM_GLOBAL_BIN_CONFLICT),
        help(r#"Remove the shim first with "pnpm shim rm {shim_package}"."#)
    )]
    VirtualShimBinConflict { packages: String, bin: String, shim_package: String },
}

/// Resolve the global packages and global bin directories, erroring with
/// `NO_GLOBAL_BIN_DIR` when the pnpm home can't be determined.
fn global_dirs(config: &Config) -> Result<(PathBuf, PathBuf), GlobalError> {
    let bin = config.global_bin.clone().ok_or(GlobalError::NoGlobalBinDir)?;
    let pkg_dir = config.global_pkg_dir.clone().ok_or(GlobalError::NoGlobalBinDir)?;
    Ok((pkg_dir, bin))
}

/// Validate the global bin dir is on `PATH` and writable, required for
/// mutating commands. Mirrors pnpm's config reader: the directory is
/// created first, so a fresh `PNPM_HOME` whose `bin` is already on `PATH`
/// but not yet on disk works on the first global command.
fn check_bin_dir(global_bin_dir: &Path) -> miette::Result<()> {
    fs::create_dir_all(global_bin_dir).map_err(|error| {
        let bin_dir = global_bin_dir.display();
        miette::miette!("failed to create the global bin directory {bin_dir}: {error}")
    })?;
    check_global_bin_dir(global_bin_dir, std::env::var("PATH").ok().as_deref(), true)
        .map_err(miette::Report::new)
}

/// `pnpm add -g`. Installs each group, links its bins into the global bin
/// directory, and records a cache-keyed hash symlink.
pub async fn handle_global_add<Reporter: self::Reporter + 'static>(
    base_config: &'static Config,
    params: &[String],
    range_spec_style: RangeSpecStyle,
    supported_architectures: Option<SupportedArchitectures>,
    allow_build: &[String],
    cwd: &Path,
) -> miette::Result<()> {
    // Both of the rules below apply to what actually gets installed, so
    // they run on the tokens a comma-separated group splits into rather
    // than on the group: `pnpm,lodash` is a request to install pnpm.
    let groups = split_into_groups(params, cwd);
    // Each selector is read as its package name, so versioned forms like
    // `pnpm@9` or `@pnpm/exe@1` can't bypass the self-install guard.
    if selects_pnpm_cli(groups.iter().flatten()) {
        return Err(GlobalError::GlobalPnpmInstall.into());
    }
    let groups = tool_install_selectors(groups);

    let (global_pkg_dir, global_bin_dir) = global_dirs(base_config)?;
    check_bin_dir(&global_bin_dir)?;
    fs::create_dir_all(&global_pkg_dir)
        .into_diagnostic()
        .wrap_err("create the global packages directory")?;
    clean_orphaned_install_dirs(&global_pkg_dir);

    let target = GlobalInstallTarget {
        base_config,
        global_pkg_dir: &global_pkg_dir,
        global_bin_dir: &global_bin_dir,
    };
    for group in groups {
        target
            .add_group::<Reporter>(
                &group,
                range_spec_style,
                supported_architectures.clone(),
                allow_build,
            )
            .await?;
    }
    Ok(())
}

/// `pnpm update -g`. Reinstalls each matching group (within its existing
/// range, or to `--latest`), then swaps its hash symlink to the new dir.
pub async fn handle_global_update<Reporter: self::Reporter + 'static>(
    base_config: &'static Config,
    params: &[String],
    selected_hashes: Option<&HashSet<String>>,
    latest: bool,
    range_spec_style: RangeSpecStyle,
    supported_architectures: Option<SupportedArchitectures>,
) -> miette::Result<()> {
    let (global_pkg_dir, global_bin_dir) = global_dirs(base_config)?;
    check_bin_dir(&global_bin_dir)?;
    clean_orphaned_install_dirs(&global_pkg_dir);

    let scanned =
        scan_global_packages(&global_pkg_dir).into_diagnostic().wrap_err("scan global packages")?;
    if scanned.is_empty() {
        println!("No global packages found");
        return Ok(());
    }
    // `pnpm self-update` owns the pnpm CLI's global install: it is what points
    // the pnpm home's bins at a release. Reinstalling that group here would
    // resolve pnpm from the `latest` dist-tag and relink the bins, silently
    // rolling the running pnpm back to whatever `latest` points at.
    let all: Vec<GlobalPackageInfo> =
        scanned.into_iter().filter(|pkg| !has_pnpm_cli_dependency(pkg)).collect();
    if all.is_empty() {
        println!(r#"No global packages to update. Run "pnpm self-update" to update pnpm itself."#);
        return Ok(());
    }
    let Some(mut to_update) = groups_matching_params(all, params) else {
        return Ok(());
    };
    if let Some(selected_hashes) = selected_hashes {
        to_update.retain(|pkg| selected_hashes.contains(&pkg.hash));
    }

    let target = GlobalInstallTarget {
        base_config,
        global_pkg_dir: &global_pkg_dir,
        global_bin_dir: &global_bin_dir,
    };
    for pkg in &to_update {
        target
            .update_group::<Reporter>(
                pkg,
                latest,
                range_spec_style,
                supported_architectures.clone(),
            )
            .await?;
    }
    Ok(())
}

/// Surface a non-fatal problem on the `pnpm:global` channel, matching
/// the TypeScript CLI's `globalWarn`.
fn warn_global<Reporter: self::Reporter>(message: &str) {
    Reporter::emit(&LogEvent::Global(GlobalLog {
        level: LogLevel::Warn,
        message: message.to_string(),
    }));
}

/// Build the registry map (`{ default, ...scoped }`) hashed into the
/// global cache key.
fn registries_with_default(config: &Config) -> Vec<(String, String)> {
    let mut registries = vec![("default".to_string(), config.registry.clone())];
    registries
        .extend(config.registries_by_scope.iter().map(|(key, value)| (key.clone(), value.clone())));
    registries
}

#[cfg(test)]
mod tests;

mod install;

mod remove;

mod shims;

mod selectors;

mod builds;

mod cleanup;

mod groups;
