//! Glob-expand `packages:` from `pnpm-workspace.yaml` into the
//! workspace's [`Project`] list.
//!
//! Out of scope (tracked as parity follow-ups):
//!
//! - `engines` / `os` / `cpu` installability filtering. Issue [#431]
//!   explicitly defers this.
//! - The `resolutions`-on-non-root warning. Single-line emission that
//!   can land when the reporter side is in place.
//! - Real-path resolution of `rootDir` for case-insensitive
//!   filesystems. Same divergence as [`root_finder`].
//!
//! [`root_finder`]: super::root_finder
//!
//! [#431]: https://github.com/pnpm/pacquet/issues/431

use crate::project_manifest::{ReadProjectManifestError, read_exact_project_manifest};
use derive_more::{Display, Error};
use miette::Diagnostic;
use pacquet_package_manifest::{PackageManifest, PackageManifestError};
use std::{
    collections::BTreeSet,
    io::ErrorKind,
    path::{Path, PathBuf},
};
use wax::{
    Glob,
    walk::{Entry, FileIterator},
};

/// A project discovered under the workspace root.
///
/// Pacquet keeps this shape narrower than pnpm's project type (which
/// also carries `rootDirRealPath`, `modulesDir`, etc.). The fields here
/// are what `pacquet-package-manager` actually needs at install time;
/// anything else is read on demand from the manifest. If a caller
/// needs more, extend here rather than reaching back into the
/// `package.json` value directly.
pub struct Project {
    pub root_dir: PathBuf,
    pub manifest: PackageManifest,
}

/// Options for [`find_workspace_projects`].
#[derive(Debug, Default, Clone)]
pub struct FindWorkspaceProjectsOpts {
    /// Package discovery patterns. When `None`, the lower-level
    /// enumeration falls back to `['.', '**']`. Callers enumerating a
    /// real workspace manifest should pass
    /// [`crate::workspace_package_patterns`] instead.
    pub patterns: Option<Vec<String>>,
}

/// Error type of the public entry points.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum FindWorkspaceProjectsError {
    #[display("Invalid glob pattern in pnpm-workspace.yaml packages: {pattern:?}: {message}")]
    #[diagnostic(code(pacquet_workspace::invalid_glob))]
    InvalidGlob {
        pattern: String,
        // Built once at construction. wax errors carry a borrow of the
        // input glob, so flatten to a string here for ergonomic storage.
        message: String,
    },

    #[display("Failed to walk workspace projects under {}: {source}", root.display())]
    #[diagnostic(code(pacquet_workspace::walk_error))]
    Walk {
        root: PathBuf,
        #[error(source)]
        source: std::io::Error,
    },

    #[diagnostic(transparent)]
    ReadManifest(#[error(source)] ReadProjectManifestError),
}

/// Find every project under `workspace_root` matching `opts.patterns`.
///
/// The per-project installability and non-root-manifest validations are
/// explicitly deferred by [#431]. When validation lands, this entry
/// point grows the filter; today it's a thin wrapper over
/// [`find_workspace_projects_no_check`].
///
/// [#431]: https://github.com/pnpm/pacquet/issues/431
pub fn find_workspace_projects(
    workspace_root: &Path,
    opts: &FindWorkspaceProjectsOpts,
) -> Result<Vec<Project>, FindWorkspaceProjectsError> {
    find_workspace_projects_no_check(workspace_root, opts)
}

