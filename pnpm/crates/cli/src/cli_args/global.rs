//! Global package install command handlers (`add -g`, `update -g`,
//! `remove -g`).
//!
//! Each space-separated CLI param is its own isolated install group (a
//! comma splits a group; local paths / URLs are kept whole). A group
//! installs into a fresh directory under the global packages dir, then a
//! hash symlink and the global bins are pointed at it.

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
use derive_more::{Display, Error};
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
use pnpm_registry::RangeSpecStyle;
use pnpm_reporter::{GlobalLog, LogEvent, LogLevel, Reporter};
use pnpm_resolving_parse_wanted_dependency::{
    is_valid_old_npm_package_name, parse_wanted_dependency,
};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    fs,
    io::{self, IsTerminal},
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

/// Link `pkgs`' bins into the global bin dir in the shape selected by the
/// `globalShims` record: bins of an enabled providing package become
/// context-aware shims, everything else gets direct shims. The runtime
/// names only count when actually installed through the `runtime:`
/// protocol, so an npm package that happens to be called `node` is not
/// elevated.
fn link_global_bins(
    config: &Config,
    pkgs: &[PackageBinSource],
    dependencies: &[(String, String)],
    global_bin_dir: &Path,
    bins_to_skip: &std::collections::HashSet<String>,
) -> miette::Result<()> {
    // A package manager installed globally opts into project-aware
    // dispatch, so it defers to whatever version a project pins and stays
    // the fallback for projects that pin nothing — the arrangement a
    // globally installed runtime already has. The entry is recorded before
    // the split below, so the bins this very run writes are the
    // dispatching flavor.
    let names = pkgs.iter().filter_map(|pkg| pkg.manifest.get("name")?.as_str());
    let newly_enabled = record_package_manager_shims(config, names)?;

    let (direct, context_aware): (Vec<_>, Vec<_>) = pkgs.iter().cloned().partition(|pkg| {
        let name = pkg.manifest.get("name").and_then(serde_json::Value::as_str);
        !name.is_some_and(|name| {
            (config.global_shims.is_enabled(name) || newly_enabled.contains(name))
                && (!pnpm_package_manifest::is_runtime_alias(name)
                    || dependencies
                        .iter()
                        .any(|(alias, spec)| alias == name && spec.starts_with("runtime:")))
        })
    });
    migrate_legacy_shims(global_bin_dir).into_diagnostic().wrap_err("migrate the global shims")?;
    if !direct.is_empty() {
        // A slot turning direct again (its package's shim switched off)
        // must not keep the native shim, which would shadow the direct
        // shim on Windows and hold a stale target everywhere.
        for (command, _) in choose_bins::<CmdShimHost>(&direct, bins_to_skip) {
            remove_native_shim(global_bin_dir, &command.name)
                .into_diagnostic()
                .wrap_err_with(|| format!("remove the stale {} shim", command.name))?;
        }
        link_bins_of_packages_with_excludes::<CmdShimHost>(
            &direct,
            global_bin_dir,
            bins_to_skip,
            &LinkBinsOptions::default(),
        )
        .map_err(miette::Report::new)
        .wrap_err("link direct global package bins")?;
    }
    for (command, _) in choose_bins::<CmdShimHost>(&context_aware, bins_to_skip) {
        install_native_shim(global_bin_dir, &command.name, &ShimTarget::Installed(command.path))
            .into_diagnostic()
            .wrap_err_with(|| format!("install the {} shim", command.name))?;
    }
    Ok(())
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
    // A tool name becomes the selector that installs the tool itself,
    // which the ordinary pipeline then handles — so the result stays a
    // normal global install that `pnpm ls -g` and `pnpm remove -g` see.
    let groups: Vec<Vec<String>> = groups
        .into_iter()
        .map(|group| {
            group.into_iter().map(|token| tool_install_selector(&token).unwrap_or(token)).collect()
        })
        .collect();

    let (global_pkg_dir, global_bin_dir) = global_dirs(base_config)?;
    check_bin_dir(&global_bin_dir)?;
    fs::create_dir_all(&global_pkg_dir)
        .into_diagnostic()
        .wrap_err("create the global packages directory")?;
    clean_orphaned_install_dirs(&global_pkg_dir);

    for group in groups {
        let install_dir = create_install_dir(&global_pkg_dir)
            .into_diagnostic()
            .wrap_err("create global install dir")?;
        let config = Box::pin(run_group_install::<Reporter>(GroupInstall {
            base_config,
            global_pkg_dir: &global_pkg_dir,
            install_dir: &install_dir,
            selectors: &group,
            range_spec_style,
            supported_architectures: supported_architectures.clone(),
            allow_build,
            lockfile_only: false,
        }))
        .await?;

        let pkgs = read_installed_packages(&install_dir);
        let dependencies = read_direct_dependencies(&install_dir);
        let aliases = dependencies.iter().map(|(alias, _)| alias.clone()).collect::<Vec<_>>();
        let aliases_to_replace = replacement_aliases(&aliases);
        let _global_bin_lock = match acquire_global_bin_lock(&global_bin_dir) {
            Ok(lock) => lock,
            Err(error) => {
                let _ = fs::remove_dir_all(&install_dir);
                return Err(error);
            }
        };

        if let Err(error) = check_virtual_shim_conflicts(&pkgs, &global_bin_dir) {
            let _ = fs::remove_dir_all(&install_dir);
            return Err(error);
        }

        let bins_to_skip = match check_global_bin_conflicts(
            &global_pkg_dir,
            &global_bin_dir,
            &pkgs,
            |existing: &GlobalPackageInfo| {
                should_replace_existing_package(existing, &aliases, &aliases_to_replace)
            },
        ) {
            Ok(skip) => skip,
            Err(error) => {
                let _ = fs::remove_dir_all(&install_dir);
                return Err(error.into());
            }
        };

        let existing =
            match collect_existing_global_installs(&global_pkg_dir, &aliases, &aliases_to_replace)
                .into_diagnostic()
                .wrap_err("scan existing global installs")
            {
                Ok(existing) => existing,
                Err(error) => {
                    let _ = fs::remove_dir_all(&install_dir);
                    return Err(error);
                }
            };
        let prospective_bins = get_actual_bin_names::<CmdShimHost>(&pkgs, &bins_to_skip);
        let replacement_plan = match plan_replaced_global_bins(
            &existing.groups_to_replace,
            &global_bin_dir,
            &prospective_bins,
            &existing.protected_bins,
            &crate::shim_dispatch::global_shims_setting(),
        ) {
            Ok(replacement_plan) => replacement_plan,
            Err(error) => {
                let _ = fs::remove_dir_all(&install_dir);
                return Err(error);
            }
        };
        let cache_hash = create_global_cache_key(&aliases, &registries_with_default(config));
        let hash_link = get_hash_link(&global_pkg_dir, &cache_hash);
        let linked_pkgs = hash_linked_packages(&pkgs, &install_dir, &hash_link);
        let activation = activate_global_install_with_extra_bin_names::<CmdShimHost>(
            &install_dir,
            &hash_link,
            &global_bin_dir,
            &pkgs,
            &bins_to_skip,
            &replacement_plan.affected_bin_names,
            || {
                link_global_bins(
                    base_config,
                    &linked_pkgs,
                    &dependencies,
                    &global_bin_dir,
                    &bins_to_skip,
                )?;
                restore_virtual_shims(&replacement_plan.shims_to_restore, &global_bin_dir)
            },
        )
        .wrap_err("activate global install")?;
        if let Some(leftover) = &activation.leftover_backup {
            warn_global::<Reporter>(&leftover.to_string());
        }
        let activated_bins = activation.activated_bins;
        if let Some(leftover) = cleanup_replaced_global_installs(
            &global_pkg_dir,
            &global_bin_dir,
            &existing.groups_to_replace,
            &cache_hash,
            &activated_bins,
            &existing.protected_bins,
            &replacement_plan.restored_bin_names(),
        )
        .wrap_err("remove existing global installs")?
        {
            warn_global::<Reporter>(&leftover.to_string());
        }
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
    let mut to_update: Vec<GlobalPackageInfo> = if params.is_empty() {
        all
    } else {
        let filtered: Vec<GlobalPackageInfo> =
            all.into_iter().filter(|pkg| params.iter().any(|param| pkg.has_alias(param))).collect();
        if filtered.is_empty() {
            println!("No matching global packages found");
            return Ok(());
        }
        filtered
    };
    if let Some(selected_hashes) = selected_hashes {
        to_update.retain(|pkg| selected_hashes.contains(&pkg.hash));
    }

    for pkg in &to_update {
        let install_dir = create_install_dir(&global_pkg_dir)
            .into_diagnostic()
            .wrap_err("create global install dir")?;
        let pins = Box::pin(pins_for_downgrades::<Reporter>(
            base_config,
            &global_pkg_dir,
            &install_dir,
            pkg,
            latest,
            range_spec_style,
            supported_architectures.clone(),
        ))
        .await?;
        Box::pin(run_group_install::<Reporter>(GroupInstall {
            base_config,
            global_pkg_dir: &global_pkg_dir,
            install_dir: &install_dir,
            selectors: &update_selectors(&pkg.dependencies, latest, &pins),
            range_spec_style,
            supported_architectures: supported_architectures.clone(),
            // `update -g` takes no `--allow-build`; the build policy comes
            // from the global `allowBuilds` loaded in `run_group_install`.
            allow_build: &[],
            lockfile_only: false,
        }))
        .await?;

        let pkgs = read_installed_packages(&install_dir);
        let dependencies = read_direct_dependencies(&install_dir);
        let _global_bin_lock = match acquire_global_bin_lock(&global_bin_dir) {
            Ok(lock) => lock,
            Err(error) => {
                let _ = fs::remove_dir_all(&install_dir);
                return Err(error);
            }
        };
        if let Err(error) = check_virtual_shim_conflicts(&pkgs, &global_bin_dir) {
            let _ = fs::remove_dir_all(&install_dir);
            return Err(error);
        }
        let bins_to_skip = match check_global_bin_conflicts(
            &global_pkg_dir,
            &global_bin_dir,
            &pkgs,
            |existing: &GlobalPackageInfo| existing.hash == pkg.hash,
        ) {
            Ok(skip) => skip,
            Err(error) => {
                let _ = fs::remove_dir_all(&install_dir);
                return Err(error.into());
            }
        };

        let protected =
            match bin_names_of_other_groups(&global_pkg_dir, &HashSet::from([pkg.hash.clone()]))
                .into_diagnostic()
                .wrap_err("scan global packages")
            {
                Ok(protected) => protected,
                Err(error) => {
                    let _ = fs::remove_dir_all(&install_dir);
                    return Err(error);
                }
            };
        let prospective_bins = get_actual_bin_names::<CmdShimHost>(&pkgs, &bins_to_skip);
        let replacement_plan = match plan_replaced_global_bins(
            std::slice::from_ref(pkg),
            &global_bin_dir,
            &prospective_bins,
            &protected,
            &crate::shim_dispatch::global_shims_setting(),
        ) {
            Ok(replacement_plan) => replacement_plan,
            Err(error) => {
                let _ = fs::remove_dir_all(&install_dir);
                return Err(error);
            }
        };
        let hash_link = get_hash_link(&global_pkg_dir, &pkg.hash);
        let linked_pkgs = hash_linked_packages(&pkgs, &install_dir, &hash_link);
        let activation = activate_global_install_with_extra_bin_names::<CmdShimHost>(
            &install_dir,
            &hash_link,
            &global_bin_dir,
            &pkgs,
            &bins_to_skip,
            &replacement_plan.affected_bin_names,
            || {
                link_global_bins(
                    base_config,
                    &linked_pkgs,
                    &dependencies,
                    &global_bin_dir,
                    &bins_to_skip,
                )?;
                restore_virtual_shims(&replacement_plan.shims_to_restore, &global_bin_dir)
            },
        )
        .wrap_err("activate global install")?;
        if let Some(leftover) = &activation.leftover_backup {
            warn_global::<Reporter>(&leftover.to_string());
        }
        let activated_bins = activation.activated_bins;
        if let Some(leftover) = cleanup_replaced_global_installs(
            &global_pkg_dir,
            &global_bin_dir,
            std::slice::from_ref(pkg),
            &pkg.hash,
            &activated_bins,
            &protected,
            &replacement_plan.restored_bin_names(),
        )
        .wrap_err("remove existing global installs")?
        {
            warn_global::<Reporter>(&leftover.to_string());
        }
    }
    Ok(())
}

/// With `--latest`, a dependency is reduced to its bare alias so the newest
/// registry version is resolved.
/// The selectors that reinstall a group. With `--latest` a plain version spec
/// is dropped so the newest release is picked; `pins` holds back the aliases
/// that would otherwise move backwards.
fn update_selectors(
    dependencies: &[(String, String)],
    latest: bool,
    pins: &HashMap<String, String>,
) -> Vec<String> {
    dependencies
        .iter()
        .map(|(alias, spec)| {
            if let Some(pin) = pins.get(alias) {
                format!("{alias}@{pin}")
            } else if latest && is_plain_version_spec(spec) {
                alias.clone()
            } else {
                format!("{alias}@{spec}")
            }
        })
        .collect()
}

/// The version to hold each dependency of `pkg` at, for the ones an update would
/// otherwise move backwards. `--latest` resolves the `latest` dist-tag, which
/// points at an older release than the one installed whenever that came from
/// another tag, or from a major that has not been promoted to `latest` yet.
///
/// The versions are resolved into `install_dir` without installing anything, so
/// a release that is about to be rejected never gets the chance to run its
/// lifecycle scripts. The install that follows reuses the lockfile written here
/// and only re-resolves what a pin changes.
///
/// Only plain version dependencies are considered: every other spec form says
/// where the package comes from, so holding one at a bare version would resolve
/// a different package from the default registry.
async fn pins_for_downgrades<Reporter: self::Reporter + 'static>(
    base_config: &'static Config,
    global_pkg_dir: &Path,
    install_dir: &Path,
    pkg: &GlobalPackageInfo,
    latest: bool,
    range_spec_style: RangeSpecStyle,
    supported_architectures: Option<SupportedArchitectures>,
) -> miette::Result<HashMap<String, String>> {
    // Only `--latest` can pick a version outside the recorded range, and only a
    // plain version spec is dropped for it. Everything else resolves within a
    // range the installed version already satisfies.
    if !latest {
        return Ok(HashMap::new());
    }
    let versions_before = installed_versions(&pkg.install_dir);
    // Nothing to compare a resolution against, so nothing to resolve.
    if !pkg
        .dependencies
        .iter()
        .any(|(alias, spec)| is_plain_version_spec(spec) && versions_before.contains_key(alias))
    {
        return Ok(HashMap::new());
    }
    run_group_install::<Reporter>(GroupInstall {
        base_config,
        global_pkg_dir,
        install_dir,
        selectors: &update_selectors(&pkg.dependencies, latest, &HashMap::new()),
        range_spec_style,
        supported_architectures,
        allow_build: &[],
        lockfile_only: true,
    })
    .await?;
    let resolved = resolved_direct_versions(install_dir);

    Ok(pkg
        .dependencies
        .iter()
        .filter(|(_, spec)| is_plain_version_spec(spec))
        .filter_map(|(alias, _)| {
            let before = Version::parse(versions_before.get(alias)?).ok()?;
            let now = resolved.get(alias)?;
            (*now < before).then(|| (alias.clone(), before.to_string()))
        })
        .collect())
}

