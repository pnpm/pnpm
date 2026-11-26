use super::{
    RuntimeError,
    RuntimeUnsetRequest,
};
use crate::{
    State,
    cli_args::{
        env::{
            cleanup_bin_links_for_removed_targets,
            cleanup_pnpm_home_without_stale_current,
            remove_global_node_install,
        },
        global::{
            GlobalError,
            remove_global_package_groups_locked,
        },
        global_bin_lock::acquire_global_bin_lock,
    },
};
use miette::IntoDiagnostic;
use pnpm_config::Config;
use pnpm_global::{
    GlobalPackageInfo,
    scan_global_packages,
};
use pnpm_package_manifest::DependencyGroup;
use pnpm_reporter::Reporter;
use pnpm_tool_protocol::is_tool_specifier;
use std::{
    collections::HashSet,
    path::Path,
};

pub fn run_unset<Reporter: self::Reporter>(
    request: &RuntimeUnsetRequest,
    config: &'static Config,
) -> miette::Result<()> {
    let global_bin_dir = config.global_bin.as_deref().filter(|dir| dir.is_dir());
    let unset = || {
        let mut removed_names = HashSet::new();
        let removed_global =
            remove_global_runtimes::<Reporter>(config, global_bin_dir, &request.runtime_name)?;
        let removed_legacy =
            cleanup_legacy_runtime_artifacts(request, global_bin_dir, &mut removed_names)?;
        if removed_global || removed_legacy {
            return Ok(());
        }
        Err(GlobalError::PkgNotFound { param: request.runtime_name.clone() }.into())
    };

    if let Some(global_bin_dir) = global_bin_dir {
        with_global_bin_lock(global_bin_dir, unset)
    } else {
        unset()
    }
}

fn cleanup_legacy_runtime_artifacts(
    request: &RuntimeUnsetRequest,
    global_bin_dir: Option<&Path>,
    removed_names: &mut HashSet<String>,
) -> miette::Result<bool> {
    if request.runtime_name != "node" {
        return Ok(false);
    }
    let pnpm_home = pnpm_config::default_pnpm_home_dir::<pnpm_config::Host>();
    let mut removed = cleanup_pnpm_home_without_stale_current(
        pnpm_home.as_deref(),
        &|_: &str| true,
        removed_names,
    )
    .into_diagnostic()?;
    if removed
        && !removed_names.is_empty()
        && let Some(global_bin_dir) = global_bin_dir
    {
        removed |= cleanup_bin_links_for_removed_targets(
            global_bin_dir,
            &["node", "npm", "npx"],
            removed_names,
        )
        .into_diagnostic()?;
    }
    Ok(removed)
}

/// The dependency group to remove the requested runtime from, or `None` to
/// remove it from every field. Without `--save-dev` or `--save-prod`, the
/// runtime is removed from both `devEngines.runtime` and `engines.runtime`.
pub fn ensure_project_runtime_matches(
    state: &State,
    request: &RuntimeUnsetRequest,
) -> miette::Result<Option<DependencyGroup>> {
    let candidates = match request.dependency_group {
        Some(group) => vec![group],
        None => vec![DependencyGroup::Dev, DependencyGroup::Prod],
    };
    let declarations = candidates
        .into_iter()
        .filter(|&group| declares_runtime(state, group, &request.runtime_name))
        .collect::<Vec<_>>();
    match declarations.as_slice() {
        [] => {
            Err(RuntimeError::NotARuntimeDependency { name: request.runtime_name.clone() }.into())
        }
        [group] => Ok(Some(*group)),
        // An unflagged removal also clears `optionalDependencies`.
        _ if declares(state, DependencyGroup::Optional, &request.runtime_name) => {
            Err(RuntimeError::AmbiguousUnset { name: request.runtime_name.clone() }.into())
        }
        _ => Ok(None),
    }
}

fn declares(state: &State, dependency_group: DependencyGroup, name: &str) -> bool {
    let dependencies_field: &str = dependency_group.into();
    state.manifest
        .value()
        .get(dependencies_field)
        .and_then(|dependencies| dependencies.get(name))
        .is_some()
}

fn declares_runtime(state: &State, dependency_group: DependencyGroup, runtime_name: &str) -> bool {
    let dependencies_field: &str = dependency_group.into();
    state.manifest
        .value()
        .get(dependencies_field)
        .and_then(|dependencies| dependencies.get(runtime_name))
        .and_then(|specifier| specifier.as_str())
        .is_some_and(is_tool_specifier)
}

fn with_global_bin_lock<Output>(
    global_bin_dir: &Path,
    action: impl FnOnce() -> miette::Result<Output>,
) -> miette::Result<Output> {
    let _lock = acquire_global_bin_lock(global_bin_dir)?;
    action()
}

fn remove_global_runtimes<Reporter: self::Reporter>(
    config: &Config,
    global_bin_dir: Option<&Path>,
    runtime_name: &str,
) -> miette::Result<bool> {
    let Some(global_pkg_dir) = config.global_pkg_dir.as_deref() else {
        return Ok(false);
    };
    let packages = select_global_runtimes(global_pkg_dir, runtime_name)?;
    if packages.is_empty() {
        return Ok(false);
    }
    remove_selected_global_runtimes::<Reporter>(global_pkg_dir, global_bin_dir, &packages)?;
    Ok(true)
}

fn select_global_runtimes(
    global_pkg_dir: &Path,
    runtime_name: &str,
) -> miette::Result<Vec<GlobalPackageInfo>> {
    let packages = scan_global_packages(global_pkg_dir)
        .into_diagnostic()?
        .into_iter()
        .filter(|pkg| is_runtime_dependency(pkg, runtime_name))
        .collect::<Vec<GlobalPackageInfo>>();
    Ok(packages)
}

fn remove_selected_global_runtimes<Reporter: self::Reporter>(
    global_pkg_dir: &Path,
    global_bin_dir: Option<&Path>,
    packages: &[GlobalPackageInfo],
) -> miette::Result<()> {
    if let Some(global_bin_dir) = global_bin_dir {
        return remove_global_package_groups_locked::<Reporter>(
            global_pkg_dir,
            global_bin_dir,
            packages,
        );
    }
    for pkg in packages {
        remove_global_node_install(global_pkg_dir, pkg).into_diagnostic()?;
    }
    Ok(())
}

fn is_runtime_dependency(pkg: &GlobalPackageInfo, runtime_name: &str) -> bool {
    pkg.dependencies
        .iter()
        .any(|(alias, specifier)| alias == runtime_name && is_tool_specifier(specifier))
}

#[cfg(test)]
mod tests;
