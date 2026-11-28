use super::{
    EcosystemFilterKind,
    cargo::{
        collect_cargo_outdated,
        has_cargo_lockfile,
    },
    cargo_manifest_ops::{
        cargo_project_name,
        declared_cargo_deps,
        has_matching_cargo_deps,
    },
    patterns_for_ecosystem,
    python::{
        collect_python_outdated,
        declared_python_dep_names,
        has_matching_python_deps,
        has_python_lockfile,
        python_project_name,
    },
    rust_toolchain::{
        RUST,
        collect_rust_toolchain_outdated,
        declares_rust_toolchain,
    },
};
use crate::cli_args::outdated::{
    OutdatedPackage,
    OutdatedQuery,
    OutdatedRun,
    TargetVersion,
};
use miette::Result;
use pnpm_config::Config;
use pnpm_matcher::{
    Matcher,
    create_matcher,
};
use pnpm_package_manifest::DependencyGroup;
use std::path::Path;

pub(crate) fn requires_ecosystem_lockfile(
    project_dir: &Path,
    config: &Config,
    query: &OutdatedQuery<'_>,
    package_patterns: &[String],
) -> Result<bool> {
    let workspace_root = config.workspace_dir.as_deref();
    if config.cargo.enabled
        && !has_cargo_lockfile(project_dir, workspace_root)
        && let Some(cargo_matcher) =
            ecosystem_matcher(package_patterns, EcosystemFilterKind::Cargo)?
    {
        let cargo_query = query_for_ecosystem(query, package_patterns, cargo_matcher.as_ref());
        if has_matching_cargo_deps(project_dir, workspace_root, &cargo_query)? {
            return Ok(true);
        }
    }
    if config.python.enabled
        && !has_python_lockfile(project_dir, workspace_root)
        && let Some(py_matcher) = ecosystem_matcher(package_patterns, EcosystemFilterKind::Python)?
    {
        let py_query = query_for_ecosystem(query, package_patterns, py_matcher.as_ref());
        if has_matching_python_deps(project_dir, workspace_root, &py_query)? {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(crate) fn declared_ecosystem_dep_names(
    project_dir: &Path,
    config: &Config,
    include: &[DependencyGroup],
) -> Result<Vec<String>> {
    let mut names = Vec::new();
    let workspace_root = config.workspace_dir.as_deref();
    if declares_rust_toolchain(project_dir, config, include)? {
        names.push(RUST.to_string());
    }
    if config.cargo.enabled {
        names.extend(
            declared_cargo_dep_names(project_dir, workspace_root, include)?
                .into_iter()
                .map(|name| format!("crate:{name}")),
        );
    }
    if config.python.enabled {
        for name in declared_python_dep_names(project_dir, workspace_root, include)? {
            names.push(format!("pypi:{name}"));
        }
    }
    Ok(names)
}

pub(super) fn declared_cargo_dep_names(
    project_dir: &Path,
    workspace_root: Option<&Path>,
    include: &[DependencyGroup],
) -> Result<Vec<String>> {
    let mut names = Vec::new();
    for dep in declared_cargo_deps(project_dir, workspace_root)?
        .iter()
        .filter(|dep| include.contains(&dep.group))
    {
        names.extend(
            dep.identities()
                .into_iter()
                .map(str::to_owned),
        );
    }
    Ok(names)
}

pub(crate) fn ecosystem_project_name(project_dir: &Path, config: &Config) -> Option<String> {
    if config.cargo.enabled
        && let Some(name) = cargo_project_name(project_dir)
    {
        return Some(name);
    }
    if config.python.enabled
        && let Some(name) = python_project_name(project_dir)
    {
        return Some(name);
    }
    None
}

pub(crate) async fn collect_ecosystem_outdated(
    project_dir: &Path,
    workspace_name: Option<String>,
    query: &OutdatedQuery<'_>,
    raw_packages: &[String],
    run: &OutdatedRun,
) -> Result<Vec<OutdatedPackage>> {
    let mut out = Vec::new();
    // Cargo and Python registries publish no dist-tags.
    if matches!(query.target_version, TargetVersion::Tag(_)) {
        return Ok(out);
    }
    if run.config.cargo.enabled
        && let Some(cargo_matcher) = ecosystem_matcher(raw_packages, EcosystemFilterKind::Cargo)?
    {
        let cargo_query = query_for_ecosystem(query, raw_packages, cargo_matcher.as_ref());
        out.extend(
            collect_rust_toolchain_outdated(project_dir, workspace_name.clone(), &cargo_query, run)
                .await?,
        );
        out.extend(
            collect_cargo_outdated(
                project_dir,
                workspace_name.clone(),
                &cargo_query,
                &run.config,
                &run.http_client,
            )
            .await?,
        );
    }
    if run.config.python.enabled
        && let Some(py_matcher) = ecosystem_matcher(raw_packages, EcosystemFilterKind::Python)?
    {
        let py_query = query_for_ecosystem(query, raw_packages, py_matcher.as_ref());
        out.extend(
            collect_python_outdated(
                project_dir,
                workspace_name,
                &py_query,
                &run.config,
                &run.http_client,
                &run.python_index_cache,
            )
            .await?,
        );
    }
    Ok(out)
}

fn query_for_ecosystem<'a>(
    query: &OutdatedQuery<'a>,
    raw_packages: &[String],
    ecosystem_matcher: Option<&'a Matcher>,
) -> OutdatedQuery<'a> {
    let match_names = if raw_packages.is_empty() { query.match_names } else { ecosystem_matcher };
    OutdatedQuery { match_names, ..*query }
}

fn ecosystem_matcher(
    raw_packages: &[String],
    kind: EcosystemFilterKind,
) -> Result<Option<Option<Matcher>>> {
    if raw_packages.is_empty() {
        return Ok(Some(None));
    }
    let patterns = patterns_for_ecosystem(raw_packages, kind)?;
    if patterns.is_empty() {
        return Ok(super::excludes_only(raw_packages).then_some(None));
    }
    Ok(Some(Some(create_matcher(&patterns))))
}
