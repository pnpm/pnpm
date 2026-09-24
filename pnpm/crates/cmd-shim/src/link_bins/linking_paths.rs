use super::{FsReadHead, LinkBinsError, LinkBinsOptions, PackageBinSource, bin_node_paths};
use pnpm_fs::{is_subdir, realpath_missing};
use std::{
    borrow::Cow,
    ffi::OsStr,
    io,
    path::{Path, PathBuf},
};

pub(super) struct LinkingPaths<'a> {
    pub(super) bins_dir: Cow<'a, Path>,
    pub(super) relocatable_root: Option<PathBuf>,
    pub(super) project_node_path: Option<String>,
    pub(super) extra_node_paths: Cow<'a, [String]>,
}

impl<'a> LinkingPaths<'a> {
    pub(super) fn new(
        bins_dir: &'a Path,
        options: &'a LinkBinsOptions,
    ) -> Result<Self, LinkBinsError> {
        let mut paths = Self {
            bins_dir: Cow::Borrowed(bins_dir),
            relocatable_root: None,
            project_node_path: project_modules_dir(bins_dir, options)
                .map(|dir| dir.to_string_lossy().into_owned()),
            extra_node_paths: Cow::Borrowed(&options.extra_node_paths),
        };
        let Some(root) = options.relocatable_root
            .as_deref()
            .filter(|_| cfg!(unix))
        else {
            return Ok(paths);
        };
        let physical_root = resolve(root)?;
        let physical_bins = resolve(bins_dir)?;
        if !is_subdir(&physical_root, &physical_bins) {
            return Ok(paths);
        }
        paths.bins_dir = Cow::Owned(physical_bins);
        paths.project_node_path =
            paths.project_node_path.map(|entry| resolve_extra(&entry, root, &physical_root));
        paths.extra_node_paths = options.extra_node_paths
            .iter()
            .map(|entry| resolve_extra(entry, root, &physical_root))
            .collect::<Vec<_>>()
            .into();
        paths.relocatable_root = Some(physical_root);
        Ok(paths)
    }

    pub(super) fn target<'target>(
        &self,
        target: &'target Path,
        original_root: Option<&Path>,
    ) -> Result<Cow<'target, Path>, LinkBinsError> {
        let Some(root) = self.relocatable_root.as_deref() else {
            return Ok(Cow::Borrowed(target));
        };
        let Some((parent, name)) = target.parent().zip(target.file_name()) else {
            return Ok(Cow::Borrowed(target));
        };
        let lexical_inside =
            is_subdir(root, target) || original_root.is_some_and(|root| is_subdir(root, target));
        let physical_parent = match resolve(parent) {
            Ok(parent) => parent,
            Err(error) if lexical_inside => return Err(error),
            Err(_) => return Ok(Cow::Borrowed(target)),
        };
        if !lexical_inside && !is_subdir(root, &physical_parent) {
            return Ok(Cow::Borrowed(target));
        }
        // Keep the final dirent: a runtime binary can itself be a symlink
        // whose path inside the project must remain relocatable.
        Ok(Cow::Owned(physical_parent.join(name)))
    }
}

/// The `NODE_PATH` entries for one package's shims: the project modules
/// dir when there is one, then the target's own `node_modules` dirs
/// (pnpm's `getBinNodePaths`), then the caller's extras. An entry that
/// appears again keeps its first position. With no project dir and no
/// extras the shims get no `NODE_PATH` at all (`extendNodePath: false`, a
/// non-isolated linker, or no hoist pattern), matching pnpm's bins
/// linker.
///
/// The result depends only on the package's symlink-resolved
/// directory — every bin lives under the package root — so a
/// caller-supplied [`PackageBinSource::resolved_location`] makes this
/// syscall-free; without one the package's `location` is
/// canonicalized once, covering all of its bins.
pub(super) fn shim_node_path(
    pkg: &PackageBinSource,
    project_node_path: Option<&str>,
    extra_node_paths: &[String],
) -> Vec<String> {
    let delimiter = if cfg!(windows) { ';' } else { ':' };
    let project_node_path = project_node_path.filter(|entry| !entry.contains(delimiter));
    if project_node_path.is_none() && extra_node_paths.is_empty() {
        return Vec::new();
    }
    let own = if let Some(resolved) = &pkg.resolved_location {
        bin_node_paths(resolved)
    } else {
        let dir =
            dunce::canonicalize(&pkg.location).unwrap_or_else(|_| pkg.location.clone());
        bin_node_paths(&dir)
    };
    let mut merged: Vec<String> = project_node_path
        .map(str::to_string)
        .into_iter()
        .collect();
    for entry in own
        .into_iter()
        .chain(extra_node_paths.iter().cloned())
    {
        if !merged.contains(&entry) {
            merged.push(entry);
        }
    }
    merged
}

fn project_modules_dir<'a>(bins_dir: &'a Path, options: &LinkBinsOptions) -> Option<&'a Path> {
    let name = options.project_modules_dir_name.as_deref()?;
    let modules_dir = bins_dir.parent()?;
    (bins_dir.file_name() == Some(OsStr::new(".bin")) && modules_dir.file_name() == Some(name))
        .then_some(modules_dir)
}

fn resolve(path: &Path) -> Result<PathBuf, LinkBinsError> {
    realpath_missing(path)
        .map_err(|error| LinkBinsError::ResolvePath { path: path.to_path_buf(), error })
}

fn resolve_extra(entry: &str, root: &Path, physical_root: &Path) -> String {
    if !Path::new(entry).is_absolute() {
        return entry.to_string();
    }
    // Extra module paths are optional and may not resolve until a later install step.
    let Ok(physical) = realpath_missing(Path::new(entry)) else {
        return entry.to_string();
    };
    if is_subdir(physical_root, &physical)
        || is_subdir(root, Path::new(entry))
        || is_subdir(physical_root, Path::new(entry))
    {
        physical.to_string_lossy().into_owned()
    } else {
        entry.to_string()
    }
}

/// The target's symlink-resolved path, which doubles as the memo key for
/// the per-target probes: importers that reach one virtual-store file
/// through different symlinks share it. Without a resolved location, the
/// literal path still dedupes within whatever scope the caller gave the
/// cache.
pub(super) fn target_probe_path(pkg: &PackageBinSource, target: &Path) -> PathBuf {
    pkg.resolved_location
        .as_ref()
        .and_then(|resolved| {
            target
                .strip_prefix(&pkg.location)
                .ok()
                .map(|bin_rel_path| resolved.join(bin_rel_path))
        })
        .unwrap_or_else(|| target.to_path_buf())
}

/// Whether `bins_dir` is the `node_modules/.bin` of the package at `location`.
///
/// A package's own bins are on `PATH` while its lifecycle scripts run, and
/// those scripts may be what creates a missing target: the `node` package's
/// preinstall runs `node` to download `bin/node`, which must not resolve to a
/// shim of `bin/node` itself. So a package's own bin is linked there only once
/// its target exists, while dependents get the shim right away (the target
/// may be built after install).
pub(super) fn is_own_bins_dir(location: &Path, bins_dir: &Path) -> bool {
    location.join("node_modules").join(".bin") == bins_dir
}

pub(super) fn target_is_missing<Sys: FsReadHead>(path: &Path) -> bool {
    matches!(
        Sys::read_head(path, 0, &mut [0u8; 1]),
        Err(error) if error.kind() == io::ErrorKind::NotFound,
    )
}
