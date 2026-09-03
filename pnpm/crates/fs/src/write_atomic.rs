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
    write_tmp_over(path, bytes, InheritMode::Yes)
}

/// Like [`write_atomic`], but the result is only ever readable by its owner.
///
/// For a file whose contents are a credential, where [`write_atomic`]'s
/// mode-preserving rule is the wrong one: inheriting a target that happens to
/// be group- or world-readable would publish the secret being written to it.
/// The file `pnpm login` records a token in also holds ordinary settings, so
/// it can already exist with the mode an editor gave it.
pub fn write_atomic_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    write_tmp_over(path, bytes, InheritMode::No)
}

/// Whether the target's existing mode survives the rename. [`InheritMode::No`]
/// leaves the 0600 `NamedTempFile` creates.
#[derive(Clone, Copy)]
enum InheritMode {
    Yes,
    No,
}

fn write_tmp_over(path: &Path, bytes: &[u8], inherit: InheritMode) -> io::Result<()> {
    let dir = path.parent().filter(|parent| !parent.as_os_str().is_empty());
    if let Some(parent) = dir {
        fs::create_dir_all(parent)?;
    }
    let mut tmp = tempfile::NamedTempFile::new_in(dir.unwrap_or_else(|| Path::new(".")))?;
    tmp.write_all(bytes)?;
    tmp.as_file().sync_all()?;
    // `NamedTempFile` creates with mode 0600 on Unix; persisting it over an
    // existing regular file would silently tighten that file's permissions, so
    // carry the target's mode across the rename to preserve it.
    //
    // `symlink_metadata` (not `metadata`) so a symlinked target is detected
    // rather than followed: the rename replaces the symlink with a fresh
    // regular file, and copying the link target's (possibly 0644) mode would
    // loosen permissions on freshly written credentials. A symlink keeps 0600.
    #[cfg(unix)]
    if matches!(inherit, InheritMode::Yes)
        && let Ok(metadata) = fs::symlink_metadata(path)
        && !metadata.file_type().is_symlink()
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = metadata.permissions().mode();
        tmp.as_file().set_permissions(std::fs::Permissions::from_mode(mode))?;
    }
    #[cfg(not(unix))]
    let _ = inherit;
    tmp.persist(path).map_err(|err| err.error)?;
    Ok(())
}

#[cfg(test)]
mod tests;
