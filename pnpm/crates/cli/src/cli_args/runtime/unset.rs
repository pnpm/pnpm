use super::{
    RuntimeError,
    RuntimeUnsetRequest,
};
use crate::{
    State,
    cli_args::{
        env::{
            cleanup_bin_links,
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
use pnpm_tool_protocol::strip_tool_protocol;
use std::{
    collections::HashSet,
    path::Path,
};

pub fn run_unset<Reporter: self::Reporter>(
    request: &RuntimeUnsetRequest,
    config: &'static Config,
) -> miette::Result<()> {
    let version = request.version_spec.as_deref();
    let mut removed_names = HashSet::new();
    let removed_global = remove_global_runtimes::<Reporter>(
        config,
        &request.runtime_name,
        version,
        &mut removed_names,
    )?;
    let removed_legacy =
        cleanup_legacy_runtime_artifacts(request, config, removed_global, &mut removed_names)?;
    if removed_global || removed_legacy {
        return Ok(());
    }
    let runtime_name = &request.runtime_name;
    let param = match version {
        Some(version) => format!("{runtime_name} (version {version})"),
        None => runtime_name.clone(),
    };
    Err(GlobalError::PkgNotFound { param }.into())
}

fn cleanup_legacy_runtime_artifacts(
    request: &RuntimeUnsetRequest,
    config: &'static Config,
    removed_global: bool,
    removed_names: &mut HashSet<String>,
) -> miette::Result<bool> {
    let version = request.version_spec.as_deref();
    let mut removed = false;
    if request.runtime_name == "node" {
        let matches_legacy_version = |name: &str| {
            version.is_none_or(|requested| runtime_version_matches("node", name, requested))
        };
        let pnpm_home = pnpm_config::default_pnpm_home_dir::<pnpm_config::Host>();
        removed = cleanup_pnpm_home_without_stale_current(
            pnpm_home.as_deref(),
            &matches_legacy_version,
            removed_names,
        )
        .into_diagnostic()?;
    }
    if (removed_global || removed)
        && let Some(global_bin_dir) = config.global_bin.as_deref().filter(|dir| dir.is_dir())
    {
        let runtime_name = request.runtime_name.as_str();
        let bin_names: &[&str] = if runtime_name == "node" {
            &["node", "npm", "npx"]
        } else {
            std::slice::from_ref(&runtime_name)
        };
        removed |= cleanup_bin_links(global_bin_dir, bin_names, removed_names).into_diagnostic()?;
    }
    Ok(removed)
}

pub fn ensure_project_runtime_matches(
    state: &State,
    request: &RuntimeUnsetRequest,
) -> miette::Result<()> {
    let dependencies_field = match request.dependency_group {
        DependencyGroup::Prod => "dependencies",
        DependencyGroup::Dev => "devDependencies",
        DependencyGroup::Optional | DependencyGroup::Peer => return Ok(()),
    };
    let declared = state.manifest
        .value()
        .get(dependencies_field)
        .and_then(|dependencies| dependencies.get(&request.runtime_name))
        .and_then(|specifier| specifier.as_str());
    let Some(specifier) = declared else {
        return Err(
            RuntimeError::NotARuntimeDependency { name: request.runtime_name.clone() }.into()
        );
    };
    let Some(declared_version) = strip_tool_protocol(specifier) else {
        return Err(
            RuntimeError::NotARuntimeDependency { name: request.runtime_name.clone() }.into()
        );
    };
    if let Some(version) = request.version_spec.as_deref()
        && !runtime_version_matches(&request.runtime_name, declared_version, version)
    {
        return Err(RuntimeError::RuntimeNotDeclared {
            name: request.runtime_name.clone(),
            version: version.to_string(),
        }
        .into());
    }
    Ok(())
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
    runtime_name: &str,
    version: Option<&str>,
    removed_names: &mut HashSet<String>,
) -> miette::Result<bool> {
    let Some(global_pkg_dir) = config.global_pkg_dir.as_deref() else {
        return Ok(false);
    };
    let mut remove = || -> miette::Result<bool> {
        let packages =
            select_global_runtimes(global_pkg_dir, runtime_name, version, removed_names)?;
        if packages.is_empty() {
            return Ok(false);
        }
        remove_selected_global_runtimes::<Reporter>(config, global_pkg_dir, &packages)?;
        Ok(true)
    };

    if let Some(global_bin_dir) = config.global_bin.as_deref().filter(|dir| dir.is_dir()) {
        with_global_bin_lock(global_bin_dir, remove)
    } else {
        remove()
    }
}

fn select_global_runtimes(
    global_pkg_dir: &Path,
    runtime_name: &str,
    version: Option<&str>,
    removed_names: &mut HashSet<String>,
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
    for pkg in &packages {
        if let Some(installed) = installed_versions(&pkg.install_dir).get(runtime_name) {
            removed_names.insert(installed.clone());
        }
    }
    Ok(packages)
}

fn remove_selected_global_runtimes<Reporter: self::Reporter>(
    config: &Config,
    global_pkg_dir: &Path,
    packages: &[GlobalPackageInfo],
) -> miette::Result<()> {
    if let Some(global_bin_dir) = config.global_bin.as_deref().filter(|dir| dir.is_dir()) {
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
        .any(|(alias, specifier)| alias == runtime_name && strip_tool_protocol(specifier).is_some())
}

fn runtime_version_matches(runtime_name: &str, actual: &str, requested: &str) -> bool {
    let actual = actual.trim();
    let selector = actual.strip_prefix('^').unwrap_or(actual);
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
