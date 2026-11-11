use super::{ForceSymlinkOutcome, TriedOnce, force_symlink_inner};
use std::{fs, io, path::Path};

/// [`force_symlink_inner`] behind a read of the existing link, so a link that
/// already points at `target` is reused without a create attempt.
///
/// Most links an install writes already exist and are up to date. On macOS
/// (APFS) a `symlink()` that fails with `EEXIST` costs an order of magnitude
/// more than a `readlink()` and does not scale across threads.
pub(super) fn force_symlink(
    target: &Path,
    link: &Path,
    create_symlink: fn(&Path, &Path) -> io::Result<()>,
) -> io::Result<ForceSymlinkOutcome> {
    if read_reusable_link(link)
        .is_ok_and(|existing| existing_symlink_up_to_date(target, link, &existing))
    {
        return Ok(ForceSymlinkOutcome { reused: true, warning: None });
    }
    force_symlink_inner(target, link, TriedOnce::default(), create_symlink)
}

/// The contents of the link at `link`, or an error when it isn't a link that
/// could be reused. On a Windows network share only the kind of link that can
/// be opened there is reused.
fn read_reusable_link(link: &Path) -> io::Result<std::path::PathBuf> {
    #[cfg(windows)]
    if let Some(contents) = super::windows::read_share_link(link) {
        return contents;
    }
    fs::read_link(link)
}

/// Whether the link at `link`, holding `existing`, can stay as it is: it
/// points at `wanted`, and on a Windows network share it is the kind of link
/// that can be opened there. This is the check after a
/// create conflict. [`force_symlink`] checks the same without an extra open.
pub(super) fn existing_link_reusable(wanted: &Path, link: &Path, existing: &Path) -> bool {
    if !existing_symlink_up_to_date(wanted, link, existing) {
        return false;
    }
    #[cfg(windows)]
    return super::windows::is_link_kind_usable(link);
    #[cfg(not(windows))]
    true
}

/// Lexical "does the existing link resolve to the wanted target?"
/// check: resolve the existing link's contents to an absolute path
/// (using `link`'s parent dir when the contents are relative), then
/// compare lexically against `wanted`. Single-level — does not follow
/// chained symlinks.
///
/// Both sides pass through [`fn@crate::lexical_normalize`] before
/// comparing. The `..` segments in the relative link contents
/// [`symlink_dir`](super::symlink_dir) writes must collapse before the comparison;
/// without that, every up-to-date relative symlink reads as stale and
/// pays an unlink + recreate.
pub(super) fn existing_symlink_up_to_date(
    wanted: &Path,
    link: &Path,
    existing_link_string: &Path,
) -> bool {
    let existing_absolute = if existing_link_string.is_absolute() {
        existing_link_string.to_path_buf()
    } else {
        link.parent()
            .unwrap_or_else(|| Path::new(""))
            .join(existing_link_string)
    };
    crate::lexical_normalize(&existing_absolute) == crate::lexical_normalize(wanted)
}