/// The version each direct dependency resolved to, read from the lockfile the
/// resolve pass wrote. Only the plain-semver shape is reported: it is the only
/// one a plain version spec resolves to, and the only one a pin can hold.
fn resolved_direct_versions(install_dir: &Path) -> HashMap<String, Version> {
    let Ok(Some(lockfile)) = Lockfile::load_from_path(&install_dir.join(Lockfile::FILE_NAME))
    else {
        return HashMap::new();
    };
    let Some(importer) = lockfile.importers.get(Lockfile::ROOT_IMPORTER_KEY) else {
        return HashMap::new();
    };
    importer
        .dependencies
        .iter()
        .flatten()
        .filter_map(|(alias, resolved)| match &resolved.version {
            ImporterDepVersion::Regular(version) => {
                Some((alias.to_string(), version.version_semver()?.clone()))
            }
            _ => None,
        })
        .collect()
}

/// Only a plain version range may be dropped in favor of the bare alias.
/// Every other spec form (`link:`, `file:`, a git or tarball URL, an `npm:`
/// alias, a named registry) also says where the package comes from, so the
/// alias alone would be resolved from the default registry: a different
/// package gets installed, or the lookup 404s and aborts the groups that
/// have not been updated yet.
fn is_plain_version_spec(spec: &str) -> bool {
    !spec.contains(':')
}

