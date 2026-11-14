pub(crate) use update::{
    PythonUpdateTargets, resolve_python_update_targets, update_python_projects,
    validate_explicit_python_requests,
};

mod update;

use super::{EcosystemUpdateStepOptions, selectors::PythonUpdateSelector};
use crate::cli_args::outdated::{
    OutdatedPackage, OutdatedQuery, TargetVersion,
    query::{OutdatedDisplayVersions, OutdatedMetadata},
};
use miette::Result;
use pep440_rs::Version as Pep440Version;
use pnpm_config::Config;
use pnpm_matcher::Matcher;
use pnpm_network::ThrottledClient;
use pnpm_package_manifest::DependencyGroup;
use pnpm_python_installer::{
    DeclaredPythonDep, LockedPythonPackages, PackageName, PythonDependencyGroup, PythonIndexCache,
    declared_dependencies, lockfile_dir_for, project_name,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

pub(crate) struct PythonUpdateOptions<'a> {
    pub(crate) requests: &'a [PythonUpdateSelector],
    pub(crate) exclusions: &'a [String],
    pub(crate) update_all_except_exclusions: bool,
    pub(crate) bare_matcher: Option<&'a Matcher>,
    pub(crate) restrict_to_selectors: bool,
    pub(crate) step: &'a EcosystemUpdateStepOptions<'a>,
}

pub(crate) fn python_project_name(project_dir: &Path) -> Option<String> {
    project_name(&project_dir.join("pyproject.toml"))
}

pub(crate) fn has_matching_python_deps(
    project_dir: &Path,
    workspace_root: Option<&Path>,
    query: &OutdatedQuery<'_>,
) -> Result<bool> {
    let pyproject = project_dir.join("pyproject.toml");
    if !pyproject.is_file() {
        return Ok(false);
    }
    Ok(declared_dependencies(&pyproject, workspace_root)?
        .into_iter()
        .any(|dep| {
            query.include_direct.contains(&map_python_group(dep.group))
                && query.matches_name(dep.name.as_ref())
        }))
}

pub(crate) fn has_python_lockfile(project_dir: &Path, workspace_root: Option<&Path>) -> bool {
    lockfile_dir_for(workspace_root, project_dir).join("pylock.toml").is_file()
}

pub(crate) fn declared_python_dep_names(
    project_dir: &Path,
    workspace_root: Option<&Path>,
    include: &[DependencyGroup],
) -> Result<Vec<String>> {
    let unique: BTreeSet<String> =
        declared_dependencies(&project_dir.join("pyproject.toml"), workspace_root)?
            .into_iter()
            .filter(|dep| include.contains(&map_python_group(dep.group)))
            .map(|dep| dep.name.to_string())
            .collect();
    Ok(unique.into_iter().collect())
}

pub(crate) async fn collect_python_outdated(
    project_dir: &Path,
    workspace_name: Option<String>,
    query: &OutdatedQuery<'_>,
    config: &Config,
    http_client: &ThrottledClient,
    index_cache: &PythonIndexCache,
) -> Result<Vec<OutdatedPackage>> {
    if !config.python.enabled {
        return Ok(Vec::new());
    }
    let workspace_root = config.workspace_dir.as_deref();
    let pyproject_path = project_dir.join("pyproject.toml");
    let declared = declared_dependencies(&pyproject_path, workspace_root)?;
    if declared.is_empty() {
        return Ok(Vec::new());
    }
    let pylock_path = lockfile_dir_for(workspace_root, project_dir).join("pylock.toml");
    if !pylock_path.is_file() {
        return Ok(Vec::new());
    }
    let locked = LockedPythonPackages::read(&pylock_path)?;
    let ctx = OutdatedPythonContext { locked: &locked, query, config, http_client, index_cache };
    let mut indices = BTreeMap::new();
    let mut merged = Vec::new();
    for dep in declared {
        let group = map_python_group(dep.group);
        if !query.include_direct.contains(&group) || !query.matches_name(dep.name.as_ref()) {
            continue;
        }
        collect_outdated_for_dep(&dep, group, &ctx, &mut indices, &mut merged).await?;
    }
    Ok(merged
        .into_iter()
        .map(|entry| into_outdated_package(&entry, workspace_name.as_deref()))
        .collect())
}

struct OutdatedPythonContext<'a> {
    locked: &'a LockedPythonPackages,
    query: &'a OutdatedQuery<'a>,
    config: &'a Config,
    http_client: &'a ThrottledClient,
    index_cache: &'a PythonIndexCache,
}

struct ResolvedDepTargets {
    wanted: Option<Pep440Version>,
    latest: Option<Pep440Version>,
}

