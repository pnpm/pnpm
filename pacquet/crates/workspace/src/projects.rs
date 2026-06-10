//! Glob-expand `packages:` from `pnpm-workspace.yaml` into the
//! workspace's [`Project`] list.
//!
//! Port of upstream's
//! [`findWorkspaceProjects`](https://github.com/pnpm/pnpm/blob/94240bc046/workspace/projects-reader/src/index.ts)
//! and
//! [`findPackages`](https://github.com/pnpm/pnpm/blob/94240bc046/workspace/projects-reader/src/findPackages.ts).
//!
//! Behavior preserved against upstream:
//!
//! - Glob-walk the workspace root using the user's `packages:` patterns
//!   (defaulting to `['.', '**']` when omitted, matching tinyglobby's
//!   call site upstream).
//! - Always include the workspace root itself, per
//!   <https://github.com/pnpm/pnpm/issues/1986>.
//! - Filter `**/node_modules/**` and `**/bower_components/**` so a
//!   pre-existing install doesn't surface synthetic projects.
//! - Dedupe matches (a path can satisfy multiple patterns) and sort
//!   lexicographically by `rootDir`, matching upstream's
//!   `lexCompare(path.dirname(path1), path.dirname(path2))`.
//!
//! Out of scope (tracked as upstream parity follow-ups):
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
/// Pacquet keeps this shape narrower than upstream's `Project` (which
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
///
/// Field names mirror upstream's `FindWorkspaceProjectsOpts` so a port
/// of any individual install entry point doesn't have to translate
/// option names.
#[derive(Debug, Default, Clone)]
pub struct FindWorkspaceProjectsOpts {
    /// `packages:` from `pnpm-workspace.yaml`. When `None`, upstream
    /// falls back to `['.', '**']`. Pacquet mirrors that default so a
    /// workspace whose manifest only carries settings still enumerates
    /// projects.
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
/// Mirrors upstream's
/// [`findWorkspaceProjects`](https://github.com/pnpm/pnpm/blob/94240bc046/workspace/projects-reader/src/index.ts)
/// except for the per-project `packageIsInstallable` /
/// `checkNonRootProjectManifest` validations, which are explicitly
/// deferred by [#431]. When validation lands, this entry point grows
/// the filter; today it's a thin wrapper over
/// [`find_workspace_projects_no_check`].
///
/// [#431]: https://github.com/pnpm/pacquet/issues/431
pub fn find_workspace_projects(
    workspace_root: &Path,
    opts: &FindWorkspaceProjectsOpts,
) -> Result<Vec<Project>, FindWorkspaceProjectsError> {
    find_workspace_projects_no_check(workspace_root, opts)
}

/// Skip-validation variant, matching upstream's
/// [`findWorkspaceProjectsNoCheck`](https://github.com/pnpm/pnpm/blob/94240bc046/workspace/projects-reader/src/index.ts).
pub fn find_workspace_projects_no_check(
    workspace_root: &Path,
    opts: &FindWorkspaceProjectsOpts,
) -> Result<Vec<Project>, FindWorkspaceProjectsError> {
    // Upstream default mirrors tinyglobby's call site: when no patterns
    // were configured (`opts.patterns ?? defaults`), search the
    // workspace root non-recursively *and* recursively. The two-pattern
    // fallback fires only on `None`, not on `Some(vec![])` — an explicit
    // empty array means "enumerate only the workspace root" (which is
    // unconditionally added below per upstream's
    // <https://github.com/pnpm/pnpm/issues/1986> rule).
    let default_patterns = [".".to_string(), "**".to_string()];
    let patterns: &[String] = match opts.patterns.as_deref() {
        Some(p) => p,
        None => &default_patterns,
    };

    // Upstream (tinyglobby) treats `!`-prefixed patterns as negations.
    // wax does not accept `!` inside `Glob::new()`, so split them out
    // and feed them through `.not()` instead. `!/...` remains a no-op:
    // relative workspace paths never match that absolute form.
    let mut include_patterns: Vec<&str> = Vec::new();
    let mut user_negation_globs: Vec<String> = Vec::new();
    for pattern in patterns {
        if let Some(body) = pattern.strip_prefix('!') {
            if body.starts_with('/') {
                continue;
            }
            let normalized = normalize_pattern(body);
            Glob::new(&normalized).map_err(|err| FindWorkspaceProjectsError::InvalidGlob {
                pattern: pattern.clone(),
                message: err.to_string(),
            })?;
            user_negation_globs.push(normalized);
        } else {
            include_patterns.push(pattern);
        }
    }

    // wax's `not` takes a single pattern; combine the ignores with
    // `wax::any` so the walk filters them all in one pass, matching
    // upstream's `ignore: ['**/node_modules/**', '**/bower_components/**']`.
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
        let normalized = normalize_pattern(pattern);
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

    // Upstream's `findPackages` always includes the workspace root,
    // even when no `packages:` pattern matches it
    // (https://github.com/pnpm/pnpm/issues/1986). Mirror that by
    // unconditionally adding the root's manifest if present.
    let root_manifest = workspace_root.join("package.json");
    if root_manifest.is_file() {
        manifest_paths.insert(root_manifest);
    }

    // Sort lexicographically by `rootDir` (= parent of the manifest),
    // matching upstream's `lexCompare(path.dirname(p1), path.dirname(p2))`.
    // `BTreeSet` already sorts by full path, but the upstream contract
    // is "by dir then by basename"; with our basename always being
    // `package.json` the two orderings coincide. Keep the explicit
    // sort below to make the contract visible.
    let mut sorted: Vec<PathBuf> = manifest_paths.into_iter().collect();
    sorted.sort_by(|left, right| {
        let dir_left = left.parent().unwrap_or_else(|| Path::new(""));
        let dir_right = right.parent().unwrap_or_else(|| Path::new(""));
        dir_left.cmp(dir_right)
    });

    let mut projects = Vec::with_capacity(sorted.len());
    for manifest_path in sorted {
        let manifest = match read_exact_project_manifest(&manifest_path) {
            Ok(m) => m,
            // Upstream swallows ENOENT mid-walk (a file vanished
            // between listing and reading) at
            // <https://github.com/pnpm/pnpm/blob/94240bc046/workspace/projects-reader/src/findPackages.ts>.
            // Mirror that exact-and-only carve-out: parse errors,
            // permission failures, and "is a directory" must still
            // propagate so a malformed `package.json` surfaces as a
            // diagnostic instead of being silently dropped from the
            // workspace.
            Err(ReadProjectManifestError::Read(PackageManifestError::Io(err)))
                if err.kind() == ErrorKind::NotFound =>
            {
                continue;
            }
            Err(ReadProjectManifestError::Read(PackageManifestError::NoImporterManifestFound(
                _,
            ))) => continue,
            Err(err) => return Err(FindWorkspaceProjectsError::ReadManifest(err)),
        };
        let root_dir = manifest_path.parent().unwrap_or(workspace_root).to_path_buf();
        projects.push(Project { root_dir, manifest });
    }

    Ok(projects)
}

/// Hardcoded ignore patterns, matching upstream's `DEFAULT_IGNORE`
/// minus the `**/test/**` / `**/tests/**` exclusions (which are only
/// relevant to `findPackages` callers that aren't enumerating a real
/// workspace — `findWorkspaceProjects` overrides them with just the
/// `node_modules` / `bower_components` pair).
const IGNORE_PATTERNS: &[&str] = &["**/node_modules/**", "**/bower_components/**"];

fn normalize_pattern(pattern: &str) -> String {
    // Mirrors upstream `normalizePatterns`: each user pattern is
    // suffixed with the manifest basename so the glob matches manifest
    // files rather than directories. Pacquet only supports
    // `package.json` today; upstream's `{json,yaml,json5}` brace
    // expansion is dropped to match the [`project_manifest`] reader.
    let trimmed = pattern.trim_end_matches('/');
    if trimmed.is_empty() {
        // `.` and `''` both mean "match the workspace root itself".
        "package.json".to_string()
    } else {
        format!("{trimmed}/package.json")
    }
}

#[cfg(test)]
mod tests;
