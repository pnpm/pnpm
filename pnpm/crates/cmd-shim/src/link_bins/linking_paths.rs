use super::{
    FsReadHead, LinkBinsError, LinkBinsOptions, PackageBinSource, bin_node_paths, remove_bin,
    shim_writer::with_extension_appended,
};
use crate::bin_resolver::Command;
use pnpm_fs::{is_subdir, realpath_missing};
use rayon::prelude::*;
use std::{
    borrow::Cow,
    ffi::OsStr,
    io,
    path::{Path, PathBuf},
};

pub(super) struct LinkingPaths<'a> {
    pub(super) bins_dir: Cow<'a, Path>,
    /// [`bins_dir`](Self::bins_dir) with its symlinks resolved, as the POSIX
    /// shim's `cd -P` resolves them. On Windows it is resolved only when a
    /// symlink or junction lies on the path, since `canonicalize` also
    /// resolves a `subst` drive, which the MSYS shell does not.
    /// [`sh_shim_path`](Self::sh_shim_path) keeps the lexical drive either way.
    physical_bins_dir: Cow<'a, Path>,
    pub(super) relocatable_root: Option<PathBuf>,
    pub(super) preserve_bin_name: bool,
    pub(super) cleanup_aliases: bool,
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
            physical_bins_dir: if cfg!(unix) || has_reparse_point_on_path(bins_dir) {
                Cow::Owned(resolve(bins_dir)?)
            } else {
                Cow::Borrowed(bins_dir)
            },
            relocatable_root: None,
            preserve_bin_name: options.preserve_bin_name,
            cleanup_aliases: false,
            project_node_path: project_modules_dir(bins_dir, options)
                .map(|dir| dir.to_string_lossy().into_owned()),
            extra_node_paths: Cow::Borrowed(&options.extra_node_paths),
        };
        let Some(root) = options.relocatable_root
            .as_deref()
            .filter(|_| cfg!(unix))
        else {
            paths.cleanup_aliases = paths.has_bin_aliases();
            return Ok(paths);
        };
        let physical_root = resolve(root)?;
        if !is_subdir(&physical_root, &paths.physical_bins_dir) {
            return Ok(paths);
        }
        paths.bins_dir.clone_from(&paths.physical_bins_dir);
        paths.project_node_path =
            paths.project_node_path.map(|entry| resolve_extra(&entry, root, &physical_root));
        paths.extra_node_paths = options.extra_node_paths
            .iter()
            .map(|entry| resolve_extra(entry, root, &physical_root))
            .collect::<Vec<_>>()
            .into();
        paths.relocatable_root = Some(physical_root);
        paths.cleanup_aliases = paths.has_bin_aliases();
        Ok(paths)
    }

    /// `shim_path`, a shim in [`bins_dir`](Self::bins_dir), as the POSIX shim
    /// for `target` computes its relative target from it, so that the `..`
    /// segments climb from the directory the shim resolves at run time. It is
    /// `shim_path` unless a symlink lies on the way. Then it is in the
    /// physical bin directory, placed under the lexical ancestor it shares
    /// with `target` when it lies under that ancestor's physical path.
    pub(super) fn sh_shim_path<'shim>(
        &self,
        target: &Path,
        shim_path: &'shim Path,
    ) -> Result<Cow<'shim, Path>, LinkBinsError> {
        let (Some(name), false) = (shim_path.file_name(), self.physical_bins_dir == self.bins_dir)
        else {
            return Ok(Cow::Borrowed(shim_path));
        };
        let Some(ancestor) = self.bins_dir
            .ancestors()
            .find(|ancestor| target.starts_with(ancestor))
        else {
            return Ok(Cow::Owned(self.physical_bins_dir.join(name)));
        };
        let physical_ancestor = resolve(ancestor)?;
        Ok(Cow::Owned(match self.physical_bins_dir.strip_prefix(&physical_ancestor) {
            Ok(below) => ancestor.join(below).join(name),
            Err(_) => self.physical_bins_dir.join(name),
        }))
    }

    fn has_bin_aliases(&self) -> bool {
        !self.preserve_bin_name
            && cfg!(unix)
            && self.bins_dir.file_name() == Some(OsStr::new(".bin"))
            && self.bins_dir
                .parent()
                .is_some_and(|parent| {
                    match std::fs::symlink_metadata(parent.join(".bin-symlinks")) {
                        Ok(_) => true,
                        Err(error) => error.kind() != io::ErrorKind::NotFound,
                    }
                })
    }

    pub(super) fn alias_path(&self, name: &str) -> Option<PathBuf> {
        if !self.preserve_bin_name
            || !cfg!(unix)
            || name == "node"
            || self.bins_dir.file_name() != Some(OsStr::new(".bin"))
        {
            return None;
        }
        let parent = self.bins_dir.parent()?;
        Some(parent.join(".bin-symlinks").join(name))
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
    (bins_dir.file_name() == Some(OsStr::new(".bin")) && modules_dir.ends_with(name)).then_some(
        modules_dir,
    )
}

