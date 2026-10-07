#[cfg(test)]
mod tests;

use super::Host;
#[cfg(target_os = "wasi")]
use pnpm_fs::CopyPermissions as Permissions;
use std::{fs, io, path::Path};
#[cfg(unix)]
use std::{fs::Permissions, os::unix::fs::PermissionsExt};

pub(super) trait FsSetPermissions {
    fn set_permissions(path: &Path, permissions: Permissions) -> io::Result<()>;
}

impl FsSetPermissions for Host {
    fn set_permissions(path: &Path, permissions: Permissions) -> io::Result<()> {
        #[cfg(unix)]
        return fs::set_permissions(path, permissions);
        #[cfg(target_os = "wasi")]
        return pnpm_fs::file_mode::set_path_permissions(path, permissions);
    }
}

pub(super) fn set_executable<Sys: FsSetPermissions>(path: &Path) -> io::Result<()> {
    if is_executable_by_everyone(permission_bits(path)?) {
        return Ok(());
    }
    #[cfg(unix)]
    let permissions = Permissions::from_mode(0o755);
    #[cfg(target_os = "wasi")]
    let permissions = 0o755;
    Sys::set_permissions(path, permissions)
}

pub(super) fn ensure_executable_bits<Sys: FsSetPermissions>(
    path: &Path,
    installed_modules_dir: Option<&Path>,
) -> io::Result<()> {
    let target = fs::canonicalize(path)?;
    let in_node_modules = target
        .ancestors()
        .skip(1)
        .any(|parent| {
            parent
                .file_name()
                .is_some_and(|name| name == "node_modules")
        });
    if !in_node_modules
        && !installed_modules_dir
            .and_then(|dir| fs::canonicalize(dir).ok())
            .is_some_and(|dir| target.starts_with(dir))
    {
        return Ok(());
    }
    let mode = permission_bits(&target)?;
    if is_executable_by_everyone(mode) {
        return Ok(());
    }
    #[cfg(unix)]
    let permissions = Permissions::from_mode(mode | 0o111);
    #[cfg(target_os = "wasi")]
    let permissions = mode | 0o111;
    Sys::set_permissions(&target, permissions)
}

/// A file that passes needs no chmod. That is what lets an install reuse a
/// file another user owns: only the owner may chmod it, even to its current mode.
fn is_executable_by_everyone(mode: u32) -> bool {
    mode & 0o111 == 0o111
}

fn permission_bits(path: &Path) -> io::Result<u32> {
    #[cfg(unix)]
    return Ok(fs::metadata(path)?.permissions().mode());
    #[cfg(target_os = "wasi")]
    return pnpm_fs::copy_permissions(path);
}