/// `pnpm remove -g`. Removes the bins, hash symlinks, and install dirs of
/// every group that contains one of the requested packages.
pub fn handle_global_remove<Reporter: self::Reporter>(
    base_config: &'static Config,
    params: &[String],
) -> miette::Result<()> {
    let (global_pkg_dir, global_bin_dir) = global_dirs(base_config)?;
    check_bin_dir(&global_bin_dir)?;
    let _global_bin_lock = acquire_global_bin_lock(&global_bin_dir)?;

    let mut groups: Vec<GlobalPackageInfo> = Vec::new();
    let mut seen = HashSet::new();
    for param in params {
        let Some(pkg) = find_global_package(&global_pkg_dir, param)
            .into_diagnostic()
            .wrap_err("scan global packages")?
        else {
            return Err(GlobalError::PkgNotFound { param: param.clone() }.into());
        };
        if seen.insert(pkg.hash.clone()) {
            groups.push(pkg);
        }
    }

    // Bins shared with (and owned by) groups that survive this removal must
    // not be unlinked, or we'd delete another global package's bin.
    let exclude: HashSet<String> = groups.iter().map(|pkg| pkg.hash.clone()).collect();
    let protected = bin_names_of_other_groups(&global_pkg_dir, &exclude)
        .into_diagnostic()
        .wrap_err("scan global packages")?;
    let shims_to_restore = virtual_shims_to_restore(
        &groups,
        &global_bin_dir,
        &protected,
        &crate::shim_dispatch::global_shims_setting(),
    )?;
    let restored_bin_names = shims_to_restore.values().flatten().cloned().collect::<HashSet<_>>();
    let affected_bin_names = groups
        .iter()
        .flat_map(get_installed_bin_names)
        .filter(|bin| !protected.contains(bin))
        .collect::<HashSet<_>>();
    let mut bins_to_keep = protected;
    bins_to_keep.extend(restored_bin_names);
    let cleanup = GlobalInstallCleanup {
        global_pkg_dir: &global_pkg_dir,
        global_bin_dir: &global_bin_dir,
        bins_to_keep: &bins_to_keep,
        hash_to_keep: None,
        context: "global",
    };
    let transaction = GlobalRemovalTransaction {
        groups: &groups,
        cleanup: &cleanup,
        affected_bin_names: &affected_bin_names,
    };
    let leftover_backup = commit_global_removal::<CmdShimHost>(&transaction, || {
        restore_virtual_shims(&shims_to_restore, &global_bin_dir)
    })?;
    if let Some(leftover) = leftover_backup {
        warn_global::<Reporter>(&leftover.to_string());
    }
    removed_global_install_result(cleanup_removed_global_install_dirs(&groups, &cleanup))
}

