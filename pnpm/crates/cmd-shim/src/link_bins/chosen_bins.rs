use super::{
    DirCreation, FsCreateDirAll, FsEnsureExecutableBits, FsReadDir, FsReadHead, FsReadToString,
    FsSetExecutable, FsWrite, LinkBinsError, LinkBinsOptions, PackageBinSource, ShimSpec,
    ShimTargetCache, WindowsShimPolicy,
    linking_paths::{LinkingPaths, remove_bins_awaiting_target, shim_node_path, target_probe_path},
    package_name,
    shim_writer::write_shim,
};
use crate::bin_resolver::Command;
use rayon::prelude::*;
use std::{io, path::Path};

pub(super) fn link<Sys>(
    chosen: Vec<(Command, &PackageBinSource)>,
    bins_dir: &Path,
    options: &LinkBinsOptions,
    cache: &ShimTargetCache,
) -> Result<bool, LinkBinsError>
where
    Sys: FsReadToString
        + FsReadHead
        + FsReadDir
        + FsCreateDirAll
        + FsWrite
        + FsSetExecutable
        + FsEnsureExecutableBits,
{
    if chosen.is_empty() {
        return Ok(false);
    }
    let chosen_count = chosen.len();
    let bin_dir = prepare_bin_dir::<Sys>(bins_dir, options)?;
    let paths = LinkingPaths::new(bins_dir, bin_dir, options)?;
    let to_link = select_bins_to_link::<Sys>(chosen, bins_dir, &paths, options)?;
    link_selected_bins::<Sys>(&to_link, options, &paths, cache)?;
    Ok(to_link.len() < chosen_count)
}

fn prepare_bin_dir<Sys>(
    bins_dir: &Path,
    options: &LinkBinsOptions,
) -> Result<DirCreation, LinkBinsError>
where
    Sys: FsReadDir + FsCreateDirAll,
{
    if options.read_only {
        match Sys::read_dir(bins_dir) {
            Ok(entries) => {
                drop(entries);
                Ok(DirCreation::Unknown)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                Err(LinkBinsError::FrozenStoreWrite { path: bins_dir.to_path_buf() })
            }
            Err(error) => Err(LinkBinsError::CreateBinDir { dir: bins_dir.to_path_buf(), error }),
        }
    } else {
        Sys::create_dir_all_reporting(bins_dir)
            .map_err(|error| LinkBinsError::CreateBinDir { dir: bins_dir.to_path_buf(), error })
    }
}

fn select_bins_to_link<'packages, Sys>(
    chosen: Vec<(Command, &'packages PackageBinSource)>,
    bins_dir: &Path,
    paths: &LinkingPaths<'_>,
    options: &LinkBinsOptions,
) -> Result<Vec<(Command, &'packages PackageBinSource)>, LinkBinsError>
where
    Sys: FsReadHead,
{
    if options.read_only {
        let pending = chosen
            .iter()
            .find(|(command, pkg)| {
                super::linking_paths::awaits_target::<Sys>(pkg, bins_dir, command)
            });
        if let Some((command, _)) = pending {
            return Err(LinkBinsError::FrozenStoreWrite {
                path: paths.bins_dir.join(&command.name),
            });
        }
        Ok(chosen)
    } else {
        remove_bins_awaiting_target::<Sys>(chosen, bins_dir, &paths.bins_dir)
    }
}

fn link_selected_bins<Sys>(
    to_link: &[(Command, &PackageBinSource)],
    options: &LinkBinsOptions,
    paths: &LinkingPaths<'_>,
    cache: &ShimTargetCache,
) -> Result<(), LinkBinsError>
where
    Sys: FsReadToString + FsReadHead + FsWrite + FsSetExecutable + FsEnsureExecutableBits,
{
    to_link
        .par_iter()
        .try_for_each(|(command, pkg)| {
            let node_path =
                if options.prefer_symlinked_executables && cfg!(any(unix, target_os = "wasi")) {
                    Vec::new()
                } else {
                    shim_node_path(pkg, paths.project_node_path.as_deref(), &paths.extra_node_paths)
                };
            write_shim::<Sys>(
                ShimSpec {
                    target_path: &paths.target(&command.path, options.relocatable_root.as_deref())?,
                    probe_path: &target_probe_path(pkg, &command.path),
                    shim_path: &paths.bins_dir.join(&command.name),
                    node_path: &node_path,
                    options,
                    windows: WindowsShimPolicy::for_package(package_name(pkg)),
                    paths,
                    precalculated_runtime: command.precalculated_runtime
                        .as_ref()
                        .filter(|_| !pkg.build_pending),
                },
                cache,
            )
        })
}
