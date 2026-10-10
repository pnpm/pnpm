use super::placement::{
    file_identity_matches,
    symlinks::{imported_paths, is_symlink},
};
use std::{
    collections::{HashMap, HashSet},
    fs, io,
    path::{Component, Path, PathBuf},
};

pub(super) fn directory_matches(
    target: &Path,
    files: &HashMap<String, PathBuf>,
    keep_modules_dir: bool,
) -> bool {
    if files.is_empty() || files.contains_key(crate::NEEDS_BUILD_MARKER) {
        return false;
    }
    if !pnpm_fs::symlink_metadata_with_retry(target)
        .is_ok_and(|meta| meta.is_dir() && !is_symlink(target))
    {
        return false;
    }
    let Some(expected) = ExpectedTree::new(files, keep_modules_dir) else { return false };
    expected.matches(target).unwrap_or(false)
}

struct ExpectedTree<'a> {
    files: HashMap<PathBuf, &'a PathBuf>,
    directories: HashSet<PathBuf>,
    keep_modules_dir: bool,
}

enum EntryKind {
    File,
    Directory,
    Preserved,
}

impl<'a> ExpectedTree<'a> {
    fn new(files: &'a HashMap<String, PathBuf>, keep_modules_dir: bool) -> Option<Self> {
        let directories = imported_paths(files)
            .into_iter()
            .filter(|entry| !files.contains_key(*entry))
            .map(PathBuf::from)
            .collect();
        let mut expected = ExpectedTree {
            files: HashMap::with_capacity(files.len()),
            directories,
            keep_modules_dir,
        };
        for (relative, source) in files {
            let path = Path::new(relative);
            if path.as_os_str().is_empty()
                || !path
                    .components()
                    .all(|part| matches!(part, Component::Normal(_)))
                || path.starts_with("node_modules")
            {
                return None;
            }
            if expected.files.insert(path.to_path_buf(), source).is_some() {
                return None;
            }
        }
        Some(expected)
    }

    fn matches(&self, root: &Path) -> io::Result<bool> {
        let mut pending = vec![PathBuf::new()];
        let mut matched = 0;
        while let Some(relative) = pending.pop() {
            let Some(count) = self.matching_files_in_directory(root, &relative, &mut pending)?
            else {
                return Ok(false);
            };
            matched += count;
        }
        Ok(matched == self.files.len())
    }

    fn matching_files_in_directory(
        &self,
        root: &Path,
        relative: &Path,
        pending: &mut Vec<PathBuf>,
    ) -> io::Result<Option<usize>> {
        let mut matched = 0;
        for entry in fs::read_dir(root.join(relative))? {
            let entry = entry?;
            let path = relative.join(entry.file_name());
            match self.entry_kind(&path, &entry.path())? {
                Some(EntryKind::File) => matched += 1,
                Some(EntryKind::Directory) => pending.push(path),
                Some(EntryKind::Preserved) => {}
                None => return Ok(None),
            }
        }
        Ok(Some(matched))
    }

    fn entry_kind(&self, relative: &Path, target: &Path) -> io::Result<Option<EntryKind>> {
        let metadata = fs::symlink_metadata(target)?;
        if metadata.file_type().is_symlink() || (metadata.is_dir() && is_symlink(target)) {
            return Ok(None);
        }
        if metadata.is_dir() {
            if self.keep_modules_dir && relative == Path::new("node_modules") {
                return Ok(Some(EntryKind::Preserved));
            }
            return Ok(self.directories.contains(relative).then_some(EntryKind::Directory));
        }
        let Some(source) = self.files.get(relative) else { return Ok(None) };
        let source_metadata = fs::symlink_metadata(source)?;
        Ok(file_identity_matches((target, &metadata), (source, &source_metadata))
            .then_some(EntryKind::File))
    }
}
