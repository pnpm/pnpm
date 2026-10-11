//! Read access to the files of a git-hosted package, wherever they are: a
//! checkout on disk while it is prepared, or the store's file map of a git
//! host's archive while it is resolved.

use std::{
    collections::HashMap,
    fs,
    io::Read as _,
    path::{Component, Path, PathBuf},
};

/// The files of one directory of a package, addressed by paths relative to
/// it with forward slashes.
pub trait PackageFiles {
    /// Whether `path` names a file.
    fn contains(&self, path: &str) -> bool;

    /// Up to `limit` leading bytes of the file at `path`, or `None` when it
    /// cannot be read.
    fn read_head(&self, path: &str, limit: u64) -> Option<Vec<u8>>;
}

/// Paths come from the package's own manifest, so one that is absolute or
/// climbs out of the directory names no file of it.
impl PackageFiles for Path {
    fn contains(&self, path: &str) -> bool {
        is_inside(path) && self.join(path).exists()
    }

    fn read_head(&self, path: &str, limit: u64) -> Option<Vec<u8>> {
        is_inside(path)
            .then(|| read_file_head(&self.join(path), limit))
            .flatten()
    }
}

fn is_inside(path: &str) -> bool {
    Path::new(path)
        .components()
        .all(|component| matches!(component, Component::Normal(_) | Component::CurDir))
}

/// A directory of an archive the store holds, as the map from each file's
/// path in the archive to its content-addressed copy.
pub struct CasPackageFiles<'a> {
    pub files: &'a HashMap<String, PathBuf>,
    /// The directory within the archive, as a resolution's `path` spells
    /// it (`/packages/foo`). `None` is the archive root.
    pub dir: Option<&'a str>,
}

impl CasPackageFiles<'_> {
    fn archive_path(&self, path: &str) -> Option<String> {
        if !is_inside(path) {
            return None;
        }
        let path = path.trim_start_matches("./").trim_end_matches('/');
        Some(match self.dir.unwrap_or("").trim_matches('/') {
            "" => path.to_string(),
            dir => format!("{dir}/{path}"),
        })
    }
}

impl PackageFiles for CasPackageFiles<'_> {
    /// A directory counts too, as it does on disk: `main` may name one.
    fn contains(&self, path: &str) -> bool {
        let Some(path) = self.archive_path(path) else { return false };
        let dir_prefix = format!("{path}/");
        self.files.contains_key(&path)
            || self.files
                .keys()
                .any(|file| file.starts_with(&dir_prefix))
    }

    fn read_head(&self, path: &str, limit: u64) -> Option<Vec<u8>> {
        read_file_head(self.files.get(&self.archive_path(path)?)?, limit)
    }
}

fn read_file_head(path: &Path, limit: u64) -> Option<Vec<u8>> {
    let mut head = Vec::new();
    fs::File::open(path)
        .ok()?
        .take(limit)
        .read_to_end(&mut head)
        .ok()?;
    Some(head)
}

#[cfg(test)]
mod tests;
