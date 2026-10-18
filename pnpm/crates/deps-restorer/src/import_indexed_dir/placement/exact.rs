use super::all_files_match;
use std::{
    collections::{HashMap, HashSet},
    io,
    path::{Path, PathBuf},
};

pub(in crate::import_indexed_dir) fn all_imported_entries_match_exactly(
    dir_path: &Path,
    cas_paths: &HashMap<String, PathBuf>,
    preserve_node_modules: bool,
) -> bool {
    if !all_files_match(dir_path, cas_paths) {
        return false;
    }
    let mut actual =
        ImportedEntries { root: dir_path, preserve_node_modules, paths: HashSet::new() };
    if collect_imported_entries(dir_path, &mut actual).is_err() {
        return false;
    }
    let mut expected: HashSet<_> = cas_paths
        .keys()
        .filter(|entry| entry.as_str() != crate::NEEDS_BUILD_MARKER)
        .filter(|entry| !preserve_node_modules || !is_node_modules_entry(entry))
        .map(PathBuf::from)
        .collect();
    for entry in expected.clone() {
        let mut parent = entry.parent();
        while let Some(directory) = parent.filter(|directory| !directory.as_os_str().is_empty()) {
            expected.insert(directory.to_path_buf());
            parent = directory.parent();
        }
    }
    actual.paths == expected
}

struct ImportedEntries<'a> {
    root: &'a Path,
    preserve_node_modules: bool,
    paths: HashSet<PathBuf>,
}

fn collect_imported_entries(directory: &Path, entries: &mut ImportedEntries<'_>) -> io::Result<()> {
    for dirent in std::fs::read_dir(directory)? {
        let dirent = dirent?;
        let file_type = dirent.file_type()?;
        let path = dirent.path();
        if file_type.is_dir() {
            if directory == entries.root
                && path
                    .file_name()
                    .is_some_and(|name| name == "node_modules")
                && entries.preserve_node_modules
            {
                continue;
            }
            entries.paths.insert(
                path.strip_prefix(entries.root)
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?
                    .to_path_buf(),
            );
            collect_imported_entries(&path, entries)?;
        } else if path
            .file_name()
            .is_none_or(|name| name != crate::NEEDS_BUILD_MARKER)
        {
            entries.paths.insert(
                path.strip_prefix(entries.root)
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?
                    .to_path_buf(),
            );
        }
    }
    Ok(())
}

fn is_node_modules_entry(entry: &str) -> bool {
    Path::new(entry)
        .components()
        .next()
        .is_some_and(|part| part.as_os_str() == "node_modules")
}