fn check_virtual_shim_conflicts(
    packages: &[PackageBinSource],
    global_bin_dir: &Path,
) -> miette::Result<()> {
    let mut providers_by_bin: HashMap<String, BTreeSet<String>> = HashMap::new();
    for package in packages {
        let package_name =
            package.manifest.get("name").and_then(serde_json::Value::as_str).unwrap_or("");
        for command in pnpm_cmd_shim::get_bins_from_package_manifest::<CmdShimHost>(
            &package.manifest,
            &package.location,
        ) {
            providers_by_bin.entry(command.name).or_default().insert(package_name.to_string());
        }
    }
    if providers_by_bin.is_empty() {
        return Ok(());
    }
    let restoration_owners = virtual_shim_restoration_owners(global_bin_dir)?;
    for (bin, providers) in providers_by_bin {
        let bin_path = global_bin_dir.join(&bin);
        let owner = virtual_shim_owner(&bin_path)
            .into_diagnostic()
            .wrap_err_with(|| format!("inspect global bin at {}", bin_path.display()))?;
        let owner = owner.as_ref().or_else(|| restoration_owners.get(&bin));
        let Some(owner) = owner else { continue };
        if providers.len() == 1 && providers.contains(owner) {
            continue;
        }
        return Err(GlobalError::VirtualShimBinConflict {
            packages: providers.into_iter().collect::<Vec<_>>().join(", "),
            bin,
            shim_package: owner.clone(),
        }
        .into());
    }
    Ok(())
}

fn virtual_shims_to_restore(
    groups: &[GlobalPackageInfo],
    global_bin_dir: &Path,
    protected: &HashSet<String>,
    enabled: &GlobalShims,
) -> miette::Result<BTreeMap<String, BTreeSet<String>>> {
    let mut shims = BTreeMap::<String, BTreeSet<String>>::new();
    for group in groups {
        for package in read_installed_packages(&group.install_dir) {
            let Some(package_name) =
                package.manifest.get("name").and_then(serde_json::Value::as_str)
            else {
                continue;
            };
            if !enabled.is_enabled(package_name) {
                continue;
            }
            let recorded = virtual_shim_bins_to_restore(global_bin_dir, package_name)?
                .into_iter()
                .collect::<HashSet<_>>();
            if recorded.is_empty() {
                continue;
            }
            for command in pnpm_cmd_shim::get_bins_from_package_manifest::<CmdShimHost>(
                &package.manifest,
                &package.location,
            ) {
                if recorded.contains(&command.name) && !protected.contains(&command.name) {
                    shims.entry(package_name.to_string()).or_default().insert(command.name);
                }
            }
        }
    }
    Ok(shims)
}

struct ReplacedGlobalBinPlan {
    shims_to_restore: BTreeMap<String, BTreeSet<String>>,
    affected_bin_names: HashSet<String>,
}

impl ReplacedGlobalBinPlan {
    fn restored_bin_names(&self) -> HashSet<String> {
        self.shims_to_restore.values().flatten().cloned().collect()
    }
}

fn plan_replaced_global_bins(
    groups: &[GlobalPackageInfo],
    global_bin_dir: &Path,
    prospective_bins: &HashSet<String>,
    protected_bins: &HashSet<String>,
    enabled: &GlobalShims,
) -> miette::Result<ReplacedGlobalBinPlan> {
    let occupied_bins = prospective_bins.union(protected_bins).cloned().collect::<HashSet<_>>();
    let shims_to_restore =
        virtual_shims_to_restore(groups, global_bin_dir, &occupied_bins, enabled)?;
    let affected_bin_names = groups
        .iter()
        .flat_map(get_installed_bin_names)
        .filter(|bin| !occupied_bins.contains(bin))
        .collect();
    Ok(ReplacedGlobalBinPlan { shims_to_restore, affected_bin_names })
}

fn restore_virtual_shims(
    shims_to_restore: &BTreeMap<String, BTreeSet<String>>,
    global_bin_dir: &Path,
) -> miette::Result<()> {
    for (package, bins) in shims_to_restore {
        for bin in bins {
            install_native_shim(global_bin_dir, bin, &ShimTarget::Virtual(package.clone()))
                .into_diagnostic()
                .wrap_err_with(|| format!("restore the {package} shims"))?;
        }
    }
    Ok(())
}

/// What to install into a fresh global group directory. See
/// [`run_group_install`].
struct GroupInstall<'a> {
    base_config: &'a Config,
    global_pkg_dir: &'a Path,
    /// The group's own directory, created by the caller.
    install_dir: &'a Path,
    selectors: &'a [String],
    range_spec_style: RangeSpecStyle,
    supported_architectures: Option<SupportedArchitectures>,
    allow_build: &'a [String],
    /// Resolve and write the lockfile without linking anything or running a
    /// build. Nothing a resolution is only being inspected for gets the chance
    /// to run its lifecycle scripts.
    lockfile_only: bool,
}

/// Install `install.selectors` into `install.install_dir`, returning the leaked
/// per-group [`Config`] (anchored there, saving to `dependencies`). Then run the
/// global build-approval flow. Shared by add and update.
async fn run_group_install<Reporter: self::Reporter + 'static>(
    install: GroupInstall<'_>,
) -> miette::Result<&'static Config> {
    let GroupInstall {
        base_config,
        global_pkg_dir,
        install_dir,
        selectors,
        range_spec_style,
        supported_architectures,
        allow_build,
        lockfile_only,
    } = install;
    let mut cfg =
        global_group_config(base_config, install_dir, global_pkg_dir, supported_architectures)?;
    apply_allow_build(&mut cfg, allow_build, global_pkg_dir)?;

    let config: &'static Config = Config::leak(cfg);

    let manifest_path = install_dir.join("package.json");
    let selectors = selectors
        .iter()
        .map(|selector| infer_local_package_alias(selector))
        .collect::<miette::Result<Vec<_>>>()?;
    let state = State::init(manifest_path, config, false)
        .wrap_err("initialize the global install state")?;
    add_packages::<Reporter, _>(
        state,
        &selectors,
        range_spec_style,
        None,
        lockfile_only,
        config.supported_architectures.clone(),
        Some([DependencyGroup::Prod]),
    )
    .await?;

    if !lockfile_only {
        prompt_approve_global_builds::<Reporter>(config, install_dir, global_pkg_dir).await?;
    }
    Ok(config)
}

