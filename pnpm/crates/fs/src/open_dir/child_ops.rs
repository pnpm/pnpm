#[cfg(not(any(unix, windows)))]
use super::is_dir_no_follow;
#[cfg(windows)]
use super::windows;
use super::{ChildFileStat, ChildWrite, OpenDir, validate_child_name};
#[cfg(unix)]
use super::{c_name, unix};
#[cfg(unix)]
use std::os::fd::AsFd as _;
#[cfg(any(not(unix), test))]
use std::path::Path;
use std::{
    ffi::{OsStr, OsString},
    io::{self, ErrorKind},
    sync::atomic::{AtomicU64, Ordering},
};
#[cfg(not(any(unix, windows)))]
use std::{fs, time::SystemTime};

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

impl OpenDir {
    /// Inspect the regular file `name` directly inside this directory without
    /// following symbolic links. Returns `None` for directories, symbolic
    /// links, special files, and missing entries.
    pub fn child_file_stat(&self, name: &OsStr) -> io::Result<Option<ChildFileStat>> {
        let name = validate_child_name(name)?;
        #[cfg(unix)]
        return unix::stat_file_at(self.handle.as_fd(), &c_name(name)?);
        #[cfg(windows)]
        return windows::stat_child_file(self.pinned.path(), name);
        #[cfg(not(any(unix, windows)))]
        return fallback_child_file_stat(&self.path, name);
    }

    /// Read the regular file `name` directly inside this directory without
    /// following symbolic links. Returns `None` for directories, symbolic
    /// links, special files, and missing entries.
    pub fn read_child_file(&self, name: &OsStr) -> io::Result<Option<Vec<u8>>> {
        self.read_child_file_bounded(name, usize::MAX)
    }

    /// Read the regular file `name` directly inside this directory without
    /// following symbolic links, failing with [`ErrorKind::InvalidData`] if its
    /// length exceeds `max_bytes`. Returns `None` for directories, symbolic
    /// links, special files, and missing entries.
    pub fn read_child_file_bounded(
        &self,
        name: &OsStr,
        max_bytes: usize,
    ) -> io::Result<Option<Vec<u8>>> {
        let name = validate_child_name(name)?;
        #[cfg(unix)]
        return unix::read_file_at(self.handle.as_fd(), &c_name(name)?, max_bytes);
        #[cfg(windows)]
        return windows::read_child_file(&self.pinned, name, max_bytes);
        #[cfg(not(any(unix, windows)))]
        {
            let Some(stat) = self.child_file_stat(name)? else {
                return Ok(None);
            };
            ensure_within_max_bytes(stat.len, max_bytes)?;
            let file = match fs::File::open(self.path.join(name)) {
                Ok(file) => file,
                Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
                Err(error) => return Err(error),
            };
            read_bounded_bytes(file, max_bytes).map(Some)
        }
    }

    /// Atomically publish `bytes` at `name` inside this directory without
    /// replacing an existing entry or following symbolic links.
    pub fn create_child_file_new(&self, name: &OsStr, bytes: &[u8]) -> io::Result<()> {
        self.publish_child_file(name, bytes, ChildWrite::CreateNew)
    }

    /// Atomically write `bytes` to `name` inside this directory, refusing
    /// symbolic links or directories at `name`.
    pub fn write_child_file(&self, name: &OsStr, bytes: &[u8]) -> io::Result<()> {
        self.publish_child_file(name, bytes, ChildWrite::Replace)
    }

    fn publish_child_file(&self, name: &OsStr, bytes: &[u8], mode: ChildWrite) -> io::Result<()> {
        let name = validate_child_name(name)?;
        #[cfg(unix)]
        return unix::write_file_at(self.handle.as_fd(), name, &c_name(name)?, bytes, mode);
        #[cfg(windows)]
        return windows::write_child_file(&self.pinned, name, bytes, mode);
        #[cfg(not(any(unix, windows)))]
        return write_child_file_in_pinned_dir(&self.path, name, bytes, mode, || {
            self.has_symlink_child(name)
        });
    }

