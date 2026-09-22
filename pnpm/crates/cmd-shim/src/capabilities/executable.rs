#[cfg(test)]
mod tests;

use super::Host;
use std::{
    fs::{self, Permissions},
    io,
    os::unix::fs::PermissionsExt,
    path::Path,
};

pub(super) trait FsSetPermissions {
    fn set_permissions(path: &Path, permissions: Permissions) -> io::Result<()>;
}

impl FsSetPermissions for Host {
    fn set_permissions(path: &Path, permissions: Permissions) -> io::Result<()> {
        fs::set_permissions(path, permissions)
    }
}

pub(super) fn ensure_executable_bits<Sys: FsSetPermissions>(path: &Path) -> io::Result<()> {
    let target = fs::canonicalize(path)?;
    if !target
        .ancestors()
        .skip(1)
        .any(|parent| {
            parent
                .file_name()
                .is_some_and(|name| name == "node_modules")
        })
    {
        return Ok(());
    }
    let mode = fs::metadata(&target)?.permissions().mode();
    if mode & 0o111 == 0o111 {
        return Ok(());
    }
    Sys::set_permissions(&target, Permissions::from_mode(mode | 0o111))
}