fn global_group_config(
    base_config: &Config,
    install_dir: &Path,
    global_pkg_dir: &Path,
    supported_architectures: Option<SupportedArchitectures>,
) -> miette::Result<Config> {
    let mut cfg = base_config.clone();
    cfg.modules_dir = install_dir.join("node_modules");
    cfg.virtual_store_dir = install_dir.join("node_modules").join(".pnpm");
    // Each global group is self-contained, so the virtual store lives
    // inside its install dir (never the shared global one).
    cfg.enable_global_virtual_store = false;
    // Persist a `pnpm-lock.yaml` in the group's install dir (pnpm sets
    // `lockfileDir = installDir`). `outdated -g` / `update -g` read these
    // pins to determine the currently-installed versions. A `lockfileDir`
    // the environment set cannot redirect it — pnpm deletes the setting
    // under `--global`.
    cfg.lockfile = true;
    cfg.lockfile_dir = None;
    // Pin the group's workspace root to its own install dir (pnpm's
    // `rootProjectManifestDir: installDir`, `workspaceDir: undefined`). The
    // install dir sits *under* the global packages dir, which carries a
    // `pnpm-workspace.yaml` of global settings (`allowBuilds`, `catalog`,
    // ...). Leaving this unset would let the install pipeline walk up, adopt
    // that file as the workspace, and then fail trying to enumerate its
    // non-existent root project. Anchoring here keeps the group install an
    // isolated single project.
    cfg.workspace_dir = Some(install_dir.to_path_buf());
    cfg.supported_architectures = supported_architectures;

    // A global install is isolated from the caller's project, so it must
    // not inherit that project's dependency-graph configuration. pnpm
    // achieves this by running the install with `cwd` = the pnpm home dir;
    // pacquet clones the caller's already-loaded config, so drop those
    // project-scoped resolution settings explicitly. Inheriting `overrides`
    // is what surfaced as `ERR_PNPM_CATALOG_IN_OVERRIDES` — a repo override
    // referencing a `catalog:` the isolated install (with no catalogs) no
    // longer resolves — and inheriting `catalogMode: strict` would likewise
    // reject the install against an empty catalog.
    cfg.overrides = None;
    cfg.catalogs = None;
    cfg.catalog_mode = CatalogMode::default();
    cfg.package_extensions = None;
    cfg.patched_dependencies = None;
    // The GVS resolution env injected by `Config::current` points at the
    // *caller's* node_modules; the group's own virtual store is
    // project-local (GVS forced off above), so inheriting it would let
    // the group's lifecycle scripts resolve phantom deps from the
    // caller's tree.
    cfg.extra_env.remove("NODE_PATH");
    cfg.extra_env.remove("NODE_OPTIONS");

    // Build-script policy for global installs comes from the global packages
    // directory, never the caller's repo — otherwise a repo-controlled
    // `pnpm-workspace.yaml` could decide which lifecycle scripts run during
    // `add -g` / `update -g`. Drop the inherited repo policy and load the
    // global `allowBuilds` (where the approval prompt persists its
    // decisions) instead.
    cfg.dangerously_allow_all_builds = false;
    cfg.allow_builds.clear();
    if let Some((_, settings)) = WorkspaceSettings::find_and_load(global_pkg_dir)
        .map_err(miette::Report::new)
        .wrap_err("load global allowBuilds")?
    {
        if let Some(allow_builds) = settings.allow_builds {
            cfg.allow_builds = decided_allow_builds(allow_builds);
        }
        if let Some(allow_all) = settings.dangerously_allow_all_builds {
            cfg.dangerously_allow_all_builds = allow_all;
        }
    }
    // Don't fail the install when a dependency's build is ignored; the
    // global approval prompt (run after the install) records the ignored
    // builds and prompts rather than erroring under `strictDepBuilds`.
    cfg.strict_dep_builds = false;

    Ok(cfg)
}

pub async fn approve_global_builds<Reporter: self::Reporter + 'static>(
    base_config: &'static Config,
    args: ApproveBuildsArgs,
) -> miette::Result<()> {
    args.validate()?;
    let global_pkg_dir = base_config.global_pkg_dir.as_ref().ok_or(GlobalError::NoGlobalBinDir)?;
    let packages =
        scan_global_packages(global_pkg_dir).into_diagnostic().wrap_err("scan global packages")?;
    let mut groups: Vec<(PathBuf, IgnoredBuildsScan)> = Vec::new();
    let mut pending = BTreeSet::new();
    let canonical_global_pkg_dir = (!packages.is_empty())
        .then(|| dunce::canonicalize(global_pkg_dir))
        .transpose()
        .into_diagnostic()
        .wrap_err("resolve the global packages directory")?;
    for package in packages {
        if canonical_global_pkg_dir
            .as_ref()
            .is_some_and(|root| !is_subdir(root, &package.install_dir))
        {
            continue;
        }
        let config = global_group_config(
            base_config,
            &package.install_dir,
            global_pkg_dir,
            base_config.supported_architectures.clone(),
        )?;
        let scan = get_automatically_ignored_builds(&config)?;
        if let Some(names) = &scan.names {
            pending.extend(names.iter().cloned());
        }
        groups.push((package.install_dir, scan));
    }
    if pending.is_empty() {
        println!("There are no packages awaiting approval");
        return Ok(());
    }
    let pending = pending.into_iter().collect::<Vec<_>>();
    let Some(decision) = args.decide(&pending)? else {
        return Ok(());
    };

    write_approval_settings(global_pkg_dir, &decision)?;
    let mut rebuild_groups = Vec::new();
    for (install_dir, scan) in groups {
        let build_packages: Vec<String> = decision
            .build_packages
            .iter()
            .filter(|name| scan.names.as_ref().is_some_and(|names| names.contains(name)))
            .cloned()
            .collect();
        clear_decided_ignored_builds(scan.modules_manifest, &scan.modules_dir, &decision)?;
        if !build_packages.is_empty() {
            rebuild_groups.push((install_dir, build_packages));
        }
    }
    for (install_dir, build_packages) in rebuild_groups {
        let config = Config::leak(global_group_config(
            base_config,
            &install_dir,
            global_pkg_dir,
            base_config.supported_architectures.clone(),
        )?);
        let state = State::init(install_dir.join("package.json"), config, true)
            .wrap_err("initialize the global approve-builds state")?;
        let selection = crate::cli_args::rebuild::RebuildSelection {
            names: Some(build_packages),
            projects: Vec::new(),
        };
        run_rebuild::<Reporter>(&state, selection, None).await?;
    }
    Ok(())
}

