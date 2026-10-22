//! Decide which files inside a package directory end up in a published
//! tarball. Port of [`npm-packlist`](https://github.com/npm/npm-packlist).
//!
//! Both `pacquet pack` (computing a tarball's contents) and the
//! git / directory fetchers (deciding what to import into the CAS) need
//! this, so it lives in its own crate.
//!
//! File selection uses one per-directory ignore rule stack. Manifest `files`
//! rules replace root ignore files; required paths and standard package files
//! are appended at the root. Bundled dependencies use the same selection.

pub use ignore_walk::files_field::{FilesMatcher, build_files_matcher};

use derive_more::{Display, Error};
use ignore_walk::{PackageRules, walk_files};
use pnpm_diagnostics::miette::{self, Diagnostic};
use pnpm_package_manifest::safe_read_package_json_from_dir;
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque},
    ffi::OsStr,
    fs,
    path::{Component, Path, PathBuf},
};

#[cfg(test)]
mod tests;

/// Error type of [`packlist`]. Surfaces the subset of npm-packlist
/// failures the current scope can produce.
#[derive(Debug, Display, Error, Diagnostic)]
#[non_exhaustive]
pub enum PacklistError {
    #[display("I/O error while computing packlist for {pkg_dir}: {source}")]
    #[diagnostic(code(ERR_PNPM_FS_PACKLIST_IO))]
    Io {
        pkg_dir: String,
        #[error(source)]
        source: std::io::Error,
    },
}

/// Version-control directory names that exclude every file under
/// them at any depth. Drops VCS state from a published package
/// regardless of where in the tree it happens to sit, the same as
/// npm-packlist. Exact-segment match: a path with a literal segment
/// named `.git` / `.svn` / `.hg` / `CVS` is filtered, but a regular
/// file like `lib/foo.hg-stub` (basename `foo.hg-stub`, not `.hg`) is
/// not.
const ALWAYS_EXCLUDED_DIR_SEGMENTS: &[&str] = &[".git", ".svn", ".hg", "CVS"];

/// Walk `pkg_dir` and return forward-slash relative paths for every
/// file the published tarball should contain. Paths are relative to
/// `pkg_dir`, with no leading `./`.
pub fn packlist(pkg_dir: &Path, manifest: &Value) -> Result<Vec<String>, PacklistError> {
    packlist_with_options(pkg_dir, manifest, PacklistOptions::default())
}

#[derive(Debug, Default, Clone, Copy)]
pub struct PacklistOptions<'a> {
    pub workspace_dir: Option<&'a Path>,
    /// Project directory whose `node_modules` holds the bundled dependencies
    /// when `pkg_dir` is a subdirectory of it, such as `publishConfig.directory`.
    pub bundled_dependencies_dir: Option<&'a Path>,
}

/// Variant of [`packlist`] that lets callers pass workspace context.
/// Workspace packages without a package-level `.npmignore` and without a
/// manifest `files` allowlist honor ancestor `.npmignore` / `.gitignore`
/// files between the workspace root and the package, matching npm-packlist's
/// `prefix` / `workspaces` behavior. Callers without workspace context keep
/// the safer package-only walk.
///
/// Returns only the files that live at their packed path under `pkg_dir`.
/// A bundled dependency resolved through an isolated `node_modules` layout is
/// packed at a different path than it is read from; [`packlist_with_sources`]
/// returns those too.
pub fn packlist_with_options(
    pkg_dir: &Path,
    manifest: &Value,
    options: PacklistOptions<'_>,
) -> Result<Vec<String>, PacklistError> {
    Ok(packlist_with_sources(pkg_dir, manifest, options)?
        .into_iter()
        .filter(|(file, source)| *source == pkg_dir.join(file))
        .map(|(file, _)| file)
        .collect())
}

