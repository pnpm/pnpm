//! `allowBuilds` approvals for the workspace projects a deploy installs as
//! `file:` dependencies. In the source workspace, a project runs its
//! scripts without approval. The deploy rewrites the project as a `file:`
//! dependency, which a name rule in `allowBuilds` does not approve.
//!
//! Each approval is the exact dependency path of a discovered workspace
//! project's root directory. A `file:` dependency on any other directory
//! stays unapproved. A denial in `allowBuilds` wins over the approval,
//! whether it names the project's deployed key or the key the source
//! workspace gives it.

use super::{
    ConvertCtx, HashMap, IntoDiagnostic, Lockfile, LockfileResolution, PackageKey, PackageMetadata,
    Path, PathBuf, ProjectInfo, ProjectPathKey, create_file_url_key, lexical_normalize,
    relative_path,
};
use pnpm_package_manager::AllowBuildPolicy;

/// A workspace project a legacy deploy may inject.
#[derive(Clone)]
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
        if let Some(project) = workspace_project_of_package(package_key, metadata, ctx)? {
            keys.push(ProjectBuildKey {
                key: package_key.to_string(),
                source_key: project.name
                    .as_deref()
                    .map(|name| injected_project_key(name, &project.root_dir, ctx.lockfile_dir)),
            });
        }
    }
    approve(allow_builds, keys)
}

/// Approves each workspace project under the dependency path the resolver of
/// a legacy deploy gives an injected project: its root relative to
/// `lockfile_dir`. `source_lockfile_dir` is the lockfile dir of the
/// workspace the projects come from.
pub(super) fn approve_legacy_workspace_project_builds(
    allow_builds: &mut HashMap<String, bool>,
    workspace_projects: &[NamedProjectRoot],
    lockfile_dir: &Path,
    source_lockfile_dir: &Path,
) -> miette::Result<()> {
    let keys = workspace_projects
        .iter()
        .map(|project| ProjectBuildKey {
            key: injected_project_key(&project.name, &project.root_dir, lockfile_dir),
            source_key: Some(injected_project_key(
                &project.name,
                &project.root_dir,
                source_lockfile_dir,
            )),
        })
        .collect();
    approve(allow_builds, keys)
}

/// A key to approve, and the key the source workspace gives the same
/// project, a denial of which also applies to it.
struct ProjectBuildKey {
    key: String,
    source_key: Option<String>,
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
                !is_denied(&keys.key) && !keys.source_key.as_deref().is_some_and(is_denied)
            })
            .map(|keys| (keys.key, true)),
    );
    Ok(())
}

/// The key an install in `lockfile_dir` gives the project as an injected
/// `file:` dependency.
fn injected_project_key(name: &str, root_dir: &Path, lockfile_dir: &Path) -> String {
    format!("{name}@file:{}", relative_path(lockfile_dir, root_dir))
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
    let Some(project) = ctx.projects_by_path
        .get(&ProjectPathKey::new(&directory))
        .filter(|project| project.root_dir == directory)
    else {
        return Ok(None);
    };
    let key = create_file_url_key(&directory, "", ctx.projects_by_path, None)?;
    Ok((key == *package_key).then_some(project))
}

#[cfg(test)]
mod tests;
