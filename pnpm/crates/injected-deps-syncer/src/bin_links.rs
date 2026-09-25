use pnpm_cmd_shim::{
    LinkBinsOptions, PackageBinSource, get_bins_from_package_manifest, link_bins,
    link_bins_of_packages, remove_bin,
};
use pnpm_package_manifest::safe_read_package_json_from_dir;
use pnpm_workspace::{FindWorkspaceProjectsOpts, find_workspace_projects_no_check};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::Arc,
};

use crate::SyncInjectedDepsError;

/// Package and workspace locations for updating binary links.
pub(crate) struct SyncBinLinks<'a> {
    pub(crate) pkg_root_dir: &'a Path,
    pub(crate) resolved_targets: &'a [PathBuf],
    pub(crate) workspace_dir: &'a Path,
    pub(crate) previous_bin_names: &'a [String],
    pub(crate) hoisted_bin_dir: Option<&'a Path>,
    pub(crate) ignored_directories: &'a [PathBuf],
    pub(crate) layout: LinkLayout<'a>,
}

/// The layout the relinked shims are written for: the name of the modules
/// directories the workspace installs into, whether that name reaches a shim's
/// `NODE_PATH`, and the bin-link settings the install linked with.
pub(crate) struct LinkLayout<'a> {
    pub(crate) modules_dir_name: &'a std::ffi::OsStr,
    pub(crate) extend_node_path: bool,
    pub(crate) link_options: &'a LinkBinsOptions,
}

#[derive(Clone, Copy)]
struct RemoveStaleBins<'a> {
    target_dir: &'a Path,
    parent_modules_dir: &'a Path,
    hoisted_bin_dir: Option<&'a Path>,
    stale_bin_names: &'a [&'a String],
}

pub(crate) fn bin_names(manifest: &serde_json::Value, pkg_root_dir: &Path) -> Vec<String> {
    get_bins_from_package_manifest::<pnpm_cmd_shim::Host>(manifest, pkg_root_dir)
        .into_iter()
        .map(|command| command.name)
        .collect()
}

pub(crate) fn sync_bin_links(opts: &SyncBinLinks<'_>) -> Result<(), SyncInjectedDepsError> {
    let manifest = safe_read_package_json_from_dir(opts.pkg_root_dir)
        .map_err(|error| SyncInjectedDepsError::ReadManifest {
            dir: opts.pkg_root_dir.to_path_buf(),
            error,
        })?;
    let Some(manifest) = manifest.filter(|manifest| manifest.get("name").is_some()) else {
        return Ok(());
    };

    let current_bin_names: HashSet<String> =
        bin_names(&manifest, opts.pkg_root_dir).into_iter().collect();
    let stale_bin_names: Vec<&String> = opts.previous_bin_names
        .iter()
        .filter(|name| !current_bin_names.contains(*name))
        .collect();

    let has_bins = manifest.get("bin").is_some();
    let manifest = Arc::new(manifest);
    let link_options = workspace_link_options(opts);

    for target_dir in opts.resolved_targets {
        sync_target_bins(target_dir, &manifest, opts, &stale_bin_names, &link_options)?;
    }

    relink_project_bins(opts, has_bins, &stale_bin_names)
}

fn sync_target_bins(
    target_dir: &Path,
    manifest: &Arc<serde_json::Value>,
    opts: &SyncBinLinks<'_>,
    stale_bin_names: &[&String],
    link_options: &LinkBinsOptions,
) -> Result<(), SyncInjectedDepsError> {
    let Some(parent_modules_dir) = target_dir.parent() else {
        return Ok(());
    };
    remove_stale_bins(RemoveStaleBins {
        target_dir,
        parent_modules_dir,
        hoisted_bin_dir: opts.hoisted_bin_dir,
        stale_bin_names,
    })?;

    if manifest.get("bin").is_none() {
        return Ok(());
    }
    let packages = [PackageBinSource::new(target_dir.to_path_buf(), Arc::clone(manifest))];
    link_bins_of_packages::<pnpm_cmd_shim::Host>(
        &packages,
        &parent_modules_dir.join(".bin"),
        link_options,
    )
    .map_err(SyncInjectedDepsError::LinkBins)
}

fn workspace_link_options(opts: &SyncBinLinks<'_>) -> LinkBinsOptions {
    LinkBinsOptions {
        relocatable_root: Some(opts.workspace_dir.to_path_buf()),
        project_modules_dir_name: (opts.layout.extend_node_path
            && opts.layout.modules_dir_name != "node_modules")
            .then(|| opts.layout.modules_dir_name.to_owned()),
        ..opts.layout.link_options.clone()
    }
}

fn relink_project_bins(
    opts: &SyncBinLinks<'_>,
    has_bins: bool,
    stale_bin_names: &[&String],
) -> Result<(), SyncInjectedDepsError> {
    if !has_bins && stale_bin_names.is_empty() {
        return Ok(());
    }
    let projects = find_workspace_projects_no_check(
        opts.workspace_dir,
        &FindWorkspaceProjectsOpts {
            patterns: None,
            ignored_directories: opts.ignored_directories.to_vec(),
        },
    )
    .map_err(|error| SyncInjectedDepsError::FindProjects { error })?;
    let link_options = workspace_link_options(opts);
    for project in projects {
        relink_single_project_bins(
            &project.root_dir,
            opts.layout.modules_dir_name,
            &link_options,
            stale_bin_names,
        )?;
    }
    Ok(())
}

fn relink_single_project_bins(
    project_root_dir: &Path,
    modules_dir_name: &std::ffi::OsStr,
    link_options: &LinkBinsOptions,
    stale_bin_names: &[&String],
) -> Result<(), SyncInjectedDepsError> {
    let project_modules_dir = project_root_dir.join(modules_dir_name);
    let project_bin_dir = project_modules_dir.join(".bin");
    for name in stale_bin_names {
        remove_bin(&project_bin_dir.join(name.as_str()))
            .map_err(|error| SyncInjectedDepsError::RemoveBin {
                path: project_bin_dir.join(name.as_str()),
                error,
            })?;
    }
    link_bins::<pnpm_cmd_shim::Host>(
        &project_modules_dir,
        &project_modules_dir.join(".bin"),
        link_options,
    )
    .map_err(SyncInjectedDepsError::LinkBins)
}

fn remove_stale_bins(remove: RemoveStaleBins<'_>) -> Result<(), SyncInjectedDepsError> {
    let bin_dirs = [
        remove.parent_modules_dir.join(".bin"),
        remove.target_dir.join("node_modules").join(".bin"),
    ];
    for bin_dir in bin_dirs
        .iter()
        .map(PathBuf::as_path)
        .chain(remove.hoisted_bin_dir)
    {
        for name in remove.stale_bin_names {
            remove_bin(&bin_dir.join(name.as_str()))
                .map_err(|error| SyncInjectedDepsError::RemoveBin {
                    path: bin_dir.join(name.as_str()),
                    error,
                })?;
        }
    }
    Ok(())
}
