use std::{
    fs,
    io::{self, Write as _},
    path::Path,
};

/// Atomically replace `path` with `bytes`: write a sibling temp file, then
/// rename it over the target, creating parent directories as needed. A crash
/// or I/O failure mid-write leaves the original file intact rather than a
/// truncated one. Mirrors `write-file-atomic`.
///
/// Symlink-safe and permission-preserving: an existing regular target's mode
/// is carried across the rename, while a symlinked target is replaced with a
/// fresh regular file (the link is never followed, so the write can't be
/// redirected to an unintended path). A new file gets 0666 less the umask,
/// the mode `fs::write` gives it, so other users can read what pnpm writes
/// into a project.
///
/// A file that holds or may come to hold a credential uses
/// [`write_atomic_private_if_new`] or [`write_atomic_private`] instead.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_tmp_over(path, bytes, InheritMode::Yes, 0o666)
}

/// Like [`write_atomic`], but a new file is readable only by its owner.
///
/// For config files such as `.npmrc` and `auth.ini` that may come to hold a
/// token. An existing file keeps the mode its owner gave it.
pub fn write_atomic_private_if_new(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_tmp_over(path, bytes, InheritMode::Yes, 0o600)
}

/// Like [`write_atomic`], but the result is only ever readable by its owner.
///
/// For a file whose contents are a credential, where [`write_atomic`]'s
/// mode-preserving rule is the wrong one: inheriting a target that happens to
/// be group- or world-readable would publish the secret being written to it.
/// The file `pnpm login` records a token in also holds ordinary settings, so
/// it can already exist with the mode an editor gave it.
pub fn write_atomic_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
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

fn write_tmp_over(path: &Path, bytes: &[u8], inherit: InheritMode, mode: u32) -> io::Result<()> {
    let dir = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    if let Some(parent) = dir {
        fs::create_dir_all(parent)?;
    }
    // Registered before the create, so no interrupt finds the temp file
    // unregistered. A colliding random name is retried under a new one,
    // whose registration replaces the old.
    let inherited = match inherit {
        InheritMode::Yes => existing_mode(path),
        InheritMode::No => None,
    };
    // A replacement starts at the mode it will end with, so its contents are
    // never readable by more users than the file it replaces.
    let creation_mode = inherited.map_or(mode, |existing| existing & 0o777);
    let mut _pending_temp = None;
    let mut tmp = tempfile::Builder::new()
        .make_in(dir.unwrap_or_else(|| Path::new(".")), |path| {
            _pending_temp = Some(crate::pending_temp::track_temp_file(path));
            create_new_file(path, creation_mode)
        })?;
    tmp.write_all(bytes)?;
    crate::sync_file(tmp.as_file())?;
    if let Some(existing) = inherited {
        set_mode(tmp.as_file(), existing)?;
    }
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

/// The mode of an existing regular file at `path`, which a replacement keeps.
///
/// `symlink_metadata` (not `metadata`) so a symlinked target is detected
/// rather than followed: the rename replaces the symlink with a fresh regular
/// file, and copying the link target's (possibly 0644) mode would loosen
/// permissions on freshly written credentials. A symlinked target gets the
/// creation mode.
#[cfg(unix)]
fn existing_mode(path: &Path) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::symlink_metadata(path)
        .ok()
        .filter(|metadata| !metadata.file_type().is_symlink())
        .map(|metadata| metadata.permissions().mode())
}

#[cfg(target_os = "wasi")]
fn existing_mode(path: &Path) -> Option<u32> {
    crate::wasi_fs::path_mode(path)
        .ok()
        .filter(|mode| mode & libc::S_IFMT != libc::S_IFLNK)
        .map(|mode| mode & 0o7777)
}

#[cfg(not(any(unix, target_os = "wasi")))]
fn existing_mode(_path: &Path) -> Option<u32> {
    None
}

#[cfg(unix)]
fn set_mode(file: &fs::File, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    file.set_permissions(fs::Permissions::from_mode(mode))
}

#[cfg(target_os = "wasi")]
fn set_mode(file: &fs::File, mode: u32) -> io::Result<()> {
    crate::wasi_fs::set_file_mode(file, mode)
}

#[cfg(not(any(unix, target_os = "wasi")))]
fn set_mode(_file: &fs::File, _mode: u32) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests;
