mod reporting;

mod store;

mod links;

mod installation;

#[cfg(unix)]
use super::Host;
use super::{FsHardLink, FsReflink};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

fn write_source(dir: &Path, name: &str, contents: &[u8]) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, contents).expect("write source file");
    path
}

/// A filesystem that refuses `link(2)` with `EPERM`: what a FUSE
/// filesystem without hardlinks (`EdenFS`) answers for every link inside
/// its mount. pnpm's store-to-checkout links usually fail `EXDEV` first
/// and retire the tier before the ladder reaches such a link, so this
/// is met when source and target both sit inside the mount, as the
/// `file:` packages a repeat install re-imports do. Reflink and copy
/// behave as the real filesystem does.
#[cfg(unix)]
struct EpermHardLink;

#[cfg(unix)]
impl FsHardLink for EpermHardLink {
    fn hard_link(_source: &Path, _target: &Path) -> io::Result<()> {
        Err(io::Error::from_raw_os_error(libc::EPERM))
    }
}

#[cfg(unix)]
impl FsReflink for EpermHardLink {
    fn reflink(source: &Path, target: &Path) -> io::Result<()> {
        Host::reflink(source, target)
    }
}

/// The user-namespace containers of pnpm/pnpm#14722, where `FICLONE`
/// is refused with `EPERM`. Hardlinks work there.
#[cfg(unix)]
struct EpermReflink;

#[cfg(unix)]
impl FsHardLink for EpermReflink {
    fn hard_link(source: &Path, target: &Path) -> io::Result<()> {
        Host::hard_link(source, target)
    }
}

#[cfg(unix)]
impl FsReflink for EpermReflink {
    fn reflink(_source: &Path, _target: &Path) -> io::Result<()> {
        Err(io::Error::from_raw_os_error(libc::EPERM))
    }
}

/// A filesystem that refuses both links with `EPERM`, so the `Auto`
/// ladder has only the copy tier left.
#[cfg(unix)]
struct EpermLinks;

#[cfg(unix)]
impl FsHardLink for EpermLinks {
    fn hard_link(_source: &Path, _target: &Path) -> io::Result<()> {
        Err(io::Error::from_raw_os_error(libc::EPERM))
    }
}

#[cfg(unix)]
impl FsReflink for EpermLinks {
    fn reflink(_source: &Path, _target: &Path) -> io::Result<()> {
        Err(io::Error::from_raw_os_error(libc::EPERM))
    }
}

#[cfg(unix)]
struct EaccesHardLink;

#[cfg(unix)]
impl FsHardLink for EaccesHardLink {
    fn hard_link(_source: &Path, _target: &Path) -> io::Result<()> {
        Err(io::Error::from_raw_os_error(libc::EACCES))
    }
}

#[cfg(unix)]
impl FsReflink for EaccesHardLink {
    fn reflink(source: &Path, target: &Path) -> io::Result<()> {
        Host::reflink(source, target)
    }
}

#[cfg(unix)]
fn inode(path: &Path) -> u64 {
    use std::os::unix::fs::MetadataExt;
    fs::metadata(path).unwrap().ino()
}

#[cfg(unix)]
struct EaccesLinks;

#[cfg(unix)]
impl FsHardLink for EaccesLinks {
    fn hard_link(source: &Path, target: &Path) -> io::Result<()> {
        EaccesHardLink::hard_link(source, target)
    }
}

#[cfg(unix)]
impl FsReflink for EaccesLinks {
    fn reflink(_source: &Path, _target: &Path) -> io::Result<()> {
        Err(io::Error::from_raw_os_error(libc::EACCES))
    }
}

/// A source that has run out of names: `EMLINK` on Unix,
/// `ERROR_TOO_MANY_LINKS` on Windows, one [`io::ErrorKind`] either way.
/// A store file linked into enough projects reaches NTFS's cap of 1024
/// names long before ext4's 65000.
struct OutOfLinks;

impl FsHardLink for OutOfLinks {
    fn hard_link(_source: &Path, _target: &Path) -> io::Result<()> {
        Err(io::Error::from(io::ErrorKind::TooManyLinks))
    }
}

impl FsReflink for OutOfLinks {
    fn reflink(_source: &Path, _target: &Path) -> io::Result<()> {
        unreachable!("the hardlink tier materializes the file itself, so no tier follows it")
    }
}
