use std::{fs, io, path::Path};

/// Copy `source_path` to `target_path`, a path nothing may occupy yet,
/// with the permissions the copied file should carry.
///
/// The caller supplies the permissions so the target can be created
/// with the mode it will finish with, not the source's. The store
/// import tier computes that mode from the CAS `-exec` suffix and the
/// current umask rather than reading the source's on-disk mode, which
/// was fixed when the store was populated (pnpm/pnpm#3807).
///
/// The target is created exclusively, so a symlink squatting at the
/// path is never opened through: `O_EXCL` does not follow one, and the
/// call fails with `AlreadyExists` instead. Whatever such a link names
/// keeps its contents, and a link naming a path that does not exist
/// does not bring it into being.
///
/// Everything after the creation goes through that one handle rather
/// than the path — the bytes, the mode, and whatever `finish` does
/// with it. A writer that swaps the dirent mid-copy therefore cannot
/// redirect any of it onto a file elsewhere, the way re-opening
/// `target_path` by name could. The mode is also supplied at creation,
/// so a `0o600` mode is never briefly world-readable, and asserted
/// again at the end, because the umask can narrow the creation mode.
///
/// A failure past the creation leaves a partial file, which is
/// removed, but only while the path still names the file this call
/// created: a concurrent writer may have renamed a complete file over
/// it, and that one must survive.
pub fn copy_file_exclusive(
    source_path: &Path,
    target_path: &Path,
    permissions: &fs::Permissions,
    finish: impl FnOnce(&fs::File) -> io::Result<()>,
) -> io::Result<()> {
    let mut source = fs::File::open(source_path)?;
    let mut target = create_new_with_permissions(target_path, permissions)?;
    io::copy(&mut source, &mut target)
        .and_then(|_| target.set_permissions(permissions.clone()))
        .and_then(|()| finish(&target))
        .inspect_err(|_| {
            if path_still_names(&target, target_path) {
                let _ = fs::remove_file(target_path);
            }
        })
}

/// Copy `source_path` to `target_path`, so that a reader of the target
/// sees either what it held before or the whole copy, never a part.
///
/// The bytes go into a temp sibling that [`copy_file_exclusive`]
/// creates, which is then renamed over the target. The rename replaces
/// whatever the target holds, a symlink included, without following
/// it. A failure removes the temp file; a crash can leave one behind,
/// under a name nothing else uses.
/// Copy `source_path` to `target_path` atomically using `permissions` for the target.
pub fn copy_file_atomic_with_permissions(
    source_path: &Path,
    target_path: &Path,
    permissions: &fs::Permissions,
) -> io::Result<()> {
    let dir = target_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let temp = tempfile::Builder::new()
        .make_in(dir, |temp_path| {
            copy_file_exclusive(source_path, temp_path, permissions, |_| Ok(()))
        })?;
    let mut pending = Some(temp.into_temp_path());
    crate::retry::retry_transient_file_locks(|| {
        let temporary = pending.take().expect("temporary path retained after a failed persist");
        temporary
            .persist(target_path)
            .map_err(|error| {
                pending = Some(error.path);
                error.error
            })
    })
}

/// Copy `source_path` to `target_path`, so that a reader of the target
/// sees either what it held before or the whole copy, never a part.
///
/// The bytes go into a temp sibling that [`copy_file_exclusive`]
/// creates, which is then renamed over the target. The rename replaces
/// whatever the target holds, a symlink included, without following
/// it. A failure removes the temp file; a crash can leave one behind,
/// under a name nothing else uses.
pub fn copy_file_atomic(source_path: &Path, target_path: &Path) -> io::Result<()> {
    let permissions = fs::File::open(source_path)?.metadata()?.permissions();
    copy_file_atomic_with_permissions(source_path, target_path, &permissions)
}

/// Whether `path` still names the file `created` refers to.
///
/// Unix reads the identity straight out of the two stat results;
/// Windows keeps it behind an open handle, which `same-file` compares.
/// A path that has since been replaced, removed, or turned into a
/// symlink answers `false`.
fn path_still_names(created: &fs::File, path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let (Ok(created_meta), Ok(path_meta)) = (created.metadata(), fs::symlink_metadata(path))
        else {
            return false;
        };
        created_meta.ino() == path_meta.ino() && created_meta.dev() == path_meta.dev()
    }
    #[cfg(windows)]
    {
        let (Ok(clone), Ok(by_path)) = (created.try_clone(), same_file::Handle::from_path(path))
        else {
            return false;
        };
        same_file::Handle::from_file(clone).is_ok_and(|by_handle| by_handle == by_path)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (created, path);
        false
    }
}

/// Create `path`, failing if anything already occupies it, with the
/// mode the finished file will carry. The umask may still narrow it;
/// [`copy_file_exclusive`] asserts the exact mode once the bytes are
/// written.
#[cfg(unix)]
fn create_new_with_permissions(path: &Path, permissions: &fs::Permissions) -> io::Result<fs::File> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(permissions.mode())
        .open(path)
}

/// Windows carries no creation mode: the read-only attribute is the
/// whole of [`fs::Permissions`] there, and [`copy_file_exclusive`]
/// asserts it after the copy.
#[cfg(not(unix))]
fn create_new_with_permissions(
    path: &Path,
    _permissions: &fs::Permissions,
) -> io::Result<fs::File> {
    fs::File::create_new(path)
}

#[cfg(test)]
mod tests;
