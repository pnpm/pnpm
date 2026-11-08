use std::{fs, path::Path};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PathCompletionKind {
    Directories,
    FilesAndDirectories,
}

pub(super) fn complete_paths(
    cwd: &Path,
    value_prefix: &str,
    kind: PathCompletionKind,
) -> Vec<String> {
    let (dir_prefix, name_prefix) = split_path_prefix(value_prefix);
    let search_dir = if dir_prefix.is_empty() { cwd.to_path_buf() } else { cwd.join(dir_prefix) };
    let Ok(entries) = fs::read_dir(search_dir) else {
        return Vec::new();
    };
    let mut candidates: Vec<_> = entries
        .flatten()
        .filter_map(|entry| completion_candidate(&entry, dir_prefix, name_prefix, kind))
        .collect();
    candidates.sort();
    candidates.dedup();
    candidates
}

fn split_path_prefix(value_prefix: &str) -> (&str, &str) {
    match value_prefix.rfind(is_path_separator) {
        Some(index) => value_prefix.split_at(index + 1),
        None => ("", value_prefix),
    }
}

const fn is_path_separator(character: char) -> bool {
    character == '/' || (cfg!(windows) && character == '\\')
}

fn completion_candidate(
    entry: &fs::DirEntry,
    dir_prefix: &str,
    name_prefix: &str,
    kind: PathCompletionKind,
) -> Option<String> {
    let file_name = entry.file_name();
    let name = file_name.to_str()?;
    if (name.starts_with('.') && !name_prefix.starts_with('.')) || !name.starts_with(name_prefix) {
        return None;
    }
    match (kind, entry_is_directory(entry)) {
        (_, true) => Some(format!("{dir_prefix}{name}/")),
        (PathCompletionKind::FilesAndDirectories, false) => Some(format!("{dir_prefix}{name}")),
        (PathCompletionKind::Directories, false) => None,
    }
}

fn entry_is_directory(entry: &fs::DirEntry) -> bool {
    let Ok(file_type) = entry.file_type() else {
        return false;
    };
    file_type.is_dir() || (file_type.is_symlink() && entry.path().is_dir())
}
