use super::{
    super::{
        Config, PROJECT_LIFECYCLE_STAGES, PackageManifest, Path, load_workspace_projects,
        project_requires_lifecycle_scripts,
    },
    build_project_manifests_list,
    discovery::GateInputs,
};
use pnpm_executor::DEV_PREINSTALL_STAGE;

/// Whether an install of these never-installed projects would have
/// nothing to do: no project declares a dependency, a peer that
/// `autoInstallPeers` would fetch, or a script the install runs.
///
/// The projects are the ones the install would cover: every workspace
/// project under one shared lockfile, otherwise `manifest` alone. A
/// workspace that cannot be walked counts as having work to do, and so
/// does a pnpmfile the install would load from `lockfile_root`, whose
/// `readPackage` hook can add dependencies. So does a `lockfileDir` pinned
/// away from `manifest`, whose importers this check does not resolve.
pub(super) fn projects_have_nothing_to_install(inputs: &GateInputs<'_>) -> bool {
    let GateInputs {
        config,
        manifest,
        workspace_manifest,
        workspace_root,
        lockfile_root,
    } = *inputs;
    if manifest.path().parent() != Some(lockfile_root)
        || project_has_install_work(config, manifest)
        || runs_dev_preinstall(config, manifest, workspace_root)
        || !crate::optimistic_repeat_install::current_pnpmfiles(lockfile_root, config).is_empty()
    {
        return false;
    }
    if !config.shares_one_lockfile() {
        return true;
    }
    let Ok(workspace_projects) =
        load_workspace_projects(workspace_root, workspace_manifest, &config.managed_directories())
    else {
        return false;
    };
    build_project_manifests_list(manifest, workspace_projects.as_deref())
        .into_iter()
        .all(|(_, project_manifest)| !project_has_install_work(config, project_manifest))
}

fn project_has_install_work(config: &Config, manifest: &PackageManifest) -> bool {
    let project_dir = manifest
        .path()
        .parent()
        .expect("manifest path always has a parent dir");
    crate::optimistic_repeat_install::manifest_has_runtime_deps(manifest)
        || (config.auto_install_peers
            && crate::optimistic_repeat_install::manifest_has_required_peers(manifest))
        || (!config.ignore_scripts
            && project_requires_lifecycle_scripts(project_dir, manifest, &PROJECT_LIFECYCLE_STAGES))
}

/// `pnpm:devPreinstall` runs only from the workspace root's manifest.
fn runs_dev_preinstall(config: &Config, manifest: &PackageManifest, workspace_root: &Path) -> bool {
    !config.ignore_scripts
        && manifest.path().parent() == Some(workspace_root)
        && matches!(manifest.script(DEV_PREINSTALL_STAGE, true), Ok(Some(_)))
}
