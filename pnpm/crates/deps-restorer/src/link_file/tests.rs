mod reporting;

mod store;

mod links;

mod installation;

mod fallback;

use super::{FsCopyFile, FsHardLink, FsReflink, Host};
use pnpm_fs::{LinkFailure, StoreFile};
use std::{cell::Cell, fs, io, path::Path};

fn write_source(dir: &Path, name: &str, contents: &[u8]) -> StoreFile {
    let path = dir.join(name);
    fs::create_dir_all(path.parent().unwrap()).expect("create source parent");
    fs::write(&path, contents).expect("write source file");
    StoreFile::new(path)
}

/// A non-executable store entry, relative to the store directory.
const STORE_ENTRY: &str = "files/1b/59d9e5d4c3b2a1f0e9d8c7b6a5f4e3d2c1b0a9f8";

/// An executable store entry, relative to the store directory.
#[cfg(unix)]
const STORE_EXEC_ENTRY: &str = "files/1b/59d9e5d4c3b2a1f0e9d8c7b6a5f4e3d2c1b0a9f8-exec";

/// A reflink onto a fresh inode that carries the source's mode, which is
/// what a real one does: `clonefile` copies the source's attributes, and
/// the reflink tier sets them explicitly on Linux.
#[cfg(unix)]
struct FreshClone;

#[cfg(unix)]
impl FsHardLink for FreshClone {
    fn hard_link(_source: &Path, _target: &Path) -> io::Result<()> {
        unreachable!("the clone tier materializes the file, so no hardlink tier follows it")
    }
}

#[cfg(unix)]
impl FsReflink for FreshClone {
    fn reflink(source: &Path, target: &Path) -> io::Result<()> {
        fs::write(target, fs::read(source)?)?;
        fs::set_permissions(target, fs::metadata(source)?.permissions())
    }
}

#[cfg(unix)]
impl FsCopyFile for FreshClone {
    fn copy_file(_source: &Path, _target: &Path, _: &pnpm_fs::CopyPermissions) -> io::Result<()> {
        unreachable!("the clone tier materializes the file, so no copy tier follows it")
    }
}

/// A reflink whose target does not carry the source's mode. No real
/// reflink does this. It makes visible whether the clone tier aligned the
/// target afterwards.
#[cfg(unix)]
struct ModeDroppingClone;

#[cfg(unix)]
impl FsHardLink for ModeDroppingClone {
    fn hard_link(_source: &Path, _target: &Path) -> io::Result<()> {
        unreachable!("the clone tier materializes the file, so no hardlink tier follows it")
    }
}

#[cfg(unix)]
impl FsReflink for ModeDroppingClone {
    fn reflink(source: &Path, target: &Path) -> io::Result<()> {
        use std::os::unix::fs::PermissionsExt;
        fs::write(target, fs::read(source)?)?;
        fs::set_permissions(target, fs::Permissions::from_mode(0o400))
    }
}

#[cfg(unix)]
impl FsCopyFile for ModeDroppingClone {
    fn copy_file(_source: &Path, _target: &Path, _: &pnpm_fs::CopyPermissions) -> io::Result<()> {
        unreachable!("the clone tier materializes the file, so no copy tier follows it")
    }
}

#[cfg(unix)]
fn inode(path: &Path) -> u64 {
    use std::os::unix::fs::MetadataExt;
    fs::metadata(path).unwrap().ino()
}

/// The failures the [`Faulty`] import calls on this thread answer with.
#[derive(Debug, Clone, Copy, Default)]
struct Faults {
    hard_link: Option<LinkFailure>,
    reflink: Option<LinkFailure>,
    copy: Option<io::ErrorKind>,
}

thread_local! {
    static FAULTS: Cell<Faults> = const { Cell::new(Faults { hard_link: None, reflink: None, copy: None }) };
}

/// Import calls that fail as [`Faults`] says and otherwise succeed on any
/// test filesystem: the hardlink is a real one, the reflink a byte copy
/// that carries the source's mode as a real reflink does.
struct Faulty;

impl Faulty {
    fn set(faults: Faults) {
        FAULTS.with(|cell| cell.set(faults));
    }
}

impl FsHardLink for Faulty {
    fn hard_link(source: &Path, target: &Path) -> io::Result<()> {
        match FAULTS.with(Cell::get).hard_link {
            Some(failure) => Err(failure.sample_error()),
            None => fs::hard_link(source, target),
        }
    }
}

impl FsReflink for Faulty {
    fn reflink(source: &Path, target: &Path) -> io::Result<()> {
        match FAULTS.with(Cell::get).reflink {
            Some(failure) => Err(failure.sample_error()),
            None => fs::copy(source, target).map(|_| ()),
        }
    }
}

impl FsCopyFile for Faulty {
    fn copy_file(
        source: &Path,
        target: &Path,
        permissions: &pnpm_fs::CopyPermissions,
    ) -> io::Result<()> {
        match FAULTS.with(Cell::get).copy {
            Some(kind) => Err(io::Error::from(kind)),
            None => Host::copy_file(source, target, permissions),
        }
    }
}