type MergedOutdatedKey = (PythonDependencyGroup, PackageName, Pep440Version);

struct MergedPythonOutdated {
    name: PackageName,
    group: DependencyGroup,
    current: Pep440Version,
    wanted: Pep440Version,
    target: Pep440Version,
}

async fn collect_outdated_for_dep(
    dep: &DeclaredPythonDep,
    group: DependencyGroup,
    ctx: &OutdatedPythonContext<'_>,
    indices: &mut BTreeMap<MergedOutdatedKey, usize>,
    merged: &mut Vec<MergedPythonOutdated>,
) -> Result<()> {
    let matching = ctx.locked.matching_versions(&dep.requirement);
    if matching.is_empty() {
        return Ok(());
    }
    let Some(targets) = resolve_dep_targets(dep, ctx).await? else {
        return Ok(());
    };
    for current in matching {
        record_merged_outdated(dep, group, current, &targets, indices, merged);
    }
    Ok(())
}

pub(super) fn map_python_group(group: PythonDependencyGroup) -> DependencyGroup {
    match group {
        PythonDependencyGroup::Prod => DependencyGroup::Prod,
        PythonDependencyGroup::Optional => DependencyGroup::Optional,
        PythonDependencyGroup::Dev => DependencyGroup::Dev,
    }
}

async fn resolve_dep_targets(
    dep: &DeclaredPythonDep,
    ctx: &OutdatedPythonContext<'_>,
) -> Result<Option<ResolvedDepTargets>> {
    let wanted =
        ctx.index_cache.resolve_target_version(ctx.config, ctx.http_client, &dep.requirement, true)
            .await?;
    let latest = match ctx.query.target_version {
        TargetVersion::WithinRange => None,
        TargetVersion::Latest => {
            let Some(latest) = ctx.index_cache.resolve_target_version(
                ctx.config,
                ctx.http_client,
                &dep.requirement,
                false,
            )
            .await?
            else {
                return Ok(None);
            };
            Some(latest)
        }
    };
    Ok(Some(ResolvedDepTargets { wanted, latest }))
}

fn record_merged_outdated(
    dep: &DeclaredPythonDep,
    group: DependencyGroup,
    current: &Pep440Version,
    targets: &ResolvedDepTargets,
    indices: &mut BTreeMap<MergedOutdatedKey, usize>,
    merged: &mut Vec<MergedPythonOutdated>,
) {
    let wanted = targets.wanted.clone().unwrap_or_else(|| current.clone());
    let target = targets.latest.clone().unwrap_or_else(|| wanted.clone());
    if target <= *current {
        return;
    }
    let key = (dep.group, dep.name.clone(), current.clone());
    if let Some(&idx) = indices.get(&key) {
        let existing = &mut merged[idx];
        if target > existing.target {
            existing.target = target;
        }
        if wanted > existing.wanted {
            existing.wanted = wanted;
        }
        return;
    }
    indices.insert(key, merged.len());
    merged.push(MergedPythonOutdated {
        name: dep.name.clone(),
        group,
        current: current.clone(),
        wanted,
        target,
    });
}

fn into_outdated_package(
    entry: &MergedPythonOutdated,
    workspace_name: Option<&str>,
) -> OutdatedPackage {
    let name_str = entry.name.to_string();
    OutdatedPackage {
        alias: name_str.clone(),
        package_name: name_str.clone(),
        belongs_to: entry.group,
        current: pep440_to_node_version(&entry.current),
        target: pep440_to_node_version(&entry.target),
        wanted: pep440_to_node_version(&entry.wanted),
        github_action: false,
        metadata: OutdatedMetadata {
            deprecated: None,
            homepage: Some(format!("https://pypi.org/project/{name_str}/")),
            workspace: workspace_name.map(str::to_string),
            display_versions: Some(Box::new(OutdatedDisplayVersions {
                current: entry.current.to_string(),
                target: entry.target.to_string(),
                wanted: entry.wanted.to_string(),
            })),
        },
    }
}

fn pep440_to_node_version(version: &Pep440Version) -> node_semver::Version {
    let release = version.release();
    let major = release.first().copied().unwrap_or(0);
    let minor = release.get(1).copied().unwrap_or(0);
    let patch = release.get(2).copied().unwrap_or(0);
    let has_suffix = version.any_prerelease()
        || version.is_post()
        || version.epoch() > 0
        || release.len() > 3
        || !version.local().is_empty();
    let pre_release = if has_suffix {
        vec![node_semver::Identifier::AlphaNumeric(version.to_string())]
    } else {
        Vec::new()
    };
    node_semver::Version { major, minor, patch, build: Vec::new(), pre_release }
}
