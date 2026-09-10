//! Per-capability dependency-injection traits and the production
//! [`Host`] provider, following the convention in
//! `pnpm/CODE_STYLE_GUIDE.md`: one trait per capability, no `&self` on
//! capability methods, and an explicit turbofish at production call
//! sites.

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

pub struct Host;

impl FsRename for Host {
    fn rename(src: &Path, dst: &Path) -> io::Result<()> {
        fs::rename(src, dst)
    }
}
