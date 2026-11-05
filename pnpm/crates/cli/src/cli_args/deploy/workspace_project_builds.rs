//! `allowBuilds` approvals for the workspace projects a deploy installs as
//! `file:` dependencies. In the source workspace, a project runs its
//! scripts without approval. The deploy rewrites the project as a `file:`
//! dependency, which a name rule in `allowBuilds` does not approve.
//!
//! Each approval is the exact dependency path of a discovered workspace
//! project's root directory. A `file:` dependency on any other directory
//! stays unapproved. A denial in `allowBuilds` wins over the approval.

use super::{
    ConvertCtx, HashMap, IntoDiagnostic, Lockfile, LockfileResolution, PackageKey, PackageMetadata,
    Path, PathBuf, ProjectPathKey, create_file_url_key, lexical_normalize, relative_path,
};
use pnpm_package_manager::AllowBuildPolicy;

/// A workspace project a legacy deploy may inject.
pub(super) struct NamedProjectRoot {
    pub(super) name: String,
    pub(super) root_dir: PathBuf,
}

/// Approves each workspace project in the lockfile of a shared-lockfile
/// deploy. A package qualifies only when its directory resolution points at
/// a project root and its key is the one the deploy generates for that root,
/// so a lockfile entry that redirects a project's key elsewhere does not.
pub(super) fn approve_workspace_project_builds(
    allow_builds: &mut HashMap<String, bool>,
    deploy_lockfile: &Lockfile,
    ctx: &ConvertCtx<'_>,
) -> miette::Result<()> {
    let mut keys = Vec::new();
    for (package_key, metadata) in deploy_lockfile.packages.iter().flatten() {
        if is_workspace_project_package(package_key, metadata, ctx)? {
            keys.push(package_key.to_string());
        }
    }
    approve(allow_builds, keys)
}

/// Approves each workspace project under the dependency path the resolver of
/// a legacy deploy gives an injected project: its root relative to
/// `lockfile_dir`.
pub(super) fn approve_legacy_workspace_project_builds(
    allow_builds: &mut HashMap<String, bool>,
    workspace_projects: &[NamedProjectRoot],
    lockfile_dir: &Path,
) -> miette::Result<()> {
    let keys = workspace_projects
        .iter()
        .map(|project| {
            format!("{}@file:{}", project.name, relative_path(lockfile_dir, &project.root_dir))
        })
        .collect();
    approve(allow_builds, keys)
}

fn approve(allow_builds: &mut HashMap<String, bool>, keys: Vec<String>) -> miette::Result<()> {
    let source_policy = AllowBuildPolicy::from_approvals(allow_builds, false).into_diagnostic()?;
    allow_builds.extend(
        keys.into_iter()
            .filter(|key| source_policy.check(key) != Some(false))
            .map(|key| (key, true)),
    );
    Ok(())
}

fn is_workspace_project_package(
    package_key: &PackageKey,
    metadata: &PackageMetadata,
    ctx: &ConvertCtx<'_>,
) -> miette::Result<bool> {
    let LockfileResolution::Directory(resolution) = &metadata.resolution else {
        return Ok(false);
    };
    let directory = lexical_normalize(&ctx.deploy_dir.join(&resolution.directory));
    let is_project_root = ctx.projects_by_path
        .get(&ProjectPathKey::new(&directory))
        .is_some_and(|project| project.root_dir == directory);
    if !is_project_root {
        return Ok(false);
    }
    Ok(create_file_url_key(&directory, "", ctx.projects_by_path, None)? == *package_key)
}

#[cfg(test)]
mod tests;
