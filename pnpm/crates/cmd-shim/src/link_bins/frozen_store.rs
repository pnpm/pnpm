#[cfg(windows)]
use super::executable::is_same_file;
use super::{
    FsReadHead, LinkBinsError, ShimSpec, is_node_bin_name, shim_writer::with_extension_appended,
    symlink_already_points_at, target_requires_shim,
};
use std::path::Path;

pub(super) fn direct_shim_is_current<Sys>(
    spec: &ShimSpec<'_>,
    stale_powershell: Option<&Path>,
) -> Result<bool, LinkBinsError>
where
    Sys: FsReadHead,
{
    #[cfg(windows)]
    let current_windows_node_executable = is_node_bin_name(spec.shim_path)
        && spec.target_path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
        && is_same_file(&with_extension_appended(spec.shim_path, "exe"), spec.target_path);
    #[cfg(not(windows))]
    let current_windows_node_executable = false;
    if !spec.options.read_only
        || spec.options.force
        || !(symlink_already_points_at(spec.shim_path, spec.target_path, spec.relocatable_root())
            || current_windows_node_executable)
    {
        return Ok(false);
    }
    if let Some(path) = stale_powershell {
        return Err(LinkBinsError::FrozenStoreWrite { path: path.to_path_buf() });
    }
    if target_requires_shim::<Sys>(spec.probe_path) {
        return Err(LinkBinsError::FrozenStoreWrite { path: spec.shim_path.to_path_buf() });
    }
    Ok(true)
}
