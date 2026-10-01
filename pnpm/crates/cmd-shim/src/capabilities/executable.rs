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
    #[cfg(unix)]
    let mode = fs::metadata(&target)?.permissions().mode();
    #[cfg(target_os = "wasi")]
    let mode = pnpm_fs::copy_permissions(&target)?;
    if mode & 0o111 == 0o111 {
        return Ok(());
    }
    #[cfg(unix)]
    let permissions = Permissions::from_mode(mode | 0o111);
    #[cfg(target_os = "wasi")]
    let permissions = mode | 0o111;
    Sys::set_permissions(&target, permissions)
}