/// Run the interactive build-approval flow against the just-installed
/// group. No-op when nothing is awaiting approval, or when stdin is not a
/// TTY (unless the test auto-approve env var is set).
async fn prompt_approve_global_builds<Reporter: self::Reporter + 'static>(
    config: &'static Config,
    install_dir: &Path,
    global_pkg_dir: &Path,
) -> miette::Result<()> {
    let pending = get_automatically_ignored_builds(config)?.names.filter(|names| !names.is_empty());
    if pending.is_none() {
        return Ok(());
    }
    let auto_approve = std::env::var("PNPM_AUTO_APPROVE_BUILDS_FOR_TESTS").as_deref() == Ok("1");
    if !auto_approve && !std::io::stdin().is_terminal() {
        return Ok(());
    }

    let manifest_path = install_dir.join("package.json");
    let config_fn = || -> miette::Result<&'static mut Config> {
        // `prepare` persists the `allowBuilds` decision to
        // `config.workspace_dir`, falling back to the passed dir (the global
        // packages dir). The group install pins `workspace_dir` to the
        // ephemeral install dir; clear it here so the decision lands in the
        // stable global packages dir, where the next global install reads it
        // back — rather than in a throwaway install group.
        let mut cfg = config.clone();
        cfg.workspace_dir = None;
        Ok(Config::leak(cfg))
    };
    // The rebuild stays anchored at the install dir (keeping `config`'s
    // `workspace_dir`), so its install pipeline doesn't walk up into the
    // global settings workspace.
    let state_fn = |require_lockfile: bool| -> miette::Result<State> {
        State::init(manifest_path.clone(), Config::leak(config.clone()), require_lockfile)
            .wrap_err("initialize the global approve-builds state")
    };

    let args = ApproveBuildsArgs { packages: Vec::new(), all: auto_approve, global: false };
    if let Some((rebuild_state, build_packages)) =
        args.prepare(global_pkg_dir, &config_fn, &state_fn)?
    {
        let selection = crate::cli_args::rebuild::RebuildSelection {
            names: Some(build_packages),
            projects: Vec::new(),
        };
        run_rebuild::<Reporter>(&rebuild_state, selection, None).await?;
    }
    Ok(())
}

struct ExistingGlobalInstalls {
    groups_to_replace: Vec<GlobalPackageInfo>,
    protected_bins: HashSet<String>,
}

fn collect_existing_global_installs(
    global_pkg_dir: &Path,
    aliases: &[String],
    aliases_to_replace: &[String],
) -> std::io::Result<ExistingGlobalInstalls> {
    let mut groups_to_replace = Vec::new();
    let mut seen = HashSet::new();
    for alias in aliases_to_replace {
        if let Some(pkg) = find_global_package(global_pkg_dir, alias)?
            && should_replace_existing_package(&pkg, aliases, aliases_to_replace)
            && seen.insert(pkg.hash.clone())
        {
            groups_to_replace.push(pkg);
        }
    }
    let exclude = groups_to_replace.iter().map(|pkg| pkg.hash.clone()).collect();
    let protected_bins = bin_names_of_other_groups(global_pkg_dir, &exclude)?;
    Ok(ExistingGlobalInstalls { groups_to_replace, protected_bins })
}

#[derive(Debug, Display, Error, Diagnostic)]
#[display("Failed to remove global packages")]
struct RemovedGlobalInstallCleanupError {
    #[error(not(source))]
    #[related]
    cleanup_reports: Vec<ArtifactCleanupError>,
}

struct GlobalInstallCleanup<'a> {
    global_pkg_dir: &'a Path,
    global_bin_dir: &'a Path,
    bins_to_keep: &'a HashSet<String>,
    hash_to_keep: Option<&'a str>,
    context: &'static str,
}

struct GlobalRemovalTransaction<'a> {
    groups: &'a [GlobalPackageInfo],
    cleanup: &'a GlobalInstallCleanup<'a>,
    affected_bin_names: &'a HashSet<String>,
}

trait FsGlobalRemoval: FsRename {
    fn remove_bin_slot(path: &Path) -> io::Result<()> {
        if let (Some(bin_dir), Some(name)) =
            (path.parent(), path.file_name().and_then(std::ffi::OsStr::to_str))
        {
            remove_native_shim(bin_dir, name)?;
        }
        remove_cmd_shim(path)
    }

    fn remove_hash_link(path: &Path) -> io::Result<()> {
        remove_symlink_dir(path)
    }

    fn restore_hash_link(target: &Path, link: &Path) -> io::Result<()> {
        symlink_dir(target, link)
    }
}

impl FsGlobalRemoval for CmdShimHost {}

fn cleanup_replaced_global_installs(
    global_pkg_dir: &Path,
    global_bin_dir: &Path,
    groups: &[GlobalPackageInfo],
    active_hash: &str,
    activated_bins: &HashSet<String>,
    protected_bins: &HashSet<String>,
    restored_bin_names: &HashSet<String>,
) -> miette::Result<Option<ArtifactCleanupError>> {
    if groups.is_empty() {
        return Ok(None);
    }
    let mut bins_to_keep = activated_bins.union(protected_bins).cloned().collect::<HashSet<_>>();
    bins_to_keep.extend(restored_bin_names.iter().cloned());
    let affected_bin_names = groups
        .iter()
        .flat_map(get_installed_bin_names)
        .filter(|bin| !bins_to_keep.contains(bin))
        .collect::<HashSet<_>>();
    let cleanup = GlobalInstallCleanup {
        global_pkg_dir,
        global_bin_dir,
        bins_to_keep: &bins_to_keep,
        hash_to_keep: Some(active_hash),
        context: "replaced global",
    };
    let transaction = GlobalRemovalTransaction {
        groups,
        cleanup: &cleanup,
        affected_bin_names: &affected_bin_names,
    };
    let leftover_backup = commit_global_removal::<CmdShimHost>(&transaction, || Ok(()))?;
    removed_global_install_result(cleanup_removed_global_install_dirs(groups, &cleanup))?;
    Ok(leftover_backup)
}

