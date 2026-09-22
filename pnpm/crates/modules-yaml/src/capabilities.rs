use std::{
    fs,
    io,
    path::Path,
    time::SystemTime,
};

/// Capability trait: read a file's contents into a [`String`].
///
/// One trait per filesystem capability so each function declares only what
/// it actually uses, and so test fakes only implement the methods that
/// will be exercised. Pattern follows the per-capability typeclass style
/// rather than `parallel-disk-usage`'s lumped `FsApi` at
/// <https://github.com/KSXGitHub/parallel-disk-usage/blob/2aa39917f9/src/app/hdd.rs#L29-L35>.
pub trait FsReadToString {
    fn read_to_string(path: &Path) -> io::Result<String>;
}

/// Capability trait: create a directory and any missing parents.
pub trait FsCreateDirAll {
    fn create_dir_all(path: &Path) -> io::Result<()>;
}

/// Capability trait: write bytes to a file, replacing existing contents.
pub trait FsWrite {
    fn write(path: &Path, contents: &[u8]) -> io::Result<()>;
}

/// Capability trait: read the current wall-clock time as a [`SystemTime`].
///
/// Decoupled from [`SystemTime::now`] so tests can fake the clock and
/// assert deterministic `prunedAt` values.
pub trait Clock {
    fn now() -> SystemTime;
}

/// Production implementation, backed by [`std::fs`] and [`SystemTime::now`].
pub struct Host;

impl FsReadToString for Host {
    #[inline]
    fn read_to_string(path: &Path) -> io::Result<String> {
        fs::read_to_string(path)
    }
}

impl FsCreateDirAll for Host {
    #[inline]
    fn create_dir_all(path: &Path) -> io::Result<()> {
        fs::create_dir_all(path)
    }
}

impl FsWrite for Host {
    #[inline]
    fn write(path: &Path, contents: &[u8]) -> io::Result<()> {
        fs::write(path, contents)
    }
}

impl Clock for Host {
    #[inline]
    fn now() -> SystemTime {
        SystemTime::now()
    }
}
