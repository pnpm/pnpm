//! The `extends` field of `pnpm-workspace.yaml`: the other workspace
//! manifests whose catalogs a manifest inherits.

use super::{
    InvalidWorkspaceManifestError, ReadWorkspaceManifestError, WORKSPACE_MANIFEST_FILENAME,
    WorkspaceManifest, read_declared_workspace_manifest,
};
use pnpm_catalogs_types::{Catalogs, DEFAULT_CATALOG_NAME};
use pnpm_local_spec::LocalSpec;
use std::path::{Path, PathBuf};
use wax::{
    Glob,
    walk::{Entry, FileIterator},
};

/// The catalogs `manifest`, which lives in `dir`, inherits: those of every
/// manifest its `extends` names, each together with the catalogs it inherits
/// in turn. A manifest named later wins over one named earlier, and glob
/// matches count in path order. Empty when `manifest` extends nothing.
pub(super) fn inherited_catalogs(
    dir: &Path,
    manifest: &WorkspaceManifest,
) -> Result<Catalogs, ReadWorkspaceManifestError> {
    Resolver::default().inherited(dir, manifest)
}

/// Walks `extends` references, remembering the manifests on the current path
/// so that one extending itself, directly or not, is reported rather than
/// followed forever. The manifests are told apart by their real directory,
/// so that a symlink back to one of them is a cycle too.
#[derive(Default)]
struct Resolver {
    ancestors: Vec<PathBuf>,
}

impl Resolver {
    fn inherited(
        &mut self,
        dir: &Path,
        manifest: &WorkspaceManifest,
    ) -> Result<Catalogs, ReadWorkspaceManifestError> {
        let Some(extends) = &manifest.extends else { return Ok(Catalogs::new()) };
        let dir = pnpm_fs::lexical_normalize(
            &std::path::absolute(dir).unwrap_or_else(|_| dir.to_path_buf()),
        );
        let real_dir = std::fs::canonicalize(&dir).unwrap_or_else(|_| dir.clone());
        if self.ancestors.contains(&real_dir) {
            return Err(ReadWorkspaceManifestError::ExtendsCycle { dir });
        }
        self.ancestors.push(real_dir);
        let mut inherited = Catalogs::new();
        for entry in extends.entries() {
            for target in targets(&dir, entry)? {
                if let Some(catalogs) = self.target_catalogs(&dir, &target)? {
                    merge(&mut inherited, catalogs);
                }
            }
        }
        self.ancestors.pop();
        Ok(inherited)
    }

    /// The catalogs of the manifest `target` points at, the inherited ones
    /// included, with local paths moved to be relative to `referenced_by`.
    /// `None` for a glob match without a manifest.
    fn target_catalogs(
        &mut self,
        referenced_by: &Path,
        target: &Target,
    ) -> Result<Option<Catalogs>, ReadWorkspaceManifestError> {
        let Some(manifest) = read_declared_workspace_manifest(&target.dir)? else {
            if target.from_glob {
                return Ok(None);
            }
            return Err(ReadWorkspaceManifestError::ExtendsNotFound {
                dir: target.dir.clone(),
                referenced_by: referenced_by.to_path_buf(),
            });
        };
        if manifest.catalog.is_some()
            && manifest.catalogs
                .as_ref()
                .is_some_and(|catalogs| catalogs.contains_key(DEFAULT_CATALOG_NAME))
        {
            return Err(ReadWorkspaceManifestError::ExtendedDefaultCatalogDefinedTwice {
                path: target.dir.join(WORKSPACE_MANIFEST_FILENAME),
            });
        }
        let mut catalogs = self.inherited(&target.dir, &manifest)?;
        merge(&mut catalogs, manifest.declared_catalogs());
        reanchor(&mut catalogs, &target.dir, referenced_by);
        Ok(Some(catalogs))
    }
}

/// A directory an `extends` reference points at.
struct Target {
    dir: PathBuf,
    /// A glob matching a directory without a manifest skips it, where a
    /// plain reference to one is an error.
    from_glob: bool,
}