/// Map each packed path to the file it is read from.
///
/// Bundled dependencies resolve from `pkg_dir` upward, and never above the
/// workspace root when `pkg_dir` is a workspace package, or above
/// [`PacklistOptions::bundled_dependencies_dir`] (default `pkg_dir`) otherwise.
pub fn packlist_with_sources(
    pkg_dir: &Path,
    manifest: &Value,
    options: PacklistOptions<'_>,
) -> Result<BTreeMap<String, PathBuf>, PacklistError> {
    let workspace_dir =
        options.workspace_dir.filter(|workspace_dir| pkg_dir.starts_with(workspace_dir));
    let own_files = collect_own_files(
        pkg_dir,
        PackageRules { workspace_dir, manifest, include_alternate_manifests: true },
    )?;
    let mut out = own_files
        .into_iter()
        .map(|file| {
            let source = pkg_dir.join(&file);
            (file, source)
        })
        .collect();
    let boundary = workspace_dir.or(options.bundled_dependencies_dir).unwrap_or(pkg_dir);
    collect_bundled_files(pkg_dir, manifest, boundary, &mut out)?;
    Ok(out)
}

/// Select one package's own files. [`collect_bundled_files`] adds dependencies.
fn collect_own_files(
    pkg_dir: &Path,
    options: PackageRules<'_>,
) -> Result<BTreeSet<String>, PacklistError> {
    let mut out = BTreeSet::new();
    walk_files(pkg_dir, options, &mut |path, file_type| {
        if is_packable(pkg_dir, path, file_type) {
            out.insert(relative_forward_slash(pkg_dir, path));
        }
    })
    .map_err(|source| io_error(pkg_dir, source))?;
    Ok(out)
}

/// The [`normalize_contained_field_path`]-ed `main`, `browser`, and `bin`
/// paths that stay inside the package. A non-string value, such as a
/// `browser` replacement map, is skipped.
fn required_field_paths(manifest: &Value) -> Vec<String> {
    let bin_paths = match manifest.get("bin") {
        Some(Value::String(bin)) => vec![bin.as_str()],
        Some(Value::Object(map)) => map
            .values()
            .filter_map(Value::as_str)
            .collect(),
        _ => Vec::new(),
    };
    ["main", "browser"]
        .into_iter()
        .filter_map(|field| manifest.get(field).and_then(Value::as_str))
        .chain(bin_paths)
        .filter_map(normalize_contained_field_path)
        .collect()
}

/// The [`normalize_contained_field_path`]-ed `pnpm.config` path when it stays
/// inside the package.
fn pnpm_config_path(manifest: &Value) -> Option<String> {
    manifest
        .get("pnpm")
        .and_then(Value::as_object)
        .and_then(|pnpm| pnpm.get("config"))
        .and_then(Value::as_str)
        .and_then(normalize_contained_field_path)
}

fn normalize_contained_field_path(raw: &str) -> Option<String> {
    let normalized = normalize_field_path(raw);
    if !is_contained_field_path(&normalized) {
        return None;
    }
    Some(
        Path::new(&normalized)
            .components()
            .filter_map(|component| match component {
                Component::Normal(segment) => segment.to_str(),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("/"),
    )
}

fn relative_forward_slash(root: &Path, full: &Path) -> String {
    let rel = full.strip_prefix(root).unwrap_or(full);
    let mut buf = PathBuf::from(rel)
        .into_os_string()
        .to_string_lossy()
        .into_owned();
    if std::path::MAIN_SEPARATOR != '/' {
        buf = buf.replace(std::path::MAIN_SEPARATOR, "/");
    }
    buf
}

/// Whether a normalized manifest path stays lexically inside the package:
/// non-empty, normal path components only, and no Windows separators.
fn is_contained_field_path(normalized: &str) -> bool {
    !normalized.is_empty()
        && !normalized.contains('\\')
        && Path::new(normalized)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn io_error(pkg_dir: &Path, source: std::io::Error) -> PacklistError {
    PacklistError::Io { pkg_dir: pkg_dir.display().to_string(), source }
}

mod bundled;
mod ignore_walk;
mod symlinks;
use bundled::collect_bundled_files;
use ignore_walk::files_field::normalize_field_path;
use symlinks::is_packable;
