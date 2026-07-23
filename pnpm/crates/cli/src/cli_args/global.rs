//! Global package install command handlers (`add -g`, `update -g`,
//! `remove -g`).
//!
//! Each space-separated CLI param is its own isolated install group (a
//! comma splits a group; local paths / URLs are kept whole). A group
//! installs into a fresh directory under the global packages dir, then a
//! hash symlink and the global bins are pointed at it.

use crate::{
    State,
    cli_args::{
        add::add_packages, approve_builds::ApproveBuildsArgs,
        ignored_builds::get_automatically_ignored_builds, rebuild::run_rebuild,
    },
};
use derive_more::{Display, Error};
use miette::{Context, Diagnostic, IntoDiagnostic};
use pacquet_cmd_shim::{Host as CmdShimHost, link_bins_of_packages_with_excludes, remove_bin};
use pacquet_config::{CatalogMode, Config, WorkspaceSettings, check_global_bin_dir};
use pacquet_fs::{force_symlink_dir, is_subdir, lexical_normalize};
use pacquet_global::{
    GlobalPackageInfo, check_global_bin_conflicts, clean_orphaned_install_dirs,
    create_global_cache_key, create_install_dir, find_global_package, get_hash_link,
    get_installed_bin_names, read_direct_dependency_aliases, read_installed_packages,
    scan_global_packages,
};
use pacquet_package_is_installable::SupportedArchitectures;
use pacquet_package_manifest::{DependencyGroup, safe_read_package_json_from_dir};
use pacquet_registry::PinnedVersion;
use pacquet_reporter::Reporter;
use pacquet_resolving_parse_wanted_dependency::{
    is_valid_old_npm_package_name, parse_wanted_dependency,
};
use std::{
    collections::HashSet,
    fs,
    io::IsTerminal,
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

    #[display(r#"Use the "pnpm self-update" command to install or update pnpm"#)]
    #[diagnostic(code(ERR_PNPM_GLOBAL_PNPM_INSTALL))]
    GlobalPnpmInstall,

    #[display("Cannot remove '{param}': not found in global packages")]
    #[diagnostic(code(ERR_PNPM_GLOBAL_PKG_NOT_FOUND))]
    PkgNotFound { param: String },

    #[display(r#"Invalid package name "{name}"."#)]
    #[diagnostic(code(ERR_PNPM_INVALID_PACKAGE_NAME))]
    InvalidPackageName { name: String },
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
        let aliases = read_direct_dependency_aliases(&install_dir);
        let aliases_to_replace = replacement_aliases(&aliases);

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

        remove_existing_global_installs(
            &global_pkg_dir,
            &global_bin_dir,
            &aliases,
            &aliases_to_replace,
        )
        .into_diagnostic()
        .wrap_err("remove existing global installs")?;

        let cache_hash = create_global_cache_key(&aliases, &registries_with_default(config));
        let hash_link = get_hash_link(&global_pkg_dir, &cache_hash);
        force_symlink_dir(&install_dir, &hash_link)
            .into_diagnostic()
            .wrap_err("link the global package install directory")?;

        link_bins_of_packages_with_excludes::<CmdShimHost>(
            &pkgs,
            &global_bin_dir,
            &bins_to_skip,
            &[],
        )
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

    let all =
        scan_global_packages(&global_pkg_dir).into_diagnostic().wrap_err("scan global packages")?;
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
            bin_names_of_other_groups(&global_pkg_dir, &HashSet::from([pkg.hash.clone()]))
                .into_diagnostic()
                .wrap_err("scan global packages")?;
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

        link_bins_of_packages_with_excludes::<CmdShimHost>(
            &pkgs,
            &global_bin_dir,
            &bins_to_skip,
            &[],
        )
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
    // Pin the group's workspace root to its own install dir (pnpm's
    // `rootProjectManifestDir: installDir`, `workspaceDir: undefined`). The
    // install dir sits *under* the global packages dir, which carries a
    // `pnpm-workspace.yaml` of global settings (`allowBuilds`, `catalog`,
    // ...). Leaving this unset would let the install pipeline walk up, adopt
    // that file as the workspace, and then fail trying to enumerate its
    // non-existent root project. Anchoring here keeps the group install an
    // isolated single project.
    cfg.workspace_dir = Some(install_dir.clone());
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
    // global approval prompt (run after the install) records the ignored
    // builds and prompts rather than erroring under `strictDepBuilds`.
    cfg.strict_dep_builds = false;

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
        pinned_version,
        None,
        false,
        config.supported_architectures.clone(),
        Some([DependencyGroup::Prod]),
    )
    .await?;

    prompt_approve_global_builds::<Reporter>(config, &install_dir, global_pkg_dir).await?;
    Ok((install_dir, config))
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

/// Remove any existing global installs of `aliases` before linking the new
/// group, deduplicated by hash.
fn remove_existing_global_installs(
    global_pkg_dir: &Path,
    global_bin_dir: &Path,
    aliases: &[String],
    aliases_to_replace: &[String],
) -> std::io::Result<()> {
    let mut to_remove: Vec<GlobalPackageInfo> = Vec::new();
    let mut seen = HashSet::new();
    for alias in aliases_to_replace {
        if let Some(pkg) = find_global_package(global_pkg_dir, alias)?
            && should_replace_existing_package(&pkg, aliases, aliases_to_replace)
            && seen.insert(pkg.hash.clone())
        {
            to_remove.push(pkg);
        }
    }
    // Bins owned by groups that survive this replacement must not be
    // unlinked, or we'd delete a different global package's bin.
    let exclude: HashSet<String> = to_remove.iter().map(|pkg| pkg.hash.clone()).collect();
    let protected = bin_names_of_other_groups(global_pkg_dir, &exclude)?;
    for pkg in &to_remove {
        remove_group(global_pkg_dir, global_bin_dir, pkg, &protected);
    }
    Ok(())
}

fn replacement_aliases(aliases: &[String]) -> Vec<String> {
    const PNPM_CLI_PACKAGE_ALIASES: [&str; 2] = ["pnpm", "@pnpm/exe"];

    let mut expanded = aliases.to_vec();
    if aliases.iter().any(|alias| is_pnpm_cli_package_alias(alias)) {
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

fn is_pnpm_cli_only_group(pkg: &GlobalPackageInfo) -> bool {
    !pkg.dependencies.is_empty()
        && pkg.dependencies.iter().all(|(alias, _)| is_pnpm_cli_package_alias(alias))
}

fn is_pnpm_cli_package_alias(alias: &str) -> bool {
    matches!(alias, "pnpm" | "@pnpm/exe")
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

/// Build the registry map (`{ default, ...scoped }`) hashed into the
/// global cache key.
fn registries_with_default(config: &Config) -> Vec<(String, String)> {
    let mut registries = vec![("default".to_string(), config.registry.clone())];
    registries.extend(config.registries.iter().map(|(key, value)| (key.clone(), value.clone())));
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
