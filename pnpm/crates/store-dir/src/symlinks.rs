//! Symlinks in the side-effects cache.
//!
//! A build output symlink is recorded as an `added` entry whose mode has the
//! [`SYMLINK_MODE`] file-type bits and whose CAFS content is the link target.
//! The content is a regular CAFS file, so a pnpm version that does not know
//! the entry type still verifies it like any other file. Such a version never
//! restores the entry, because the side-effects cache key names the diff
//! format (`SIDE_EFFECTS_FORMAT_KEY` in the graph hasher). A remote artifact
//! carries the same entry, as the shared-artifact protocol defines it.

pub use pnpm_shared_artifact_protocol::{SYMLINK_MODE, is_symlink_mode};

use crate::{CafsFileInfo, FilesMap};
use std::{collections::HashMap, fs, io, path::Path};

/// The target the side-effects cache records for a symlink at `link_path`,
/// or `None` when the link cannot be recorded.
///
/// The link must be at a plain relative path, outside the top-level
/// `node_modules` and not at `package.json`. A recordable target is
/// relative, climbs no higher than the package root, climbs only at its
/// start, and never names a `node_modules` directory. The last rule keeps a
/// restored link from resolving into the dependencies that pnpm links next to
/// the package, even through another restored link. Reserved names match in
/// any letter case, as they do on a case-insensitive filesystem.
#[must_use]
pub fn normalize_symlink_target(link_path: &str, target: &str) -> Option<String> {
    if !is_recordable_link_path(link_path) {
        return None;
    }
    if target.is_empty()
        || target.starts_with('/')
        || target.contains(['\\', '\0'])
        || matches!(target.as_bytes(), [drive, b':', ..] if drive.is_ascii_alphabetic())
    {
        return None;
    }
    let segments: Vec<&str> = target
        .split('/')
        .filter(|segment| !segment.is_empty() && *segment != ".")
        .collect();
    let parents = segments
        .iter()
        .take_while(|segment| **segment == "..")
        .count();
    let names = &segments[parents..];
    if names.is_empty()
        || names
            .iter()
            .any(|name| *name == ".." || name.eq_ignore_ascii_case("node_modules"))
    {
        return None;
    }
    let link_depth = link_path.split('/').count() - 1;
    (parents <= link_depth).then(|| segments.join("/"))
}

fn is_recordable_link_path(link_path: &str) -> bool {
    let segments: Vec<&str> = link_path.split('/').collect();
    segments
        .iter()
        .all(|segment| !matches!(*segment, "" | "." | "..") && !segment.contains(['\\', '\0']))
        && !segments[0].eq_ignore_ascii_case("node_modules")
        && !(segments.len() == 1 && segments[0].eq_ignore_ascii_case("package.json"))
}

/// Read the target of a recorded symlink from its CAFS file, or `None` when
/// the file is missing or does not hold a target in its recorded form.
pub(crate) fn read_recorded_symlink_target(
    link_path: &str,
    cas_path: &Path,
) -> io::Result<Option<String>> {
    let target = match fs::read_to_string(cas_path) {
        Ok(target) => target,
        Err(error)
            if matches!(error.kind(), io::ErrorKind::NotFound | io::ErrorKind::InvalidData) =>
        {
            return Ok(None);
        }
        Err(error) => return Err(error),
    };
    Ok((normalize_symlink_target(link_path, &target).as_deref() == Some(target.as_str()))
        .then_some(target))
}

/// Whether restoring `symlinks` next to `files` would write a file or a link
/// below one of the links, which would let the write follow that link.
pub(crate) fn writes_below_a_symlink<'a>(
    symlinks: &'a HashMap<String, String>,
    files: impl Iterator<Item = &'a String>,
) -> bool {
    files
        .chain(symlinks.keys())
        .any(|path| {
            path.match_indices('/')
                .any(|(slash, _)| symlinks.contains_key(&path[..slash]))
        })
}

/// A side-effects overlay: the post-build files resolved to CAFS paths, and
/// the symlinks the build created with their targets.
///
/// `symlinks` is empty off Unix, where an entry that records symlinks is
/// dropped so the package is built instead.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SideEffectsOverlay {
    pub files: FilesMap,
    pub symlinks: HashMap<String, String>,
}

impl From<FilesMap> for SideEffectsOverlay {
    fn from(files: FilesMap) -> Self {
        SideEffectsOverlay { files, symlinks: HashMap::new() }
    }
}

impl CafsFileInfo {
    #[must_use]
    pub fn is_symlink(&self) -> bool {
        is_symlink_mode(self.mode)
    }
}

#[cfg(test)]
mod tests;