fn commit_global_removal<Sys: FsGlobalRemoval>(
    transaction: &GlobalRemovalTransaction<'_>,
    replace_bins: impl FnOnce() -> miette::Result<()>,
) -> miette::Result<Option<ArtifactCleanupError>> {
    replace_global_bin_slots::<Sys>(
        transaction.cleanup.global_bin_dir,
        transaction.affected_bin_names,
        || {
            replace_bins()?;
            remove_global_install_entries::<Sys>(transaction)
        },
    )
}

fn remove_global_install_entries<Sys: FsGlobalRemoval>(
    transaction: &GlobalRemovalTransaction<'_>,
) -> miette::Result<()> {
    let cleanup = transaction.cleanup;
    let cleanup_reports = cleanup_global_bin_names::<Sys>(transaction.affected_bin_names, cleanup);
    if !cleanup_reports.is_empty() {
        return removed_global_install_result(cleanup_reports);
    }

    let mut removed_hash_groups = Vec::new();
    for group in transaction.groups {
        match remove_global_hash_link::<Sys>(group, cleanup) {
            Ok(true) => removed_hash_groups.push(group),
            Ok(false) => {}
            Err(report) => {
                let mut cleanup_reports = vec![report];
                for removed_group in removed_hash_groups.into_iter().rev() {
                    let hash_link = get_hash_link(cleanup.global_pkg_dir, &removed_group.hash);
                    if let Err(source) =
                        Sys::restore_hash_link(&removed_group.install_dir, &hash_link)
                    {
                        cleanup_reports.push(ArtifactCleanupError {
                            context: format!(
                                "restore {} hash link at {}",
                                cleanup.context,
                                hash_link.display(),
                            ),
                            source,
                        });
                    }
                }
                return removed_global_install_result(cleanup_reports);
            }
        }
    }
    Ok(())
}

fn cleanup_removed_global_install_dirs(
    groups: &[GlobalPackageInfo],
    cleanup: &GlobalInstallCleanup<'_>,
) -> Vec<ArtifactCleanupError> {
    groups.iter().filter_map(|group| cleanup_global_install_dir(group, cleanup)).collect()
}

fn removed_global_install_result(
    mut cleanup_reports: Vec<ArtifactCleanupError>,
) -> miette::Result<()> {
    if cleanup_reports.is_empty() {
        return Ok(());
    }
    if cleanup_reports.len() == 1 {
        return Err(miette::Report::new(cleanup_reports.remove(0)));
    }
    Err(RemovedGlobalInstallCleanupError { cleanup_reports }.into())
}

fn cleanup_global_bin_names<Sys: FsGlobalRemoval>(
    bin_names: &HashSet<String>,
    cleanup: &GlobalInstallCleanup<'_>,
) -> Vec<ArtifactCleanupError> {
    bin_names.iter().filter_map(|bin_name| cleanup_global_bin::<Sys>(bin_name, cleanup)).collect()
}

fn cleanup_global_bin<Sys: FsGlobalRemoval>(
    bin_name: &str,
    cleanup: &GlobalInstallCleanup<'_>,
) -> Option<ArtifactCleanupError> {
    if cleanup.bins_to_keep.contains(bin_name) {
        return None;
    }
    let bin_path = cleanup.global_bin_dir.join(bin_name);
    Sys::remove_bin_slot(&bin_path).err().map(|source| ArtifactCleanupError {
        context: format!("remove {} bin at {}", cleanup.context, bin_path.display()),
        source,
    })
}

fn remove_global_hash_link<Sys: FsGlobalRemoval>(
    group: &GlobalPackageInfo,
    cleanup: &GlobalInstallCleanup<'_>,
) -> Result<bool, ArtifactCleanupError> {
    if cleanup.hash_to_keep != Some(group.hash.as_str()) {
        let hash_link = get_hash_link(cleanup.global_pkg_dir, &group.hash);
        match Sys::remove_hash_link(&hash_link) {
            Ok(()) => return Ok(true),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(ArtifactCleanupError {
                    context: format!(
                        "remove {} hash link at {}",
                        cleanup.context,
                        hash_link.display(),
                    ),
                    source,
                });
            }
        }
    }
    Ok(false)
}

fn cleanup_global_install_dir(
    group: &GlobalPackageInfo,
    cleanup: &GlobalInstallCleanup<'_>,
) -> Option<ArtifactCleanupError> {
    if is_subdir(cleanup.global_pkg_dir, &group.install_dir) {
        match fs::remove_dir_all(&group.install_dir) {
            Ok(()) => return None,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return None,
            Err(source) => {
                return Some(ArtifactCleanupError {
                    context: format!(
                        "remove {} install directory at {}",
                        cleanup.context,
                        group.install_dir.display(),
                    ),
                    source,
                });
            }
        }
    }
    None
}

fn replacement_aliases(aliases: &[String]) -> Vec<String> {
    const PNPM_CLI_PACKAGE_ALIASES: [&str; 2] = ["pnpm", "@pnpm/exe"];

    let mut expanded = aliases.to_vec();
    if aliases.iter().any(|alias| is_pnpm_cli_package_name(alias)) {
        for alias in PNPM_CLI_PACKAGE_ALIASES {
            if !expanded.iter().any(|existing| existing == alias) {
                expanded.push(alias.to_string());
            }
        }
    }
    expanded
}

fn should_replace_existing_package(
    pkg: &GlobalPackageInfo,
    aliases: &[String],
    aliases_to_replace: &[String],
) -> bool {
    if aliases.iter().any(|alias| pkg.has_alias(alias)) {
        return true;
    }
    is_pnpm_cli_only_group(pkg) && aliases_to_replace.iter().any(|alias| pkg.has_alias(alias))
}

