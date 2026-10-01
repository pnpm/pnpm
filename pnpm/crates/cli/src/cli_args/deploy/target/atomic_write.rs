#[cfg(not(target_os = "wasi"))]
use std::fs;
use std::{
    io::{self, Write},
    path::Path,
};

pub(super) fn write_atomic(path: &Path, contents: &[u8]) -> io::Result<()> {
    let dir = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut tmp = pnpm_fs::private_named_tempfile_in(dir)?;
    tmp.write_all(contents)?;
    tmp.as_file().sync_all()?;
    #[cfg(not(target_os = "wasi"))]
    if let Ok(metadata) = fs::metadata(path) {
        tmp.as_file().set_permissions(metadata.permissions())?;
    }
    #[cfg(target_os = "wasi")]
    if let Ok(permissions) = pnpm_fs::copy_permissions(path) {
        pnpm_fs::set_file_permissions(tmp.as_file(), &permissions)?;
    }
    tmp.persist(path).map_err(|error| error.error)?;
    Ok(())
}
