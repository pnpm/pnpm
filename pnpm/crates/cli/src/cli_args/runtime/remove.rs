use super::{
    RuntimeError,
    RuntimeRemoveRequest,
};
use crate::{
    State,
    cli_args::{
        env::{
            cleanup_bin_links,
            cleanup_pnpm_home,
            matches_node_version,
            remove_global_node_install,
        },
        global::{
            GlobalError,
            remove_global_package_groups,
        },
    },
};
use miette::IntoDiagnostic;
use pnpm_config::Config;
use pnpm_global::{
    GlobalPackageInfo,
    installed_versions,
    scan_global_packages,
};
use pnpm_package_manifest::DependencyGroup;
use pnpm_reporter::Reporter;
use pnpm_tool_protocol::strip_tool_protocol;
use std::collections::HashSet;

pub fn run_remove<Reporter: self::Reporter>(
    request: &RuntimeRemoveRequest,
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
    let removed_legacy = cleanup_legacy_runtime_artifacts(request, config, &mut removed_names)?;
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
    request: &RuntimeRemoveRequest,
    config: &'static Config,
    removed_names: &mut HashSet<String>,
) -> miette::Result<bool> {
    let version = request.version_spec.as_deref();
    let pnpm_home = pnpm_config::default_pnpm_home_dir::<pnpm_config::Host>();
    let matches_legacy_version = |name: &str| {
        request.runtime_name == "node"
            && version.is_none_or(|requested| runtime_version_matches("node", name, requested))
    };
    let mut removed =
        cleanup_pnpm_home(pnpm_home.as_deref(), &matches_legacy_version, removed_names)
            .into_diagnostic()?;
    if let Some(global_bin_dir) = config.global_bin.as_deref().filter(|dir| dir.is_dir()) {
        let removed_bin = if request.runtime_name == "node" {
            cleanup_bin_links(global_bin_dir, &["node", "npm", "npx"], removed_names)
                .into_diagnostic()?
        } else {
            cleanup_bin_links(global_bin_dir, &[request.runtime_name.as_str()], removed_names)
                .into_diagnostic()?
        };
        removed |= removed_bin;
    }
    Ok(removed)
}

pub fn ensure_project_runtime_matches(
    state: &State,
    request: &RuntimeRemoveRequest,
) -> miette::Result<()> {
    let Some(version) = request.version_spec.as_deref() else {
        return Ok(());
    };
    let dependencies_field = match request.dependency_group {
        DependencyGroup::Prod => "dependencies",
        DependencyGroup::Dev => "devDependencies",
        DependencyGroup::Optional | DependencyGroup::Peer => return Ok(()),
    };
    let matches = state.manifest
        .value()
        .get(dependencies_field)
        .and_then(|dependencies| dependencies.get(&request.runtime_name))
        .and_then(|specifier| specifier.as_str())
        .and_then(strip_tool_protocol)
        .is_some_and(|installed| {
            runtime_version_matches(&request.runtime_name, installed, version)
        });
    if matches {
        return Ok(());
    }
    Err(RuntimeError::RuntimeNotDeclared {
        name: request.runtime_name.clone(),
        version: version.to_string(),
    }
    .into())
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
    let packages = scan_global_packages(global_pkg_dir)
        .into_diagnostic()?
        .into_iter()
        .filter(|pkg| pkg.has_alias(runtime_name))
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

    if packages.is_empty() {
        return Ok(false);
    }
    for pkg in &packages {
        if let Some(installed) = installed_versions(&pkg.install_dir).get(runtime_name) {
            removed_names.insert(installed.clone());
        }
    }

    if let Some(global_bin_dir) = config.global_bin.as_deref().filter(|dir| dir.is_dir()) {
        remove_global_package_groups::<Reporter>(global_pkg_dir, global_bin_dir, &packages)?;
    } else {
        for pkg in &packages {
            remove_global_node_install(global_pkg_dir, pkg).into_diagnostic()?;
        }
    }
    Ok(true)
}

fn runtime_version_matches(runtime_name: &str, actual: &str, requested: &str) -> bool {
    if runtime_name == "node" {
        return matches_node_version(actual, requested);
    }
    let requested = pnpm_engine_runtime_node_resolver::strip_leading_v(requested.trim());
    if requested.is_empty() {
        return false;
    }
    actual == requested || actual.starts_with(&format!("{requested}."))
}

#[cfg(test)]
mod tests;
