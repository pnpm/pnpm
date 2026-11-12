#[cfg(not(any(unix, windows)))]
use super::is_dir_no_follow;
#[cfg(windows)]
use super::windows;
use super::{ChildFileStat, ChildWrite, OpenDir, validate_child_name};
#[cfg(unix)]
use super::{c_name, unix};
#[cfg(unix)]
use std::os::fd::AsFd as _;
#[cfg(not(unix))]
use std::path::Path;
use std::{
    ffi::{OsStr, OsString},
    io,
    sync::atomic::{AtomicU64, Ordering},
};
#[cfg(not(any(unix, windows)))]
use std::{fs, io::ErrorKind, time::SystemTime};

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
        let name = validate_child_name(name)?;
        #[cfg(unix)]
        return unix::read_file_at(self.handle.as_fd(), &c_name(name)?);
        #[cfg(windows)]
        return windows::read_child_file(&self.pinned, name);
        #[cfg(not(any(unix, windows)))]
        {
            if self.child_file_stat(name)?.is_none() {
                return Ok(None);
            }
            match fs::read(self.path.join(name)) {
                Ok(bytes) => Ok(Some(bytes)),
                Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
                Err(error) => Err(error),
            }
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

#[cfg(not(unix))]
pub(in crate::open_dir) fn write_child_file_in_pinned_dir(
    dir_path: &Path,
    name: &OsStr,
    bytes: &[u8],
    mode: ChildWrite,
    mut is_link: impl FnMut() -> io::Result<bool>,
) -> io::Result<()> {
    use std::io::Write as _;

    let tmp_path = (0..16)
        .find_map(|_| {
            let candidate = dir_path.join(unique_child_tmp_name(name));
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&candidate)
            {
                Ok(mut file) => {
                    let result = file.write_all(bytes).and_then(|()| file.sync_all());
                    drop(file);
                    Some(
                        result
                            .map(|()| candidate)
                            .inspect_err(|_| {
                                let _ = std::fs::remove_file(&candidate);
                            }),
                    )
                }
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => None,
                Err(err) => Some(Err(err)),
            }
        })
        .unwrap_or_else(|| {
            Err(io::Error::new(std::io::ErrorKind::AlreadyExists, "temporary child collided"))
        })?;
    let dest = dir_path.join(name);
    let published = match mode {
        ChildWrite::CreateNew => std::fs::hard_link(&tmp_path, &dest),
        ChildWrite::Replace => {
            if is_link()? {
                Err(io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "refusing to replace symlink child",
                ))
            } else {
                std::fs::rename(&tmp_path, &dest)
            }
        }
    };
    let _ = std::fs::remove_file(&tmp_path);
    published
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
