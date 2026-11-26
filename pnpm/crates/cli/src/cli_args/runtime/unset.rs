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
            matches_node_version,
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
use pnpm_engine_runtime_node_resolver::{
    RELEASE_CHANNELS,
    strip_leading_v,
};
use pnpm_global::{
    GlobalPackageInfo,
    installed_versions,
    scan_global_packages,
};
use pnpm_package_manifest::DependencyGroup;
use pnpm_reporter::Reporter;
use pnpm_tool_protocol::{
    is_tool_specifier,
    strip_tool_protocol,
};
use std::{
    collections::HashSet,
    path::Path,
};

pub fn run_unset<Reporter: self::Reporter>(
    request: &RuntimeUnsetRequest,
    config: &'static Config,
) -> miette::Result<()> {
    let version = request.version_spec.as_deref();
    let global_bin_dir = config.global_bin.as_deref().filter(|dir| dir.is_dir());
    let unset = || {
        let mut removed_names = HashSet::new();
        let removed_global = remove_global_runtimes::<Reporter>(
            config,
            global_bin_dir,
            &request.runtime_name,
            version,
        )?;
        let removed_legacy =
            cleanup_legacy_runtime_artifacts(request, global_bin_dir, &mut removed_names)?;
        if removed_global || removed_legacy {
            return Ok(());
        }
        let runtime_name = &request.runtime_name;
        let param = match version {
            Some(version) => format!("{runtime_name} (version {version})"),
            None => runtime_name.clone(),
        };
        Err(GlobalError::PkgNotFound { param }.into())
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
    let version = request.version_spec.as_deref();
    let matches_legacy_version = |name: &str| {
        version.is_none_or(|requested| runtime_version_matches("node", name, requested))
    };
    let pnpm_home = pnpm_config::default_pnpm_home_dir::<pnpm_config::Host>();
    let mut removed = cleanup_pnpm_home_without_stale_current(
        pnpm_home.as_deref(),
        &matches_legacy_version,
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
/// remove it from every field. Without `--save-dev` or `--save-prod`, every
/// matching declaration in `devEngines.runtime` and `engines.runtime` is
/// removed.
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
        .filter_map(|group| {
            declared_runtime_version(state, group, &request.runtime_name)
                .map(|version| (group, version))
        })
        .collect::<Vec<_>>();
    if declarations.is_empty() {
        return Err(
            RuntimeError::NotARuntimeDependency { name: request.runtime_name.clone() }.into()
        );
    }
    let matching = declarations
        .into_iter()
        .filter(|(_, declared_version)| {
            request.version_spec
                .as_deref()
                .is_none_or(|version| {
                    runtime_version_matches(&request.runtime_name, declared_version, version)
                })
        })
        .map(|(group, _)| group)
        .collect::<Vec<_>>();
    match matching.as_slice() {
        [] => Err(RuntimeError::RuntimeNotDeclared {
            name: request.runtime_name.clone(),
            version: request.version_spec.clone().unwrap_or_default(),
        }
        .into()),
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

fn declared_runtime_version<'state>(
    state: &'state State,
    dependency_group: DependencyGroup,
    runtime_name: &str,
) -> Option<&'state str> {
    let dependencies_field: &str = dependency_group.into();
    state.manifest
        .value()
        .get(dependencies_field)?
        .get(runtime_name)?
        .as_str()
        .and_then(strip_tool_protocol)
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
    version: Option<&str>,
) -> miette::Result<bool> {
    let Some(global_pkg_dir) = config.global_pkg_dir.as_deref() else {
        return Ok(false);
    };
    let packages = select_global_runtimes(global_pkg_dir, runtime_name, version)?;
    if packages.is_empty() {
        return Ok(false);
    }
    remove_selected_global_runtimes::<Reporter>(global_pkg_dir, global_bin_dir, &packages)?;
    Ok(true)
}

fn select_global_runtimes(
    global_pkg_dir: &Path,
    runtime_name: &str,
    version: Option<&str>,
) -> miette::Result<Vec<GlobalPackageInfo>> {
    let packages = scan_global_packages(global_pkg_dir)
        .into_diagnostic()?
        .into_iter()
        .filter(|pkg| is_runtime_dependency(pkg, runtime_name))
        .filter(|pkg| {
            version.is_none_or(|requested| {
                installed_versions(&pkg.install_dir)
                    .get(runtime_name)
                    .is_some_and(|installed| {
                        runtime_version_matches(runtime_name, installed, requested)
                    })
            })
        })
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

fn runtime_version_matches(runtime_name: &str, actual: &str, requested: &str) -> bool {
    let actual = actual.trim();
    let selector = actual
        .strip_prefix(['^', '~'])
        .unwrap_or(actual);
    runtime_selector_matches(runtime_name, selector.trim(), requested)
}

fn runtime_selector_matches(runtime_name: &str, actual: &str, requested: &str) -> bool {
    if runtime_name == "node" {
        return matches_node_selector(actual, requested);
    }
    let requested = strip_leading_v(requested.trim());
    if requested.is_empty() {
        return false;
    }
    let actual = strip_leading_v(actual.trim());
    actual == requested || actual.starts_with(&format!("{requested}."))
}

fn matches_node_selector(actual: &str, requested: &str) -> bool {
    if let Some((channel, version)) = actual.split_once('/') {
        return matches_node_channel_version(channel, version, requested);
    }
    matches_node_version(strip_leading_v(actual), requested)
}

fn matches_node_channel_version(channel: &str, version: &str, requested: &str) -> bool {
    if !RELEASE_CHANNELS.contains(&channel) {
        return false;
    }
    let (requested_channel, raw_version) = match requested.split_once('/') {
        Some((requested_channel, rest)) if RELEASE_CHANNELS.contains(&requested_channel) => {
            (Some(requested_channel), rest)
        }
        Some(_) => return false,
        None => (None, requested),
    };
    if requested_channel.is_some_and(|requested| requested != channel) {
        return false;
    }
    let requested = strip_leading_v(raw_version.trim());
    let version = strip_leading_v(version.trim());
    !requested.is_empty() && (version == requested || version.starts_with(&format!("{requested}.")))
}

#[cfg(test)]
mod tests;
