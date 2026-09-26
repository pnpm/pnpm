use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

use crate::install::InstallError;
use indexmap::IndexMap;
use pnpm_lockfile::Lockfile;
use pnpm_package_manifest::{DependencyGroup, PackageManifest};

pub(in crate::install) struct ProjectLifecycleGraph<'a> {
    pub(in crate::install) projects_by_dir: HashMap<PathBuf, (PathBuf, &'a PackageManifest)>,
    pub(in crate::install) dependencies: IndexMap<PathBuf, Vec<PathBuf>>,
}

pub(in crate::install) fn project_lifecycle_graph<'a>(
    projects: &[(PathBuf, &'a PackageManifest)],
    ordered_dependencies: Option<&IndexMap<PathBuf, Vec<PathBuf>>>,
    workspace_root: &Path,
    lockfile: Option<&Lockfile>,
) -> Result<ProjectLifecycleGraph<'a>, InstallError> {
    let normalized_project_dirs = projects
        .iter()
        .map(|(project_dir, _)| pnpm_fs::lexical_normalize(project_dir))
        .collect::<Vec<_>>();
    let dependencies = lifecycle_dependencies(
        projects,
        &normalized_project_dirs,
        ordered_dependencies,
        workspace_root,
        lockfile,
    )?;
    let projects_by_dir = projects
        .iter()
        .map(|project| (pnpm_fs::lexical_normalize(&project.0), project))
        .collect::<HashMap<_, _>>();
    let missing_projects = projects_outside_order(projects, &dependencies, &projects_by_dir);
    if !missing_projects.is_empty() {
        return Err(InstallError::ProjectLifecycleOrder { projects: missing_projects.join(", ") });
    }
    Ok(ProjectLifecycleGraph {
        dependencies: retain_known_projects(&dependencies, &projects_by_dir),
        projects_by_dir: projects_by_dir
            .into_iter()
            .map(|(dir, project)| (dir, project.clone()))
            .collect(),
    })
}

fn lifecycle_dependencies<'a>(
    projects: &[(PathBuf, &PackageManifest)],
    normalized_project_dirs: &[PathBuf],
    ordered_dependencies: Option<&'a IndexMap<PathBuf, Vec<PathBuf>>>,
    workspace_root: &Path,
    lockfile: Option<&Lockfile>,
) -> Result<std::borrow::Cow<'a, IndexMap<PathBuf, Vec<PathBuf>>>, InstallError> {
    let ordered_dirs = ordered_dependencies.map(|dependencies| {
        dependencies
            .keys()
            .map(|project_dir| pnpm_fs::lexical_normalize(project_dir))
            .collect::<HashSet<_>>()
    });
    let explicit_order_covers_projects = ordered_dirs
        .as_ref()
        .is_some_and(|ordered_dirs| {
            normalized_project_dirs
                .iter()
                .all(|project_dir| ordered_dirs.contains(project_dir))
        });
    let dependencies: std::borrow::Cow<IndexMap<PathBuf, Vec<PathBuf>>> =
        if explicit_order_covers_projects {
            std::borrow::Cow::Borrowed(ordered_dependencies.expect("checked as present"))
        } else if let Some(lockfile) = lockfile {
            std::borrow::Cow::Owned(link_dependencies_from_lockfile(
                projects,
                normalized_project_dirs,
                workspace_root,
                lockfile,
            ))
        } else if let Some(ordered_dirs) = ordered_dirs {
            return Err(missing_lifecycle_order(normalized_project_dirs, &ordered_dirs));
        } else {
            std::borrow::Cow::Owned(
                normalized_project_dirs
                    .iter()
                    .cloned()
                    .map(|project_dir| (project_dir, Vec::new()))
                    .collect::<IndexMap<_, _>>(),
            )
        };
    Ok(dependencies)
}

fn resolve_link_dep(
    version: &pnpm_lockfile::ImporterDepVersion,
    project_dir: &Path,
    target_to_project: &HashMap<PathBuf, PathBuf>,
) -> Option<PathBuf> {
    let pnpm_lockfile::ImporterDepVersion::Link(target) = version else {
        return None;
    };
    let normalized = pnpm_fs::lexical_normalize(&project_dir.join(target));
    target_to_project.get(&normalized).cloned()
}

