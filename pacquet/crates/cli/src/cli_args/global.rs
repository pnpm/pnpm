//! Global package install command handlers.
//!
//! Ports pnpm's `@pnpm/global.commands`
//! ([`globalAdd`](https://github.com/pnpm/pnpm/blob/1819226b51/global/commands/src/globalAdd.ts),
//! [`globalUpdate`](https://github.com/pnpm/pnpm/blob/1819226b51/global/commands/src/globalUpdate.ts),
//! [`globalRemove`](https://github.com/pnpm/pnpm/blob/1819226b51/global/commands/src/globalRemove.ts)).
//! Each space-separated CLI param is its own isolated install group (a
//! comma splits a group; local paths / URLs are kept whole). A group
//! installs into a fresh directory under the global packages dir, then a
//! hash symlink and the global bins are pointed at it.

use crate::{
    State,
    cli_args::{
        add::add_package, approve_builds::ApproveBuildsArgs,
        ignored_builds::get_automatically_ignored_builds, rebuild::run_rebuild,
    },
};
use derive_more::{Display, Error};
use miette::{Context, Diagnostic, IntoDiagnostic};
use pacquet_cmd_shim::{Host as CmdShimHost, link_bins_of_packages_with_excludes, remove_bin};
use pacquet_config::{Config, WorkspaceSettings, check_global_bin_dir};
use pacquet_fs::{force_symlink_dir, is_subdir, lexical_normalize};
use pacquet_global::{
    GlobalPackageInfo, check_global_bin_conflicts, clean_orphaned_install_dirs,
    create_global_cache_key, create_install_dir, find_global_package, get_hash_link,
    get_installed_bin_names, read_installed_packages, scan_global_packages,
};
use pacquet_package_is_installable::SupportedArchitectures;
use pacquet_package_manifest::DependencyGroup;
use pacquet_registry::PinnedVersion;
use pacquet_reporter::Reporter;
use pacquet_resolving_parse_wanted_dependency::parse_wanted_dependency;
use serde_json::Value;
use std::{
    collections::HashSet,
    fs,
    io::IsTerminal,
    path::{Path, PathBuf},
};

/// Errors specific to global package management. Codes mirror pnpm's
/// `ERR_PNPM_`-prefixed `PnpmError` codes.
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

    #[display(r#"Use the "pnpm self-update" command to install or update pnpm"#)]
    #[diagnostic(code(ERR_PNPM_GLOBAL_PNPM_INSTALL))]
    GlobalPnpmInstall,

    #[display("Cannot remove '{param}': not found in global packages")]
    #[diagnostic(code(ERR_PNPM_GLOBAL_PKG_NOT_FOUND))]
    PkgNotFound { param: String },
}

/// Resolve the global packages and global bin directories, erroring with
/// `NO_GLOBAL_BIN_DIR` when the pnpm home can't be determined — matching
/// pnpm's `if (!opts.bin)` guard.
fn global_dirs(config: &Config) -> Result<(PathBuf, PathBuf), GlobalError> {
    let bin = config.global_bin.clone().ok_or(GlobalError::NoGlobalBinDir)?;
    let pkg_dir = config.global_pkg_dir.clone().ok_or(GlobalError::NoGlobalBinDir)?;
    Ok((pkg_dir, bin))
}

/// Validate the global bin dir is on `PATH` and writable. Mirrors pnpm's
/// `checkGlobalBinDir` for mutating commands.
fn check_bin_dir(global_bin_dir: &Path) -> miette::Result<()> {
    check_global_bin_dir(global_bin_dir, std::env::var("PATH").ok().as_deref(), true)
        .map_err(miette::Report::new)
}

