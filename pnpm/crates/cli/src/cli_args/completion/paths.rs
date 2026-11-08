use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PathCompletionKind {
    Directories,
    FilesAndDirectories,
}

pub(super) const MAX_PATH_CANDIDATES: usize = 256;

pub(super) fn complete_paths(
    cwd: &Path,
    home_dir: Option<&Path>,
    value_prefix: &str,
    kind: PathCompletionKind,
) -> Vec<String> {
    let (dir_prefix, name_prefix) = split_path_prefix(value_prefix);
    let search_dir = resolve_search_dir(cwd, home_dir, dir_prefix);
    let Ok(entries) = fs::read_dir(search_dir) else {
        return Vec::new();
    };
    let mut candidates: Vec<_> = entries
        .flatten()
        .filter_map(|entry| completion_candidate(&entry, dir_prefix, name_prefix, kind))
        .take(MAX_PATH_CANDIDATES)
        .collect();
    candidates.sort();
    candidates.dedup();
    candidates
}

fn resolve_search_dir(cwd: &Path, home_dir: Option<&Path>, dir_prefix: &str) -> PathBuf {
    if dir_prefix.is_empty() {
        return cwd.to_path_buf();
    }
    if let Some(after_tilde) = strip_home_prefix(dir_prefix)
        && let Some(home) = home_dir
    {
        return if after_tilde.is_empty() { home.to_path_buf() } else { home.join(after_tilde) };
    }
    cwd.join(dir_prefix)
}

pub(super) fn resolve_directory_arg(
    cwd: &Path,
    home_dir: Option<&Path>,
    directory: &str,
) -> PathBuf {
    if let Some(unquoted) = strip_matching_quotes(directory) {
        return cwd.join(unquoted);
    }
    if directory == "~" {
        if let Some(home) = home_dir {
            return home.to_path_buf();
        }
    } else if let Some(after_tilde) = strip_home_prefix(directory)
        && let Some(home) = home_dir
    {
        return if after_tilde.is_empty() { home.to_path_buf() } else { home.join(after_tilde) };
    }
    cwd.join(directory)
}

fn strip_matching_quotes(value: &str) -> Option<&str> {
    value
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .or_else(|| {
            value
                .strip_prefix('\'')
                .and_then(|rest| rest.strip_suffix('\''))
        })
}

fn strip_home_prefix(dir_prefix: &str) -> Option<&str> {
    dir_prefix
        .strip_prefix("~/")
        .or_else(|| if cfg!(windows) { dir_prefix.strip_prefix(r"~\") } else { None })
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
