//! Reading a file as a git revision holds it.

use crate::{filter::FilterError, get_changed_projects::strip_final_newline, process::Command};
use std::{collections::HashSet, path::Path};

/// How many symlinks resolving one path may follow, as on Linux.
const MAX_SYMLINK_HOPS: usize = 40;
/// The longest path resolving may produce, in bytes.
const MAX_PATH_LEN: usize = 4096;

/// The content of `rel_path` at `commit`, following symlinks within that
/// revision the way the working-tree reader follows them on disk.
pub(super) fn read_git_file(
    commit: &str,
    workspace_dir: &Path,
    rel_path: &Path,
) -> Result<Option<String>, FilterError> {
    match resolve_git_path(commit, workspace_dir, rel_path)? {
        Some(rel_str) => show_git_file(commit, workspace_dir, &rel_str),
        None => Ok(None),
    }
}

/// `rel_path` with every symlink among its components resolved in the tree
/// of `commit`. `None` when it leaves the repository. Following more than
/// [`MAX_SYMLINK_HOPS`] symlinks, or growing past [`MAX_PATH_LEN`], fails.
fn resolve_git_path(
    commit: &str,
    workspace_dir: &Path,
    rel_path: &Path,
) -> Result<Option<String>, FilterError> {
    let Some(mut path) = normalize_git_path(rel_path) else {
        return Ok(None);
    };
    for _ in 0..=MAX_SYMLINK_HOPS {
        if path.len() > MAX_PATH_LEN {
            break;
        }
        let Some((link, rest)) = first_git_symlink(commit, workspace_dir, &path)? else {
            return Ok(Some(path));
        };
        let Some(target) = show_git_file(commit, workspace_dir, &link)? else {
            return Ok(None);
        };
        if Path::new(&target).is_absolute() {
            return Ok(None);
        }
        let parent = Path::new(&link).parent().unwrap_or_else(|| Path::new(""));
        let Some(next) = normalize_git_path(&parent.join(target).join(rest)) else {
            return Ok(None);
        };
        path = next;
    }
    Err(FilterError::FilterChanged {
        stderr: format!("{}: too many levels of symbolic links", rel_path.display()),
    })
}

/// The first prefix of `path` that is a symlink in the tree of `commit`, and
/// the rest of `path` after it. One `git ls-tree` lists every prefix.
fn first_git_symlink(
    commit: &str,
    workspace_dir: &Path,
    path: &str,
) -> Result<Option<(String, String)>, FilterError> {
    const SYMLINK_MODE: &str = "120000 ";
    let prefixes: Vec<&str> = path
        .match_indices('/')
        .map(|(index, _)| &path[..index])
        .chain(std::iter::once(path))
        .filter(|prefix| !prefix.is_empty())
        .collect();
    if prefixes.is_empty() {
        return Ok(None);
    }
    let output = Command::new("git")
        .args(["ls-tree", "--full-tree", "-z", "--end-of-options", commit, "--"])
        .args(&prefixes)
        .current_dir(workspace_dir)
        .env("LC_ALL", "C")
        .output()
        .map_err(|err| FilterError::FilterChanged { stderr: err.to_string() })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(FilterError::FilterChanged {
            stderr: strip_final_newline(&stderr).to_string(),
        });
    }
    let listing = String::from_utf8_lossy(&output.stdout);
    let symlinks: HashSet<&str> = listing
        .split('\0')
        .filter(|entry| entry.starts_with(SYMLINK_MODE))
        .filter_map(|entry| entry.split_once('\t').map(|(_, entry_path)| entry_path))
        .collect();
    Ok(prefixes
        .into_iter()
        .find(|prefix| symlinks.contains(prefix))
        .map(|link| {
            let rest = path[link.len()..].trim_start_matches('/');
            (link.to_string(), rest.to_string())
        }))
}

fn show_git_file(
    commit: &str,
    workspace_dir: &Path,
    rel_str: &str,
) -> Result<Option<String>, FilterError> {
    let output = Command::new("git")
        .args(["show", "--end-of-options", &format!("{commit}:{rel_str}")])
        .current_dir(workspace_dir)
        .env("LC_ALL", "C")
        .output()
        .map_err(|err| FilterError::FilterChanged { stderr: err.to_string() })?;
    if output.status.success() {
        let stdout = String::from_utf8(output.stdout)
            .map_err(|err| FilterError::FilterChanged { stderr: err.to_string() })?;
        Ok(Some(stdout))
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("does not exist in") || stderr.contains("exists on disk, but not in") {
            Ok(None)
        } else {
            Err(FilterError::FilterChanged { stderr: strip_final_newline(&stderr).to_string() })
        }
    }
}

/// `path` as a git revision path: forward slashes, with `.` and `..`
/// resolved, since git does not resolve them. `None` for a path outside the
/// repository.
fn normalize_git_path(path: &Path) -> Option<String> {
    let mut segments: Vec<String> = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                segments.pop()?;
            }
            std::path::Component::Normal(segment) => {
                segments.push(segment.to_string_lossy().into_owned());
            }
            _ => {}
        }
    }
    Some(segments.join("/"))
}