/// `pnpm add -g`. Installs each group, links its bins into the global bin
/// directory, and records a cache-keyed hash symlink.
pub async fn handle_global_add<Reporter: self::Reporter + 'static>(
    base_config: &'static Config,
    params: &[String],
    pinned_version: PinnedVersion,
    supported_architectures: Option<SupportedArchitectures>,
    cwd: &Path,
) -> miette::Result<()> {
    // Normalize each selector to its package name first, so versioned forms
    // like `pnpm@9` or `@pnpm/exe@1` can't bypass the self-install guard.
    if params.iter().any(|param| {
        matches!(parse_wanted_dependency(param).alias.as_deref(), Some("pnpm" | "@pnpm/exe"))
    }) {
        return Err(GlobalError::GlobalPnpmInstall.into());
    }
    let (global_pkg_dir, global_bin_dir) = global_dirs(base_config)?;
    check_bin_dir(&global_bin_dir)?;
    fs::create_dir_all(&global_pkg_dir)
        .into_diagnostic()
        .wrap_err("create the global packages directory")?;
    clean_orphaned_install_dirs(&global_pkg_dir);

    for group in split_into_groups(params, cwd) {
        let (install_dir, config) = Box::pin(run_group_install::<Reporter>(
            base_config,
            &global_pkg_dir,
            &group,
            pinned_version,
            supported_architectures.clone(),
        ))
        .await?;

        let pkgs = read_installed_packages(&install_dir);
        let aliases = read_aliases(&install_dir);

        let bins_to_skip = match check_global_bin_conflicts(
            &global_pkg_dir,
            &global_bin_dir,
            &pkgs,
            |existing: &GlobalPackageInfo| aliases.iter().any(|alias| existing.has_alias(alias)),
        ) {
            Ok(skip) => skip,
            Err(error) => {
                let _ = fs::remove_dir_all(&install_dir);
                return Err(error.into());
            }
        };

        remove_existing_global_installs(&global_pkg_dir, &global_bin_dir, &aliases);

        let cache_hash = create_global_cache_key(&aliases, &registries_with_default(config));
        let hash_link = get_hash_link(&global_pkg_dir, &cache_hash);
        force_symlink_dir(&install_dir, &hash_link)
            .into_diagnostic()
            .wrap_err("link the global package install directory")?;

        link_bins_of_packages_with_excludes::<CmdShimHost>(&pkgs, &global_bin_dir, &bins_to_skip)
            .map_err(miette::Report::new)
            .wrap_err("link global package bins")?;
    }
    Ok(())
}

