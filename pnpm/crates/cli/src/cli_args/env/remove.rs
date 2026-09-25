use crate::{
    cli_args::global::handle_global_remove,
    shim_dispatch::{ShimTarget, native_shim_target, remove_native_shim},
};
use miette::IntoDiagnostic;
use pnpm_cmd_shim::remove_bin as remove_cmd_shim;
use pnpm_config::Config;
use pnpm_global::{find_global_package, is_global_install_subdir};
use pnpm_reporter::Reporter;
use std::{collections::HashSet, fs, path::Path};

pub async fn run_remove<Reporter: self::Reporter + 'static>(
    versions: Vec<String>,
    config: &'static Config,
    _dir: &Path,
) -> miette::Result<()> {
    let mut removed_something = false;

    if remove_matching_global_node::<Reporter>(config, &versions)? {
        removed_something = true;
    }

    let mut removed_names = HashSet::new();
    let pnpm_home = pnpm_config::default_pnpm_home_dir::<pnpm_config::Host>();

    if cleanup_pnpm_home(pnpm_home.as_deref(), &versions, &mut removed_names).into_diagnostic()? {
        removed_something = true;
    }

    if let Some(global_bin_dir) = config.global_bin.as_deref()
        && cleanup_bin_links(global_bin_dir, &removed_names).into_diagnostic()?
    {
        removed_something = true;
    }

    if !removed_something {
        return Err(crate::cli_args::global::GlobalError::PkgNotFound {
            param: format!("node (version {})", versions.join(", ")),
        }
        .into());
    }

    Ok(())
}

fn matches_node_version(actual: &str, requested: &str) -> bool {
    actual == requested || actual.starts_with(&format!("{requested}."))
}

/// Remove a global `node` package when its installed version matches.
pub fn remove_matching_global_node<Reporter: self::Reporter + 'static>(
    config: &'static Config,
    versions: &[String],
) -> miette::Result<bool> {
    let Some(global_pkg_dir) = config.global_pkg_dir.as_deref() else {
        return Ok(false);
    };
    let Some(pkg) = find_global_package(global_pkg_dir, "node").into_diagnostic()? else {
        return Ok(false);
    };
    let installed = pnpm_global::installed_versions(&pkg.install_dir);
    let Some(installed_ver) = installed.get("node") else {
        return Ok(false);
    };
    if !versions.iter().any(|target_version| matches_node_version(installed_ver, target_version)) {
        return Ok(false);
    }
    if config.global_bin.is_some() {
        handle_global_remove::<Reporter>(config, &["node".to_string()])?;
    } else {
        remove_global_node_install(global_pkg_dir, &pkg).into_diagnostic()?;
    }
    Ok(true)
}

fn remove_global_node_install(
    global_pkg_dir: &Path,
    pkg: &pnpm_global::GlobalPackageInfo,
) -> std::io::Result<()> {
    let hash_link = pnpm_global::get_hash_link(global_pkg_dir, &pkg.hash);
    match pnpm_fs::remove_symlink_dir(&hash_link) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(err),
    }
    if is_global_install_subdir(global_pkg_dir, &pkg.install_dir) {
        match fs::remove_dir_all(&pkg.install_dir) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(err),
        }
    }
    Ok(())
}

fn cleanup_legacy_nodejs_dir(
    nodejs_dir: &Path,
    versions: &[String],
    removed_names: &mut HashSet<String>,
) -> std::io::Result<bool> {
    let entries = match fs::read_dir(nodejs_dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(err),
    };
    let mut removed = false;
    for entry in entries.flatten() {
        let name = entry
            .file_name()
            .to_string_lossy()
            .into_owned();
        if !versions.iter().any(|target_version| matches_node_version(&name, target_version)) {
            continue;
        }
        fs::remove_dir_all(entry.path())?;
        removed_names.insert(name);
        removed = true;
    }
    Ok(removed)
}

fn is_path_in_removed_names(target: &Path, removed_names: &HashSet<String>) -> bool {
    let target_str = target.to_string_lossy();
    removed_names
        .iter()
        .any(|name| {
            target_str
                .split(['/', '\\'])
                .any(|part| part == name)
        })
}