/// Whether `pkg` is a global group the pnpm CLI is installed in — the install
/// that `pnpm self-update` owns. `update -g` leaves the whole group alone:
/// reinstalling it would relink pnpm's bin whatever else the group holds.
pub fn has_pnpm_cli_dependency(pkg: &GlobalPackageInfo) -> bool {
    pkg.dependencies.iter().any(|(alias, spec)| is_pnpm_cli_dependency(alias, Some(spec)))
}

/// Whether `pkg` is a global group holding nothing but the pnpm CLI. `add -g`
/// refuses to create one.
fn is_pnpm_cli_only_group(pkg: &GlobalPackageInfo) -> bool {
    !pkg.dependencies.is_empty()
        && pkg.dependencies.iter().all(|(alias, spec)| is_pnpm_cli_dependency(alias, Some(spec)))
}

/// Whether any of `params` names the pnpm CLI itself. Each selector is
/// normalized to the package it installs first, so neither a versioned form
/// like `pnpm@9` nor an aliased one like `foo@npm:pnpm@9` bypasses the guard.
pub fn selects_pnpm_cli<'a>(params: impl IntoIterator<Item = &'a String>) -> bool {
    params.into_iter().any(|param| {
        let parsed = parse_wanted_dependency(param);
        is_pnpm_cli_dependency(
            parsed.alias.as_deref().unwrap_or_default(),
            parsed.bare_specifier.as_deref(),
        )
    })
}

/// Whether a dependency declared as `alias` at `spec` is the pnpm CLI. An
/// `npm:` alias resolves to its target, so `foo` at `npm:pnpm@9` is the pnpm
/// CLI under another name — the install still carries pnpm's own `pnpm` bin.
fn is_pnpm_cli_dependency(alias: &str, spec: Option<&str>) -> bool {
    let name = npm_alias_target(spec);
    is_pnpm_cli_package_name(name.as_deref().unwrap_or(alias))
}

fn is_pnpm_cli_package_name(name: &str) -> bool {
    matches!(name, "pnpm" | "@pnpm/exe")
}

/// The package an `npm:` alias points at, or `None` for any other spec.
fn npm_alias_target(spec: Option<&str>) -> Option<String> {
    parse_wanted_dependency(spec?.strip_prefix("npm:")?).alias
}

/// The set of bin names provided by global package groups other than those
/// in `exclude_hashes`.
fn bin_names_of_other_groups(
    global_pkg_dir: &Path,
    exclude_hashes: &HashSet<String>,
) -> std::io::Result<HashSet<String>> {
    let mut names = HashSet::new();
    for pkg in scan_global_packages(global_pkg_dir)? {
        if exclude_hashes.contains(&pkg.hash) {
            continue;
        }
        for bin in get_installed_bin_names(&pkg) {
            names.insert(bin);
        }
    }
    Ok(names)
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

// --- param grouping (split/resolve helpers) -------------------------------

fn split_into_groups(params: &[String], base_dir: &Path) -> Vec<Vec<String>> {
    params
        .iter()
        .map(|param| {
            split_comma_separated(param, base_dir)
                .into_iter()
                .map(|token| resolve_local_param(&token, base_dir))
                .collect::<Vec<String>>()
        })
        .filter(|group| !group.is_empty())
        .collect()
}

fn split_comma_separated(param: &str, base_dir: &Path) -> Vec<String> {
    if !param.contains(',') {
        return vec![param.to_string()];
    }
    if param.contains("://") {
        return vec![param.to_string()];
    }
    if refers_to_existing_local_path(param, base_dir) {
        return vec![param.to_string()];
    }
    param.split(',').map(str::trim).filter(|token| !token.is_empty()).map(str::to_string).collect()
}

fn refers_to_existing_local_path(param: &str, base_dir: &Path) -> bool {
    let path_part = if let Some(rest) = param.strip_prefix("file:") {
        rest
    } else if let Some(rest) = param.strip_prefix("link:") {
        rest
    } else if param.starts_with('.')
        || param.starts_with('/')
        || param.starts_with('~')
        || is_windows_drive_path(param)
    {
        param
    } else {
        return false;
    };
    let resolved = if Path::new(path_part).is_absolute() {
        PathBuf::from(path_part)
    } else {
        base_dir.join(path_part)
    };
    resolved.exists()
}

/// Mirror the TypeScript `resolveLocalParam`: rewrite only *dot-relative*
/// `file:`/`link:` selectors against `base_dir`. Bare names, home-relative
/// (`~/`), and absolute selectors pass through unchanged so the local
/// resolver's own `~` expansion and registry fallbacks still apply.
fn resolve_local_param(param: &str, base_dir: &Path) -> String {
    for prefix in ["file:", "link:"] {
        if let Some(rest) = param.strip_prefix(prefix) {
            if rest.starts_with('.') {
                return format!("{prefix}{}", lexical_normalize(&base_dir.join(rest)).display());
            }
            return param.to_string();
        }
    }
    if param.starts_with('.') {
        return lexical_normalize(&base_dir.join(param)).display().to_string();
    }
    param.to_string()
}

fn infer_local_package_alias(selector: &str) -> miette::Result<String> {
    let Some(path) = selector.strip_prefix("file:").map(Path::new) else {
        return Ok(selector.to_string());
    };
    let path_display = path.display().to_string();
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(selector.to_string());
        }
        Err(error) => {
            return Err(error)
                .into_diagnostic()
                .wrap_err(format!("read local package metadata from {path_display}"));
        }
    };
    if !metadata.is_dir() {
        return Ok(selector.to_string());
    }
    let manifest = safe_read_package_json_from_dir(path)
        .map_err(miette::Report::new)
        .wrap_err_with(|| format!("read local package manifest from {path_display}"))?
        .ok_or_else(|| miette::miette!("No package.json was found in {path_display}"))?;
    let name = manifest
        .get("name")
        .and_then(serde_json::Value::as_str)
        .filter(|name| !name.is_empty())
        .or_else(|| path.file_name().and_then(|name| name.to_str()))
        .filter(|name| !name.is_empty())
        .ok_or_else(|| {
            miette::miette!("The local package at {path_display} has no package name")
        })?;
    if !is_valid_old_npm_package_name(name) {
        return Err(GlobalError::InvalidPackageName { name: name.to_string() }.into());
    }
    Ok(format!("{name}@{selector}"))
}

fn is_windows_drive_path(param: &str) -> bool {
    let bytes = param.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'/' || bytes[2] == b'\\')
}

#[cfg(test)]
mod tests;