/// Whether a symlink, a junction, or another reparse point lies on `dir`'s
/// path.
#[cfg(windows)]
fn has_reparse_point_on_path(dir: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    dir.ancestors()
        .any(|ancestor| {
            std::fs::symlink_metadata(ancestor)
                .is_ok_and(|metadata| {
                    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
                })
        })
}

#[cfg(not(windows))]
fn has_reparse_point_on_path(_dir: &Path) -> bool {
    false
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

/// Remove the shims of the `chosen` bins that [`awaits_target`] holds back,
/// and return the rest for linking.
///
/// Every removal finishes before the caller writes a shim: on Windows the
/// siblings of a removed bin `tool` include `tool.cmd`, which may be the shim
/// of another bin.
pub(super) fn remove_bins_awaiting_target<'packages, Sys: FsReadHead>(
    chosen: Vec<(Command, &'packages PackageBinSource)>,
    bins_dir: &Path,
    shims_dir: &Path,
) -> Result<Vec<(Command, &'packages PackageBinSource)>, LinkBinsError> {
    let (awaiting, to_link): (Vec<_>, Vec<_>) = chosen
        .into_par_iter()
        .partition(|(command, pkg)| awaits_target::<Sys>(pkg, bins_dir, &command.path));
    awaiting
        .par_iter()
        .try_for_each(|(command, _)| {
            let shim_path = shims_dir.join(&command.name);
            remove_bin(&shim_path)
                .map_err(|error| LinkBinsError::RemoveStaleBin { path: shim_path, error })
        })?;
    Ok(to_link)
}

/// Whether the bin's `target` is missing while a lifecycle script that may
/// create it can still run with `bins_dir` on `PATH`.
///
/// A package's scripts run with its own `node_modules/.bin` and the project's
/// `.bin` on `PATH`. The `node` package's preinstall runs `node` to download
/// `bin/node`, which must not resolve to a shim of `bin/node` itself
/// (pnpm/pnpm#15501). So a package's own bin is linked there only once its
/// target exists, and so is any bin of a package whose build is still pending
/// ([`PackageBinSource::build_pending`]). A shim an earlier install left is
/// removed. Other dependents get the shim right away, because the target may
/// be built after install.
fn awaits_target<Sys: FsReadHead>(pkg: &PackageBinSource, bins_dir: &Path, target: &Path) -> bool {
    (pkg.build_pending || pkg.location.join("node_modules").join(".bin") == bins_dir)
        && target_is_missing::<Sys>(&target_probe_path(pkg, target))
}

/// Whether neither `path` nor, for an extensionless `path` on Windows, its
/// `.exe` sibling exists. The shim runs an extensionless target directly, and
/// Windows then finds the `.exe`.
fn target_is_missing<Sys: FsReadHead>(path: &Path) -> bool {
    let missing = |path: &Path| {
        matches!(
            Sys::read_head(path, 0, &mut [0u8; 1]),
            Err(error) if matches!(error.kind(), io::ErrorKind::NotFound | io::ErrorKind::NotADirectory),
        )
    };
    missing(path)
        && (!cfg!(windows)
            || path.extension().is_some()
            || missing(&with_extension_appended(path, "exe")))
}