fn targets(dir: &Path, entry: &str) -> Result<Vec<Target>, ReadWorkspaceManifestError> {
    if is_glob(entry) {
        return glob_targets(dir, entry);
    }
    let path = pnpm_fs::lexical_normalize(&dir.join(entry));
    let manifest_dir = if path
        .file_name()
        .is_some_and(|name| name == WORKSPACE_MANIFEST_FILENAME)
    {
        path.parent().map_or_else(|| path.clone(), Path::to_path_buf)
    } else {
        path
    };
    Ok(vec![Target { dir: manifest_dir, from_glob: false }])
}

/// The directories holding a `pnpm-workspace.yaml` that `pattern` matches,
/// in path order. The static head of the pattern, `..` segments and absolute
/// paths included, becomes the directory the glob walks, so the glob itself
/// never climbs out of it.
fn glob_targets(dir: &Path, pattern: &str) -> Result<Vec<Target>, ReadWorkspaceManifestError> {
    let pattern = manifest_pattern(pattern);
    let (base, tail) = split_static_head(dir, &pattern);
    let glob = Glob::new(&tail).map_err(|error| invalid_pattern(&pattern, &error))?;
    if !base.is_dir() {
        return Ok(Vec::new());
    }
    let mut dirs = manifest_dirs(&glob, &base, &pattern)?;
    dirs.sort();
    dirs.dedup();
    Ok(dirs
        .into_iter()
        .map(|dir| Target { dir, from_glob: true })
        .collect())
}

/// `pattern` split into the directory its static head names, resolved
/// against `dir`, and the glob that follows it.
fn split_static_head(dir: &Path, pattern: &str) -> (PathBuf, String) {
    let segments: Vec<&str> = pattern.split('/').collect();
    let first_dynamic = segments
        .iter()
        .position(|segment| is_glob(segment))
        .unwrap_or(segments.len() - 1);
    let base = pnpm_fs::lexical_normalize(&dir.join(segments[..first_dynamic].join("/")));
    (base, segments[first_dynamic..].join("/"))
}

/// The parent directory of every file under `base` that `glob` matches,
/// `node_modules` left out.
fn manifest_dirs(
    glob: &Glob<'_>,
    base: &Path,
    pattern: &str,
) -> Result<Vec<PathBuf>, ReadWorkspaceManifestError> {
    let walk = glob
        .walk(base)
        .not("**/node_modules/**")
        .map_err(|error| invalid_pattern(pattern, &error))?;
    let mut dirs = Vec::new();
    for entry in walk {
        let entry = entry.map_err(|error| ReadWorkspaceManifestError::WalkExtendsPattern {
            pattern: pattern.to_string(),
            message: error.to_string(),
        })?;
        dirs.extend(
            entry
                .path()
                .parent()
                .map(Path::to_path_buf),
        );
    }
    Ok(dirs)
}

fn invalid_pattern(pattern: &str, error: &impl ToString) -> ReadWorkspaceManifestError {
    ReadWorkspaceManifestError::Invalid(InvalidWorkspaceManifestError::InvalidExtendsPattern {
        pattern: pattern.to_string(),
        message: error.to_string(),
    })
}

/// `pattern` pointed at `pnpm-workspace.yaml` files: `packages/*` matches
/// the manifest inside each matched directory.
fn manifest_pattern(pattern: &str) -> String {
    let pattern = pattern.trim_end_matches('/');
    if pattern.rsplit('/').next() == Some(WORKSPACE_MANIFEST_FILENAME) {
        pattern.to_string()
    } else {
        format!("{pattern}/{WORKSPACE_MANIFEST_FILENAME}")
    }
}

fn is_glob(value: &str) -> bool {
    value.contains(['*', '?', '{', '}', '[', ']'])
}

/// Move the entries of `catalogs` that name a local path, written relative
/// to `from`, to be relative to `to`.
fn reanchor(catalogs: &mut Catalogs, from: &Path, to: &Path) {
    let specifiers = catalogs.values_mut().flat_map(|catalog| catalog.values_mut());
    for specifier in specifiers {
        if let Some(spec) = LocalSpec::parse_filesystem(specifier, from) {
            *specifier = spec.render(Some(to));
        }
    }
}

/// Add `overlay` to `catalogs` entry by entry, `overlay` winning.
fn merge(catalogs: &mut Catalogs, overlay: Catalogs) {
    for (name, catalog) in overlay {
        catalogs
            .entry(name)
            .or_default()
            .extend(catalog);
    }
}

#[cfg(test)]
mod tests;