fn cleanup_nodejs_current(
    pnpm_home: &Path,
    removed_names: &HashSet<String>,
) -> std::io::Result<bool> {
    let nodejs_current = pnpm_home.join("nodejs_current");
    if !nodejs_current.is_symlink() {
        return Ok(false);
    }
    let is_dangling = !nodejs_current.exists();
    let points_to_removed = fs::read_link(&nodejs_current)
        .is_ok_and(|target| is_path_in_removed_names(&target, removed_names));
    if is_dangling || points_to_removed {
        fs::remove_file(&nodejs_current)?;
        return Ok(true);
    }
    Ok(false)
}

fn is_symlink_removed(bin_path: &Path, removed_names: &HashSet<String>) -> std::io::Result<bool> {
    if !bin_path.is_symlink() {
        return Ok(false);
    }
    if !bin_path.exists() {
        return Ok(true);
    }
    let target = fs::read_link(bin_path)?;
    Ok(is_path_in_removed_names(&target, removed_names))
}

fn is_native_shim_removed(
    global_bin_dir: &Path,
    bin_name: &str,
    removed_names: &HashSet<String>,
) -> std::io::Result<bool> {
    match native_shim_target(global_bin_dir, bin_name) {
        Ok(Some(ShimTarget::Installed(target))) => {
            Ok(!target.exists() || is_path_in_removed_names(&target, removed_names))
        }
        Ok(None | Some(ShimTarget::Virtual(_))) => Ok(false),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err),
    }
}

fn is_cmd_shim_removed(
    global_bin_dir: &Path,
    bin_name: &str,
    removed_names: &HashSet<String>,
) -> std::io::Result<bool> {
    for ext in [".cmd", ".ps1"] {
        let shim = global_bin_dir.join(format!("{bin_name}{ext}"));
        let content = match fs::read_to_string(&shim) {
            Ok(content) => content,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => return Err(err),
        };
        let points_to_removed = content
            .split(['"', '\'', '\\', '/', ' ', '\t', '\r', '\n'])
            .any(|segment| removed_names.contains(segment));
        if points_to_removed {
            return Ok(true);
        }
    }
    Ok(false)
}

fn cleanup_single_bin_link(
    global_bin_dir: &Path,
    bin_name: &str,
    removed_names: &HashSet<String>,
) -> std::io::Result<bool> {
    let bin_path = global_bin_dir.join(bin_name);
    let should_remove = is_symlink_removed(&bin_path, removed_names)?
        || is_native_shim_removed(global_bin_dir, bin_name, removed_names)?
        || is_cmd_shim_removed(global_bin_dir, bin_name, removed_names)?;

    if should_remove {
        remove_cmd_shim(&bin_path)?;
        remove_native_shim(global_bin_dir, bin_name)?;
        for ext in [".cmd", ".ps1"] {
            let shim = global_bin_dir.join(format!("{bin_name}{ext}"));
            if shim.exists() {
                fs::remove_file(&shim)?;
            }
        }
        if bin_path.exists() || bin_path.is_symlink() {
            fs::remove_file(&bin_path)?;
        }
        return Ok(true);
    }
    Ok(false)
}

fn cleanup_bin_links(
    global_bin_dir: &Path,
    removed_names: &HashSet<String>,
) -> std::io::Result<bool> {
    let mut removed = false;
    for bin_name in ["node", "npm", "npx"] {
        if cleanup_single_bin_link(global_bin_dir, bin_name, removed_names)? {
            removed = true;
        }
    }
    Ok(removed)
}

fn cleanup_pnpm_home(
    pnpm_home: Option<&Path>,
    versions: &[String],
    removed_names: &mut HashSet<String>,
) -> std::io::Result<bool> {
    let Some(pnpm_home) = pnpm_home else {
        return Ok(false);
    };
    let dir_removed =
        cleanup_legacy_nodejs_dir(&pnpm_home.join("nodejs"), versions, removed_names)?;
    let link_removed = cleanup_nodejs_current(pnpm_home, removed_names)?;
    Ok(dir_removed || link_removed)
}
