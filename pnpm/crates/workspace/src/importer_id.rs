//! Compute the lockfile importer key for a workspace project.
//!
//! Derives the `importers:` keys the lockfile carries: the POSIX
//! relative path from the lockfile dir to each project's root dir, or
//! `"."` for the root importer.

use std::path::Path;

/// Returns `"."` for the root importer; otherwise the POSIX (forward-
/// slash) relative path from `lockfile_dir` to `project_dir`. Used as
/// the key into `Lockfile::importers` so both the lockfile writer and
/// `symlink_direct_dependencies::importer_root_dir` (the reverse
/// direction) agree on the spelling.
#[must_use]
pub fn importer_id_from_root_dir(lockfile_dir: &Path, project_dir: &Path) -> String {
    if project_dir == lockfile_dir {
        return ".".to_string();
    }
    // The overwhelmingly common shape — the project directly under the
    // root, both spelled cleanly — needs no `diff_paths` walk over the
    // two paths' long shared prefix: the suffix is the id. A suffix
    // carrying dot components (or a raw form that disagrees with its
    // components, like a trailing separator) takes the full math below.
    if let Ok(rel) = project_dir.strip_prefix(lockfile_dir)
        && let Some(rel_str) = rel.to_str()
        && !rel_str.is_empty()
        && rel_str.split(['/', '\\']).all(|segment| !matches!(segment, "" | "." | ".."))
    {
        return rel_str.replace('\\', "/");
    }
    match pathdiff::diff_paths(project_dir, lockfile_dir) {
        Some(rel) => {
            let rendered = rel.to_string_lossy().into_owned();
            if rendered.is_empty() || rendered == "." {
                ".".to_string()
            } else {
                rendered.replace('\\', "/")
            }
        }
        None => project_dir.to_string_lossy().replace('\\', "/"),
    }
}

#[cfg(test)]
mod tests;
