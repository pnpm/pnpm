//! `allowBuilds` approvals for the workspace projects a deploy installs as
//! `file:` dependencies. In the source workspace, a project runs its
//! scripts without approval. The deploy rewrites the project as a `file:`
//! dependency, which a name rule in `allowBuilds` does not approve.
//!
//! Each approval is the exact dependency path of a discovered workspace
//! project's root directory, or of the publish directory that dependents
//! link it from. A publish directory qualifies only when it is a real
//! directory inside the project root and outside `node_modules`. A `file:`
//! dependency on any other directory stays unapproved. A denial in
//! `allowBuilds` wins over the approval, whether it names the project's
//! deployed key or a key the source workspace gives it.

use super::{
    ConvertCtx, HashMap, IntoDiagnostic, Lockfile, LockfileResolution, PackageKey, PackageMetadata,
    Path, PathBuf, ProjectInfo, ProjectPathKey, create_file_url_key, lexical_normalize,
    relative_path,
};
use pnpm_package_manager::AllowBuildPolicy;
use std::fs;

/// A workspace project a legacy deploy may inject.
#[derive(Clone)]
pub(super) struct NamedProjectRoot {
    pub(super) name: String,
    pub(super) root_dir: PathBuf,
    /// See [`ProjectInfo`]'s field of the same name.
    pub(super) linked_publish_dir: Option<PathBuf>,
}

/// Approves each workspace project in the lockfile of a shared-lockfile
/// deploy. A package qualifies only when its directory resolution points at
/// a project's package directory and its key is the one the deploy generates
/// for that directory, so a lockfile entry that redirects a project's key
/// elsewhere does not.
pub(super) fn approve_workspace_project_builds(
    allow_builds: &mut HashMap<String, bool>,
    deploy_lockfile: &Lockfile,
    ctx: &ConvertCtx<'_>,
) -> miette::Result<()> {
    let mut keys = Vec::new();
    for (package_key, metadata) in deploy_lockfile.packages.iter().flatten() {
        if let Some(project) = workspace_project_of_package(package_key, metadata, ctx)? {
            keys.push(ProjectBuildKey {
                key: package_key.to_string(),
                source_keys: project.name
                    .as_deref()
                    .map(|name| {
                        injected_project_keys(
                            name,
                            &package_dirs(&project.root_dir, project.linked_publish_dir.as_deref()),
                            ctx.lockfile_dir,
                        )
                    })
                    .unwrap_or_default(),
            });
        }
    }
    approve(allow_builds, keys)
}

/// Approves each workspace project under the dependency paths the resolver
/// of a legacy deploy gives an injected project: its package directories
/// relative to `lockfile_dir`. `source_lockfile_dir` is the lockfile dir of
/// the workspace the projects come from.
pub(super) fn approve_legacy_workspace_project_builds(
    allow_builds: &mut HashMap<String, bool>,
    workspace_projects: &[NamedProjectRoot],
    lockfile_dir: &Path,
    source_lockfile_dir: &Path,
) -> miette::Result<()> {
    let mut keys = Vec::new();
    for project in workspace_projects {
        let package_dirs = package_dirs(&project.root_dir, project.linked_publish_dir.as_deref());
        let source_keys = injected_project_keys(&project.name, &package_dirs, source_lockfile_dir);
        for key in injected_project_keys(&project.name, &package_dirs, lockfile_dir) {
            keys.push(ProjectBuildKey { key, source_keys: source_keys.clone() });
        }
    }
    approve(allow_builds, keys)
}

/// A key to approve, and the keys the source workspace gives the same
/// project, a denial of any of which also applies to it.
struct ProjectBuildKey {
    key: String,
    source_keys: Vec<String>,
}

fn approve(
    allow_builds: &mut HashMap<String, bool>,
    keys: Vec<ProjectBuildKey>,
) -> miette::Result<()> {
    let source_policy = AllowBuildPolicy::from_approvals(allow_builds, false).into_diagnostic()?;
    let is_denied = |key: &str| source_policy.check(key) == Some(false);
    allow_builds.extend(
        keys.into_iter()
            .filter(|keys| {
                !is_denied(&keys.key) && !keys.source_keys.iter().any(|key| is_denied(key))
            })
            .map(|keys| (keys.key, true)),
    );
    Ok(())
}

/// The keys an install in `lockfile_dir` gives the project as an injected
/// `file:` dependency on each of the given package directories.
fn injected_project_keys(name: &str, package_dirs: &[PathBuf], lockfile_dir: &Path) -> Vec<String> {
    package_dirs
        .iter()
        .map(|package_dir| format!("{name}@file:{}", relative_path(lockfile_dir, package_dir)))
        .collect()
}

/// The directories a dependent may install the project from: its root, and
/// its linked publish directory when that is the project's own.
fn package_dirs(root_dir: &Path, linked_publish_dir: Option<&Path>) -> Vec<PathBuf> {
    std::iter::once(root_dir.to_path_buf())
        .chain(
            linked_publish_dir
                .filter(|publish_dir| is_own_publish_dir(root_dir, publish_dir))
                .map(Path::to_path_buf),
        )
        .collect()
}

/// Whether `publish_dir` is a real directory inside `root_dir`, reached
/// through no symlink and no `node_modules`. `node_modules` matches in any
/// case, as a case-insensitive file system resolves every spelling to it.
/// An unreadable path does not qualify, so the approval fails closed.
fn is_own_publish_dir(root_dir: &Path, publish_dir: &Path) -> bool {
    let Ok(relative) = publish_dir.strip_prefix(root_dir) else {
        return false;
    };
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|component| component.as_os_str().eq_ignore_ascii_case("node_modules"))
    {
        return false;
    }
    match (fs::canonicalize(root_dir), fs::canonicalize(publish_dir)) {
        (Ok(real_root_dir), Ok(real_publish_dir)) => {
            real_publish_dir == real_root_dir.join(relative)
        }
        _ => false,
    }
}

fn workspace_project_of_package<'ctx>(
    package_key: &PackageKey,
    metadata: &PackageMetadata,
    ctx: &ConvertCtx<'ctx>,
) -> miette::Result<Option<&'ctx ProjectInfo>> {
    let LockfileResolution::Directory(resolution) = &metadata.resolution else {
        return Ok(None);
    };
    let directory = lexical_normalize(&ctx.deploy_dir.join(&resolution.directory));
    let project = ctx.projects_by_path
        .get(&ProjectPathKey::new(&directory))
        .filter(|project| project.root_dir == directory)
        .or_else(|| {
            ctx.projects_by_path
                .values()
                .find(|project| {
                    project.linked_publish_dir.as_deref() == Some(&*directory)
                        && is_own_publish_dir(&project.root_dir, &directory)
                })
        });
    let Some(project) = project else {
        return Ok(None);
    };
    let key = create_file_url_key(&directory, "", ctx.projects_by_path, None)?;
    Ok((key == *package_key).then_some(project))
}

#[cfg(test)]
mod tests;