/// `pnpm update -g`. Reinstalls each matching group (within its existing
/// range, or to `--latest`), then swaps its hash symlink to the new dir.
pub async fn handle_global_update<Reporter: self::Reporter + 'static>(
    base_config: &'static Config,
    params: &[String],
    latest: bool,
    pinned_version: PinnedVersion,
    supported_architectures: Option<SupportedArchitectures>,
) -> miette::Result<()> {
    let (global_pkg_dir, global_bin_dir) = global_dirs(base_config)?;
    check_bin_dir(&global_bin_dir)?;
    clean_orphaned_install_dirs(&global_pkg_dir);

    let all = scan_global_packages(&global_pkg_dir);
    if all.is_empty() {
        println!("No global packages found");
        return Ok(());
    }
    let to_update: Vec<GlobalPackageInfo> = if params.is_empty() {
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

    for pkg in &to_update {
        let selectors: Vec<String> = pkg
            .dependencies
            .iter()
            .map(|(alias, spec)| if latest { alias.clone() } else { format!("{alias}@{spec}") })
            .collect();
        let (install_dir, config) = Box::pin(run_group_install::<Reporter>(
            base_config,
            &global_pkg_dir,
            &selectors,
            pinned_version,
            supported_architectures.clone(),
        ))
        .await?;
        let _ = config;

        let pkgs = read_installed_packages(&install_dir);
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

        // Remove stale bins from the old install before swapping, but keep
        // any bin owned by a different global group.
        let protected =
            bin_names_of_other_groups(&global_pkg_dir, &HashSet::from([pkg.hash.clone()]));
        for bin in get_installed_bin_names(pkg) {
            if protected.contains(&bin) {
                continue;
            }
            let _ = remove_bin(&global_bin_dir.join(&bin));
        }

        let hash_link = get_hash_link(&global_pkg_dir, &pkg.hash);
        force_symlink_dir(&install_dir, &hash_link)
            .into_diagnostic()
            .wrap_err("swap the global package install directory")?;
        if is_subdir(&global_pkg_dir, &pkg.install_dir) {
            let _ = fs::remove_dir_all(&pkg.install_dir);
        }

        link_bins_of_packages_with_excludes::<CmdShimHost>(&pkgs, &global_bin_dir, &bins_to_skip)
            .map_err(miette::Report::new)
            .wrap_err("link global package bins")?;
    }
    Ok(())
}

/// `pnpm remove -g`. Removes the bins, hash symlinks, and install dirs of
/// every group that contains one of the requested packages.
pub fn handle_global_remove(base_config: &'static Config, params: &[String]) -> miette::Result<()> {
    let (global_pkg_dir, global_bin_dir) = global_dirs(base_config)?;
    check_bin_dir(&global_bin_dir)?;

    let mut groups: Vec<GlobalPackageInfo> = Vec::new();
    let mut seen = HashSet::new();
    for param in params {
        let Some(pkg) = find_global_package(&global_pkg_dir, param) else {
            return Err(GlobalError::PkgNotFound { param: param.clone() }.into());
        };
        if seen.insert(pkg.hash.clone()) {
            groups.push(pkg);
        }
    }

    // Bins shared with (and owned by) groups that survive this removal must
    // not be unlinked, or we'd delete another global package's bin.
    let exclude: HashSet<String> = groups.iter().map(|pkg| pkg.hash.clone()).collect();
    let protected = bin_names_of_other_groups(&global_pkg_dir, &exclude);

    for pkg in &groups {
        remove_group(&global_pkg_dir, &global_bin_dir, pkg, &protected);
    }
    Ok(())
}

/// Install `selectors` into a fresh group directory under `global_pkg_dir`,
/// returning that directory and the leaked per-group [`Config`] (anchored
/// there, saving to `dependencies`). Then run the global build-approval
/// flow. Shared by add and update.
async fn run_group_install<Reporter: self::Reporter + 'static>(
    base_config: &Config,
    global_pkg_dir: &Path,
    selectors: &[String],
    pinned_version: PinnedVersion,
    supported_architectures: Option<SupportedArchitectures>,
) -> miette::Result<(PathBuf, &'static Config)> {
    let install_dir = create_install_dir(global_pkg_dir)
        .into_diagnostic()
        .wrap_err("create global install dir")?;

    let mut cfg = base_config.clone();
    cfg.modules_dir = install_dir.join("node_modules");
    cfg.virtual_store_dir = install_dir.join("node_modules").join(".pnpm");
    // Each global group is self-contained, so the virtual store lives
    // inside its install dir (never the shared global one).
    cfg.enable_global_virtual_store = false;
    // Persist a `pnpm-lock.yaml` in the group's install dir (pnpm sets
    // `lockfileDir = installDir`). `outdated -g` / `update -g` read these
    // pins to determine the currently-installed versions.
    cfg.lockfile = true;
    cfg.workspace_dir = None;
    cfg.supported_architectures = supported_architectures;

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
            cfg.allow_builds = allow_builds;
        }
        if let Some(allow_all) = settings.dangerously_allow_all_builds {
            cfg.dangerously_allow_all_builds = allow_all;
        }
    }
    // Don't fail the install when a dependency's build is ignored; the
    // global approval prompt (run after the install) handles it. Mirrors
    // pnpm's global flow, which records ignored builds and prompts rather
    // than erroring under `strictDepBuilds`.
    cfg.strict_dep_builds = false;

    let config: &'static Config = Config::leak(cfg);

    let manifest_path = install_dir.join("package.json");
    for selector in selectors {
        let state = State::init(manifest_path.clone(), config, false)
            .wrap_err("initialize the global install state")?;
        add_package::<Reporter, _, _>(
            state,
            selector,
            pinned_version,
            None,
            false,
            config.supported_architectures.clone(),
            || std::iter::once(DependencyGroup::Prod),
        )
        .await?;
    }

    prompt_approve_global_builds::<Reporter>(config, &install_dir, global_pkg_dir).await?;
    Ok((install_dir, config))
}

