use std::{
    fs,
    io::{self, Write as _},
    path::Path,
};

/// Atomically replace `path` with `bytes`: write a sibling temp file, then
/// rename it over the target, creating parent directories as needed. A crash
/// or I/O failure mid-write leaves the original file intact rather than a
/// truncated one — important for credential files like `.npmrc` / `auth.ini`,
/// where a partial write could drop other tokens. Mirrors pnpm's
/// `write-ini-file`.
///
/// Symlink-safe and permission-preserving: an existing regular target's mode
/// is carried across the rename, while a symlinked target is replaced with a
/// fresh regular file (the link is never followed, so the write can't be
/// redirected to an unintended path). New files keep the conservative 0600
/// default — they may hold credentials.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    create_parent_dir(path)?;
    write_tmp_over(path, bytes, InheritMode::Yes, 0o600)
}

/// Like [`write_atomic`], but a new file gets mode 0666 less the umask, and a
/// missing parent directory is an error. For project files other users read.
pub fn write_atomic_with_umask(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_tmp_over(path, bytes, InheritMode::Yes, 0o666)
}

/// Like [`write_atomic`], but the result is only ever readable by its owner.
///
/// For a file whose contents are a credential, where [`write_atomic`]'s
/// mode-preserving rule is the wrong one: inheriting a target that happens to
/// be group- or world-readable would publish the secret being written to it.
/// The file `pnpm login` records a token in also holds ordinary settings, so
/// it can already exist with the mode an editor gave it.
pub fn write_atomic_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    create_parent_dir(path)?;
    write_tmp_over(path, bytes, InheritMode::No, 0o600)
}

/// Whether the target's existing mode survives the rename. [`InheritMode::No`]
/// leaves the creation mode.
#[derive(Clone, Copy)]
enum InheritMode {
    Yes,
    No,
}

/// Create `path` exclusively with `mode`, less the umask.
#[cfg(not(target_os = "wasi"))]
fn create_new_file(path: &Path, mode: u32) -> io::Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options
        .read(true)
        .write(true)
        .create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, mode);
    #[cfg(not(unix))]
    let _ = mode;
    options.open(path)
}

#[cfg(target_os = "wasi")]
fn create_new_file(path: &Path, mode: u32) -> io::Result<fs::File> {
    crate::wasi_fs::create_new(path, mode)
}

fn create_parent_dir(path: &Path) -> io::Result<()> {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => fs::create_dir_all(parent),
        _ => Ok(()),
    }
}

fn write_tmp_over(path: &Path, bytes: &[u8], inherit: InheritMode, mode: u32) -> io::Result<()> {
    let dir = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    // Registered before the create, so no interrupt finds the temp file
    // unregistered. A colliding random name is retried under a new one,
    // whose registration replaces the old.
    let mut _pending_temp = None;
    let mut tmp = tempfile::Builder::new()
        .make_in(dir.unwrap_or_else(|| Path::new(".")), |path| {
            _pending_temp = Some(crate::pending_temp::track_temp_file(path));
            create_new_file(path, mode)
        })?;
    tmp.write_all(bytes)?;
    crate::sync_file(tmp.as_file())?;
    inherit_permissions(path, tmp.as_file(), inherit)?;
    let mut pending = Some(tmp.into_temp_path());
    crate::retry::retry_transient_file_locks(|| {
        let temporary = pending.take().expect("temporary path retained after a failed persist");
        match temporary.persist(path) {
            Ok(()) => Ok(()),
            Err(error) => {
                pending = Some(error.path);
                Err(error.error)
            }
        }
    })?;
    Ok(())
}

fn inherit_permissions(path: &Path, file: &fs::File, inherit: InheritMode) -> io::Result<()> {
    // The temp file is created with the caller's mode; persisting it over an
    // existing regular file would silently change that file's permissions, so
    // carry the target's mode across the rename to preserve it.
    //
    // `symlink_metadata` (not `metadata`) so a symlinked target is detected
    // rather than followed: the rename replaces the symlink with a fresh
    // regular file, and copying the link target's (possibly 0644) mode would
    // loosen permissions on freshly written credentials. A symlinked target gets
    // the creation mode.
    #[cfg(unix)]
    if matches!(inherit, InheritMode::Yes)
        && let Ok(metadata) = fs::symlink_metadata(path)
        && !metadata.file_type().is_symlink()
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = metadata.permissions().mode();
        file.set_permissions(std::fs::Permissions::from_mode(mode))?;
    }
    #[cfg(target_os = "wasi")]
    if matches!(inherit, InheritMode::Yes)
        && let Ok(mode) = crate::wasi_fs::path_mode(path)
        && mode & libc::S_IFMT != libc::S_IFLNK
    {
        crate::wasi_fs::set_file_mode(file, mode & 0o7777)?;
    }
    #[cfg(not(any(unix, target_os = "wasi")))]
    let _ = (path, file, inherit);
    Ok(())
}

#[cfg(test)]
mod tests;