/// Skip-validation variant.
pub fn find_workspace_projects_no_check(
    workspace_root: &Path,
    opts: &FindWorkspaceProjectsOpts,
) -> Result<Vec<Project>, FindWorkspaceProjectsError> {
    // When no patterns were configured, search the workspace root
    // non-recursively *and* recursively. The two-pattern fallback fires
    // only on `None`, not on `Some(vec![])` — an explicit empty array
    // means "enumerate only the workspace root" (which is
    // unconditionally added below per
    // <https://github.com/pnpm/pnpm/issues/1986>).
    let default_patterns = [".".to_string(), "**".to_string()];
    let patterns: &[String] = match opts.patterns.as_deref() {
        Some(p) => p,
        None => &default_patterns,
    };

    // `!`-prefixed patterns are negations. wax does not accept `!`
    // inside `Glob::new()`, so split them out and feed them through
    // `.not()` instead. `!/...` remains a no-op: relative workspace
    // paths never match that absolute form.
    let mut include_patterns: Vec<&str> = Vec::new();
    let mut user_negation_globs: Vec<String> = Vec::new();
    for pattern in patterns {
        if let Some(body) = pattern.strip_prefix('!') {
            if body.starts_with('/') {
                continue;
            }
            for normalized in normalize_manifest_patterns(body) {
                Glob::new(&normalized).map_err(|err| FindWorkspaceProjectsError::InvalidGlob {
                    pattern: pattern.clone(),
                    message: err.to_string(),
                })?;
                user_negation_globs.push(normalized);
            }
        } else {
            include_patterns.push(pattern);
        }
    }

    // wax's `not` takes a single pattern; combine the ignores with
    // `wax::any` so the walk filters them all in one pass (ignoring
    // `**/node_modules/**` and `**/bower_components/**`).
    // Built once outside the per-pattern loop and `.clone()`-d into each
    // `Walk::not` call (both `Glob` and `Any` derive `Clone` in wax),
    // since `IGNORE_PATTERNS` is a constant and reparsing it on every
    // user-supplied pattern is wasted work.
    let ignore_template = wax::any(
        IGNORE_PATTERNS
            .iter()
            .copied()
            .chain(user_negation_globs.iter().map(std::string::String::as_str)),
    )
    .map_err(|err| FindWorkspaceProjectsError::InvalidGlob {
        pattern: "<built-in ignore>".to_string(),
        message: err.to_string(),
    })?;

    let mut manifest_paths: BTreeSet<PathBuf> = BTreeSet::new();
    for pattern in include_patterns {
        for normalized in normalize_manifest_patterns(pattern) {
            if is_literal_pattern(&normalized) && !workspace_root.join(&normalized).is_file() {
                continue;
            }
            let glob =
                Glob::new(&normalized).map_err(|err| FindWorkspaceProjectsError::InvalidGlob {
                    pattern: pattern.to_string(),
                    message: err.to_string(),
                })?;

            let walk = glob.walk(workspace_root).not(ignore_template.clone()).map_err(|err| {
                FindWorkspaceProjectsError::InvalidGlob {
                    pattern: pattern.to_string(),
                    message: err.to_string(),
                }
            })?;

            for entry in walk {
                let entry = entry.map_err(|err| FindWorkspaceProjectsError::Walk {
                    root: workspace_root.to_path_buf(),
                    source: std::io::Error::other(err.to_string()),
                })?;
                manifest_paths.insert(entry.path().to_path_buf());
            }
        }
    }

    for basename in PROJECT_MANIFEST_BASENAMES {
        let root_manifest = workspace_root.join(basename);
        if root_manifest.is_file() {
            manifest_paths.insert(root_manifest);
        }
    }

    // Sort lexicographically by `rootDir` (= parent of the manifest).
    let mut sorted: Vec<PathBuf> = manifest_paths.into_iter().collect();
    sorted.sort_by(|left, right| {
        let dir_left = left.parent().unwrap_or_else(|| Path::new(""));
        let dir_right = right.parent().unwrap_or_else(|| Path::new(""));
        dir_left.cmp(dir_right)
    });

    let mut projects = Vec::with_capacity(sorted.len());
    let mut seen_roots = BTreeSet::new();
    for manifest_path in sorted {
        let root_dir = manifest_path.parent().unwrap_or(workspace_root).to_path_buf();
        if seen_roots.contains(&root_dir) {
            continue;
        }
        let manifest = match read_exact_project_manifest(&manifest_path) {
            Ok(m) => m,
            // Swallow ENOENT mid-walk (a file vanished between listing
            // and reading). This is the exact-and-only carve-out: parse
            // errors, permission failures, and "is a directory" must
            // still propagate so a malformed `package.json` surfaces as
            // a diagnostic instead of being silently dropped from the
            // workspace.
            Err(ReadProjectManifestError::Read(PackageManifestError::Io(err)))
                if err.kind() == ErrorKind::NotFound =>
            {
                continue;
            }
            Err(ReadProjectManifestError::ReadFile { source, .. })
                if source.kind() == ErrorKind::NotFound =>
            {
                continue;
            }
            Err(ReadProjectManifestError::Read(PackageManifestError::NoImporterManifestFound(
                _,
            ))) => continue,
            Err(err) => return Err(FindWorkspaceProjectsError::ReadManifest(err)),
        };
        seen_roots.insert(root_dir.clone());
        projects.push(Project { root_dir, manifest });
    }

    Ok(projects)
}

/// Hardcoded ignore patterns. Enumerating a real workspace excludes
/// only `node_modules` and `bower_components`, not the `**/test/**` /
/// `**/tests/**` directories that the lower-level package-finding path
/// excludes.
const IGNORE_PATTERNS: &[&str] = &["**/node_modules/**", "**/bower_components/**"];
const PROJECT_MANIFEST_BASENAMES: &[&str] = &["package.json", "package.yaml"];

fn normalize_manifest_patterns(pattern: &str) -> Vec<String> {
    // Each user pattern is suffixed with every supported manifest basename
    // so the glob matches manifest files rather than directories.
    let trimmed = pattern.trim_end_matches('/');
    if trimmed.is_empty() || trimmed == "." {
        return Vec::new();
    }
    PROJECT_MANIFEST_BASENAMES.iter().map(|basename| format!("{trimmed}/{basename}")).collect()
}

fn is_literal_pattern(pattern: &str) -> bool {
    !pattern.chars().any(|ch| matches!(ch, '*' | '?' | '[' | ']' | '{' | '}'))
}

#[cfg(test)]
mod tests;