/// Run the interactive build-approval flow against the just-installed
/// group, mirroring pnpm's `promptApproveGlobalBuilds`. No-op when nothing
/// is awaiting approval, or when stdin is not a TTY (unless the test
/// auto-approve env var is set).
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
    let config_fn = || -> miette::Result<&'static mut Config> { Ok(Config::leak(config.clone())) };
    let state_fn = |require_lockfile: bool| -> miette::Result<State> {
        State::init(manifest_path.clone(), Config::leak(config.clone()), require_lockfile)
            .wrap_err("initialize the global approve-builds state")
    };

    let args = ApproveBuildsArgs { packages: Vec::new(), all: auto_approve, global: false };
    if let Some((rebuild_state, build_packages)) =
        args.prepare(global_pkg_dir, &config_fn, &state_fn)?
    {
        run_rebuild::<Reporter>(&rebuild_state, Some(build_packages)).await?;
    }
    Ok(())
}

/// Remove any existing global installs of `aliases` before linking the new
/// group, deduplicated by hash. Mirrors pnpm's `removeExistingGlobalInstalls`.
fn remove_existing_global_installs(
    global_pkg_dir: &Path,
    global_bin_dir: &Path,
    aliases: &[String],
) {
    let mut to_remove: Vec<GlobalPackageInfo> = Vec::new();
    let mut seen = HashSet::new();
    for alias in aliases {
        if let Some(pkg) = find_global_package(global_pkg_dir, alias)
            && seen.insert(pkg.hash.clone())
        {
            to_remove.push(pkg);
        }
    }
    // Bins owned by groups that survive this replacement must not be
    // unlinked, or we'd delete a different global package's bin.
    let exclude: HashSet<String> = to_remove.iter().map(|pkg| pkg.hash.clone()).collect();
    let protected = bin_names_of_other_groups(global_pkg_dir, &exclude);
    for pkg in &to_remove {
        remove_group(global_pkg_dir, global_bin_dir, pkg, &protected);
    }
}

/// Remove a group's bins (except those in `protected`, owned by a surviving
/// group), its hash symlink, and its install dir.
fn remove_group(
    global_pkg_dir: &Path,
    global_bin_dir: &Path,
    pkg: &GlobalPackageInfo,
    protected: &HashSet<String>,
) {
    for bin in get_installed_bin_names(pkg) {
        if protected.contains(&bin) {
            continue;
        }
        let _ = remove_bin(&global_bin_dir.join(&bin));
    }
    let _ = fs::remove_file(get_hash_link(global_pkg_dir, &pkg.hash));
    if is_subdir(global_pkg_dir, &pkg.install_dir) {
        let _ = fs::remove_dir_all(&pkg.install_dir);
    }
}

/// The set of bin names provided by global package groups other than those
/// in `exclude_hashes`. Mirrors pnpm's `getBinNamesOfOtherGroups`.
fn bin_names_of_other_groups(
    global_pkg_dir: &Path,
    exclude_hashes: &HashSet<String>,
) -> HashSet<String> {
    let mut names = HashSet::new();
    for pkg in scan_global_packages(global_pkg_dir) {
        if exclude_hashes.contains(&pkg.hash) {
            continue;
        }
        for bin in get_installed_bin_names(&pkg) {
            names.insert(bin);
        }
    }
    names
}

/// The direct-dependency aliases of an install directory's `package.json`.
fn read_aliases(install_dir: &Path) -> Vec<String> {
    let Ok(text) = fs::read_to_string(install_dir.join("package.json")) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<Value>(&text) else {
        return Vec::new();
    };
    value
        .get("dependencies")
        .and_then(Value::as_object)
        .map(|deps| deps.keys().cloned().collect())
        .unwrap_or_default()
}

/// Build the registry map (`{ default, ...scoped }`) pnpm hashes into the
/// global cache key.
fn registries_with_default(config: &Config) -> Vec<(String, String)> {
    let mut registries = vec![("default".to_string(), config.registry.clone())];
    registries.extend(config.registries.iter().map(|(key, value)| (key.clone(), value.clone())));
    registries
}

// --- param grouping (port of globalAdd's split/resolve helpers) -----------

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

fn is_windows_drive_path(param: &str) -> bool {
    let bytes = param.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'/' || bytes[2] == b'\\')
}

#[cfg(test)]
mod tests;
