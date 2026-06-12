//! Compute the lockfile importer key for a workspace project.
//!
//! Mirrors upstream's
//! [`(path.relative(lockfileDir, p.rootDir) || '.').split(path.sep).join('/')`](https://github.com/pnpm/pnpm/blob/212315de16/pnpm/src/main.ts#L2469)
//! call site, used by `pkg-manager/core` to derive the `importers:` keys
//! the lockfile carries.

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
