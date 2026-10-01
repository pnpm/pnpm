use crate::{
    FsCreateDirAll, FsEnsureExecutableBits, FsReadDir, FsReadFile, FsReadHead, FsReadToString,
    FsSetExecutable, FsWalkFiles, FsWrite, Host,
};
use std::{
    io,
    path::{Path, PathBuf},
    sync::Mutex,
};

pub(super) static READ_DIRS: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
pub(super) static READ_MANIFESTS: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
pub(super) static READ_HEADS: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
pub(super) struct Counting;

impl FsReadDir for Counting {
    fn read_dir(path: &Path) -> io::Result<impl Iterator<Item = PathBuf>> {
        READ_DIRS
            .lock()
            .unwrap()
            .push(path.to_owned());
        Host::read_dir(path)
    }
}
impl FsReadFile for Counting {
    fn read_file(path: &Path) -> io::Result<Vec<u8>> {
        READ_MANIFESTS
            .lock()
            .unwrap()
            .push(path.to_owned());
        Host::read_file(path)
    }
}
impl FsReadHead for Counting {
    fn read_head(path: &Path, offset: u64, buffer: &mut [u8]) -> io::Result<usize> {
        READ_HEADS
            .lock()
            .unwrap()
            .push(path.to_owned());
        Host::read_head(path, offset, buffer)
    }
}
impl FsReadToString for Counting {
    fn read_to_string(path: &Path) -> io::Result<String> {
        Host::read_to_string(path)
    }
}
impl FsWalkFiles for Counting {
    fn walk_files(path: &Path) -> io::Result<impl Iterator<Item = PathBuf>> {
        Host::walk_files(path)
    }
}
impl FsCreateDirAll for Counting {
    fn create_dir_all(path: &Path) -> io::Result<()> {
        Host::create_dir_all(path)
    }
}
impl FsWrite for Counting {
    fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
        Host::write(path, bytes)
    }
    fn write_replace(path: &Path, bytes: &[u8]) -> io::Result<()> {
        Host::write_replace(path, bytes)
    }
}
impl FsSetExecutable for Counting {
    fn set_executable(path: &Path) -> io::Result<()> {
        Host::set_executable(path)
    }
}
impl FsEnsureExecutableBits for Counting {
    fn ensure_executable_bits(path: &Path, modules: Option<&Path>) -> io::Result<()> {
        Host::ensure_executable_bits(path, modules)
    }
}