/// Maps each project's root, and each publish directory its dependents link
/// to, back to the project. Publish directories come from both the manifest and
/// the lockfile importer, since the links being ordered follow the lockfile.
fn map_targets_to_projects(
    projects: &[(PathBuf, &PackageManifest)],
    normalized_project_dirs: &[PathBuf],
    workspace_root: &Path,
    lockfile: &Lockfile,
) -> HashMap<PathBuf, PathBuf> {
    let mut target_to_project: HashMap<PathBuf, PathBuf> = HashMap::new();
    for ((project_dir, manifest), normalized_project_dir) in
        projects.iter().zip(normalized_project_dirs)
    {
        target_to_project.insert(normalized_project_dir.clone(), normalized_project_dir.clone());
        let importer_id = pnpm_workspace::importer_id_from_root_dir(workspace_root, project_dir);
        let snapshot_publish_dir = lockfile.importers
            .get(&importer_id)
            .filter(|snapshot| snapshot.link_directory != Some(false))
            .and_then(|snapshot| snapshot.publish_directory.as_deref());
        for publish_dir in manifest_publish_dir(manifest).into_iter().chain(snapshot_publish_dir) {
            let normalized_publish = pnpm_fs::lexical_normalize(&project_dir.join(publish_dir));
            target_to_project.insert(normalized_publish, normalized_project_dir.clone());
        }
    }
    target_to_project
}

fn manifest_publish_dir(manifest: &PackageManifest) -> Option<&str> {
    let publish_config = manifest.value().get("publishConfig")?;
    let link_directory = publish_config
        .get("linkDirectory")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(true);
    link_directory
        .then(|| publish_config.get("directory").and_then(serde_json::Value::as_str))
        .flatten()
}

/// Each project's `link:` dependencies on the other projects, read off the
/// lockfile's importer entries.
fn link_dependencies_from_lockfile(
    projects: &[(PathBuf, &PackageManifest)],
    normalized_project_dirs: &[PathBuf],
    workspace_root: &Path,
    lockfile: &Lockfile,
) -> IndexMap<PathBuf, Vec<PathBuf>> {
    let target_to_project =
        map_targets_to_projects(projects, normalized_project_dirs, workspace_root, lockfile);
    projects
        .iter()
        .zip(normalized_project_dirs)
        .map(|((project_dir, _), normalized_project_dir)| {
            let importer_id =
                pnpm_workspace::importer_id_from_root_dir(workspace_root, project_dir);
            let dependencies = lockfile.importers
                .get(&importer_id)
                .into_iter()
                .flat_map(|snapshot| {
                    [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional]
                        .into_iter()
                        .filter_map(|group| snapshot.get_map_by_group(group))
                        .flat_map(|dependencies| dependencies.values())
                })
                .filter_map(|dep| resolve_link_dep(&dep.version, project_dir, &target_to_project))
                .filter(|dep_project_dir| dep_project_dir != normalized_project_dir)
                .collect();
            (normalized_project_dir.clone(), dependencies)
        })
        .collect()
}

fn projects_outside_order(
    projects: &[(PathBuf, &PackageManifest)],
    dependencies: &IndexMap<PathBuf, Vec<PathBuf>>,
    projects_by_dir: &HashMap<PathBuf, &(PathBuf, &PackageManifest)>,
) -> Vec<String> {
    let included: HashSet<PathBuf> = dependencies
        .keys()
        .map(|dir| pnpm_fs::lexical_normalize(dir))
        .filter(|dir| projects_by_dir.contains_key(dir))
        .collect();
    projects
        .iter()
        .filter(|(project_dir, _)| !included.contains(&pnpm_fs::lexical_normalize(project_dir)))
        .map(|(project_dir, _)| project_dir.display().to_string())
        .collect()
}

/// The dependency order normalized and narrowed to the projects the run
/// knows.
fn retain_known_projects(
    dependencies: &IndexMap<PathBuf, Vec<PathBuf>>,
    projects_by_dir: &HashMap<PathBuf, &(PathBuf, &PackageManifest)>,
) -> IndexMap<PathBuf, Vec<PathBuf>> {
    dependencies
        .iter()
        .filter_map(|(dir, project_dependencies)| {
            let dir = pnpm_fs::lexical_normalize(dir);
            projects_by_dir
                .contains_key(&dir)
                .then(|| {
                    (
                        dir,
                        project_dependencies
                            .iter()
                            .map(|dependency| pnpm_fs::lexical_normalize(dependency))
                            .filter(|dependency| projects_by_dir.contains_key(dependency))
                            .collect(),
                    )
                })
        })
        .collect()
}

fn missing_lifecycle_order(
    project_dirs: &[PathBuf],
    ordered_dirs: &HashSet<PathBuf>,
) -> InstallError {
    InstallError::ProjectLifecycleOrder {
        projects: project_dirs
            .iter()
            .filter(|dir| !ordered_dirs.contains(*dir))
            .map(|dir| dir.display().to_string())
            .collect::<Vec<_>>()
            .join(", "),
    }
}
