//! Per-capability dependency-injection traits and the production
//! [`Host`] provider, following the convention in
//! `pnpm/CODE_STYLE_GUIDE.md`: one trait per capability, no `&self` on
//! capability methods, and an explicit turbofish at production call
//! sites.

use crate::remove_dirent;
use std::{fs, io, path::Path};

/// Move a filesystem entry from `src` to `dst`, as [`fs::rename`].
///
/// The seam exists for [`fn@crate::rename_even_across_devices`], whose
/// fallback runs only when the kernel reports the two paths as being
/// on different filesystems — a condition no test can stage portably
/// on a single volume.
pub trait FsRename {
    fn rename(src: &Path, dst: &Path) -> io::Result<()>;
}

/// Remove a filesystem entry, as [`fn@crate::remove_dirent`].
///
/// The seam exists for [`fn@crate::rename_even_across_devices`], whose
/// contract turns on what happens when this fails after the move is
/// already committed — a state the real filesystem only reaches
/// through platform-specific permission tricks.
pub trait FsRemoveDirent {
    fn remove_dirent(path: &Path) -> io::Result<()>;
}

/// Clone a filesystem entry using copy-on-write, as [`reflink_copy::reflink`].
/// The destination must not exist. Restricted containers may deny cloning
/// while allowing ordinary copies, including pnpm/pnpm#14722.
pub trait FsReflink {
    fn reflink(source: &Path, target: &Path) -> io::Result<()>;
}

/// Create a hard link from `source` to `target`, as [`fs::hard_link`].
///
/// The seam exists so tests can hand callers the errors only some
/// filesystems return — `EPERM` from a FUSE mount that has no
/// hardlinks, `EXDEV` from a target on another mount — which a temp
/// dir on a single volume cannot reproduce.
pub trait FsHardLink {
    fn hard_link(source: &Path, target: &Path) -> io::Result<()>;
}

pub struct Host;

impl FsReflink for Host {
    fn reflink(source: &Path, target: &Path) -> io::Result<()> {
        reflink_copy::reflink(source, target)
    }
}

impl FsHardLink for Host {
    fn hard_link(source: &Path, target: &Path) -> io::Result<()> {
        fs::hard_link(source, target)
    }
}

impl FsRename for Host {
    fn rename(src: &Path, dst: &Path) -> io::Result<()> {
        fs::rename(src, dst)
    }
}

impl FsRemoveDirent for Host {
    fn remove_dirent(path: &Path) -> io::Result<()> {
        remove_dirent(path)
    }
}