    /// Whether `name` directly inside this directory is a symbolic link or
    /// reparse point.
    pub fn has_symlink_child(&self, name: &OsStr) -> io::Result<bool> {
        let name = validate_child_name(name)?;
        #[cfg(unix)]
        return unix::is_symlink_at(self.handle.as_fd(), &c_name(name)?);
        #[cfg(windows)]
        return windows::is_reparse_child(self.pinned.path(), name);
        #[cfg(not(any(unix, windows)))]
        match fs::symlink_metadata(self.path.join(name)) {
            Ok(metadata) => Ok(metadata.file_type().is_symlink()),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }

    /// Remove the regular file `name` directly inside this directory without
    /// following symbolic links. Returns `false` when `name` is missing, a
    /// directory, or a symbolic link.
    pub fn remove_child_file(&self, name: &OsStr) -> io::Result<bool> {
        let name = validate_child_name(name)?;
        #[cfg(unix)]
        return unix::unlink_file_at(self.handle.as_fd(), &c_name(name)?);
        #[cfg(windows)]
        return windows::remove_child_file(&self.pinned, name);
        #[cfg(not(any(unix, windows)))]
        {
            if self.child_file_stat(name)?.is_none() {
                return Ok(false);
            }
            match fs::remove_file(self.path.join(name)) {
                Ok(()) => Ok(true),
                Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
                Err(error) => Err(error),
            }
        }
    }

    /// Remove the empty directory `name` directly inside this directory without
    /// following symbolic links. Returns `false` when `name` is non-empty,
    /// missing, a file, or a symbolic link.
    pub fn remove_empty_child_dir(&self, name: &OsStr) -> io::Result<bool> {
        let name = validate_child_name(name)?;
        #[cfg(unix)]
        return unix::unlink_empty_dir_at(self.handle.as_fd(), &c_name(name)?);
        #[cfg(windows)]
        return windows::remove_empty_child_dir(&self.pinned, name);
        #[cfg(not(any(unix, windows)))]
        return fallback_remove_empty_child_dir(&self.path, name);
    }
}

pub(in crate::open_dir) fn unique_child_tmp_name(name: &OsStr) -> OsString {
    let counter = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = crate::process_id();
    let mut random = [0u8; 8];
    let random = match getrandom::fill(&mut random) {
        Ok(()) => u64::from_ne_bytes(random),
        Err(_) => 0,
    };
    let mut tmp = name.to_os_string();
    tmp.push(format!(".tmp.{pid}.{counter}.{random:016x}"));
    tmp
}

pub(in crate::open_dir) fn ensure_within_max_bytes(len: u64, max_bytes: usize) -> io::Result<()> {
    if u64::try_from(max_bytes).is_ok_and(|limit| len > limit) {
        return Err(io::Error::new(ErrorKind::InvalidData, "child file exceeds maximum size"));
    }
    Ok(())
}

pub(in crate::open_dir) fn read_bounded_bytes(
    mut reader: impl io::Read,
    max_bytes: usize,
) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    if let Some(limit_plus_one) = u64::try_from(max_bytes)
        .ok()
        .and_then(|limit| limit.checked_add(1))
    {
        let mut bounded = io::Read::take(&mut reader, limit_plus_one);
        io::Read::read_to_end(&mut bounded, &mut bytes)?;
        if bytes.len() > max_bytes {
            return Err(io::Error::new(ErrorKind::InvalidData, "child file exceeds maximum size"));
        }
    } else {
        io::Read::read_to_end(&mut reader, &mut bytes)?;
    }
    Ok(bytes)
}

#[cfg(any(not(unix), test))]
pub(in crate::open_dir) fn write_child_file_in_pinned_dir(
    dir_path: &Path,
    name: &OsStr,
    bytes: &[u8],
    mode: ChildWrite,
    mut is_link: impl FnMut() -> io::Result<bool>,
) -> io::Result<()> {
    let tmp_path = create_temp_child_in_pinned_dir(dir_path, name, bytes)?;
    let dest = dir_path.join(name);
    let published = match mode {
        ChildWrite::CreateNew => {
            crate::retry::retry_transient_file_locks(|| std::fs::hard_link(&tmp_path, &dest))
        }
        ChildWrite::Replace => replace_pinned_dest(&tmp_path, &dest, &mut is_link),
    };
    match mode {
        ChildWrite::CreateNew => {
            let _ = crate::remove_file_with_retry(&tmp_path);
        }
        ChildWrite::Replace => {
            if published.is_err() {
                let _ = crate::remove_file_with_retry(&tmp_path);
            }
        }
    }
    published
}

#[cfg(any(not(unix), test))]
fn create_temp_child_in_pinned_dir(
    dir_path: &Path,
    name: &OsStr,
    bytes: &[u8],
) -> io::Result<std::path::PathBuf> {
    use std::io::Write as _;

    for _ in 0..16 {
        let candidate = dir_path.join(unique_child_tmp_name(name));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(mut file) => {
                let result = file.write_all(bytes).and_then(|()| crate::sync_file(&file));
                drop(file);
                if let Err(err) = result {
                    let _ = crate::remove_file_with_retry(&candidate);
                    return Err(err);
                }
                return Ok(candidate);
            }
            Err(err) if err.kind() == ErrorKind::AlreadyExists => {}
            Err(err) => return Err(err),
        }
    }
    Err(io::Error::new(ErrorKind::AlreadyExists, "temporary child collided"))
}

#[cfg(any(not(unix), test))]
fn replace_pinned_dest(
    tmp_path: &Path,
    dest: &Path,
    is_link: &mut impl FnMut() -> io::Result<bool>,
) -> io::Result<()> {
    if is_link()? {
        return Err(io::Error::new(ErrorKind::InvalidInput, "refusing to replace symlink child"));
    }
    crate::rename_with_retry(tmp_path, dest)
}

#[cfg(not(any(unix, windows)))]
fn fallback_child_file_stat(dir_path: &Path, name: &OsStr) -> io::Result<Option<ChildFileStat>> {
    let metadata = match fs::symlink_metadata(dir_path.join(name)) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if !metadata.file_type().is_file() {
        return Ok(None);
    }
    let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
    let accessed = metadata.accessed().unwrap_or(modified);
    Ok(Some(ChildFileStat { len: metadata.len(), modified, accessed }))
}

#[cfg(not(any(unix, windows)))]
fn fallback_remove_empty_child_dir(dir_path: &Path, name: &OsStr) -> io::Result<bool> {
    let path = dir_path.join(name);
    if !is_dir_no_follow(&path)? {
        return Ok(false);
    }
    match fs::remove_dir(&path) {
        Ok(()) => Ok(true),
        Err(error)
            if matches!(error.kind(), ErrorKind::NotFound | ErrorKind::DirectoryNotEmpty) =>
        {
            Ok(false)
        }
        Err(error) => Err(error),
    }
}
