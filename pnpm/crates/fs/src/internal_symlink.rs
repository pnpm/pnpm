//! A package's symlink to something inside the package, as a copy of the
//! package recreates it.

use std::{
    collections::HashSet,
    fs, io,
    path::{Component, Path, PathBuf},
};

/// The source symlinks an import or sync recreates. Either way, a link whose
/// target leads out of the package is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymlinkTargets {
    /// Only links whose target is in the selected path set. A file link to a
    /// target left out is imported as a file, and a directory link is left out.
    Imported,
    /// Also a directory link whose target is left out. A file link to a target
    /// left out is still imported as a file.
    Internal,
}

/// What an admitted internal symlink's target resolves to on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SymlinkTargetKind {
    File(PathBuf),
    Directory(PathBuf),
}

/// A symlink inside a package whose target stays within the package root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmittedSymlink {
    /// Target path to write when recreating the link, with an absolute target
    /// made relative to the link's parent directory.
    pub link_target: PathBuf,
    /// Forward-slash path relative to the package root that `link_target`
    /// resolves to (`""` when it points to the package root itself).
    pub target_in_package: String,
    /// Resolved target kind on disk, or `None` when the target is missing.
    pub target_kind: Option<SymlinkTargetKind>,
}

impl AdmittedSymlink {
    #[must_use]
    pub fn is_dir(&self) -> bool {
        matches!(self.target_kind, Some(SymlinkTargetKind::Directory(_)))
    }
}

/// Where a placement pass reads and writes a package's symlinks.
#[derive(Debug, Clone, Copy)]
pub struct SymlinkPlacementContext<'a> {
    pub dest_root: &'a Path,
    pub selected_paths: &'a HashSet<&'a str>,
    pub policy: SymlinkTargets,
}

/// How [`resolve_symlink_placement`] handles an internal symlink under a
/// [`SymlinkTargets`] policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SymlinkPlacement {
    /// Recreate the symlink with `link_target`.
    Preserve { link_target: PathBuf, target_in_package: String, is_dir: bool },
    /// Import the target as a regular file because the target path was not
    /// selected.
    ImportFile,
    /// Omit the entry (a directory link to an unselected path under
    /// [`SymlinkTargets::Imported`], or a broken link to an unselected path).
    Omit,
}

/// Whether `path` is a symlink or, on Windows, a directory junction.
#[must_use]
pub fn is_symlink(path: &Path) -> bool {
    crate::is_symlink_or_junction(path).unwrap_or(false)
}

/// Whether `metadata` from [`fs::symlink_metadata`] describes a symlink or,
/// on Windows, a directory junction.
#[must_use]
pub fn is_symlink_metadata(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt as _;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        metadata.file_type().is_symlink()
            || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

/// Every path in `paths` and each directory above it, the package root
/// included as `""`.
#[must_use]
pub fn selected_paths_and_ancestors<'a>(
    paths: impl IntoIterator<Item = &'a str>,
) -> HashSet<&'a str> {
    let mut selected = HashSet::from([""]);
    for mut path in paths {
        while !path.is_empty() && selected.insert(path) {
            path = path.rsplit_once('/').map_or("", |(parent, _)| parent);
        }
    }
    selected
}

/// `link`, a link target read from `link_path`, with an absolute one made
/// relative to the link's directory. Both sides are canonicalized first:
/// a directory walk lists links under the canonical package root, while the
/// link text may reach the same place through another path. The leaf of
/// `link` is not dereferenced so an absolute link pointing at another
/// internal symlink preserves the link chain.
#[must_use]
pub fn relative_link_target(link_path: &Path, link: PathBuf) -> PathBuf {
    let (true, Some(parent)) = (link.is_absolute(), link_path.parent()) else {
        return link;
    };
    let parent = fs::canonicalize(parent)
        .or_else(|_| crate::realpath_missing(parent))
        .unwrap_or_else(|_| parent.to_path_buf());
    let link = canonicalize_preserving_leaf(&link);
    pathdiff::diff_paths(&link, &parent).unwrap_or(link)
}

fn canonicalize_preserving_leaf(path: &Path) -> PathBuf {
    if let (Some(parent), Some(name)) = (path.parent(), path.file_name())
        && let Ok(canon_parent) =
            fs::canonicalize(parent).or_else(|_| crate::realpath_missing(parent))
    {
        return crate::lexical_normalize(&canon_parent.join(name));
    }
    fs::canonicalize(path)
        .or_else(|_| crate::realpath_missing(path))
        .unwrap_or_else(|_| path.to_path_buf())
}

/// Admit `symlink` when its target stays inside `pkg_dir`, returning
/// `Ok(Err(link_target))` when the target escapes `pkg_dir`.
pub fn admit_internal_symlink(
    pkg_dir: &Path,
    symlink: &Path,
) -> io::Result<Result<AdmittedSymlink, PathBuf>> {
    let (link_target, target_in_package) =
        match resolve_internal_link_target(symlink, pkg_dir, symlink)? {
            Ok(resolved) => resolved,
            Err(escaped) => return Ok(Err(escaped)),
        };
    let target_kind = match fs::canonicalize(symlink) {
        Ok(canon_target) => {
            if let Ok(canon_root) = fs::canonicalize(pkg_dir)
                && !canon_target.starts_with(&canon_root)
            {
                return Ok(Err(link_target));
            }
            match fs::metadata(&canon_target) {
                Ok(meta) if meta.is_dir() => Some(SymlinkTargetKind::Directory(canon_target)),
                Ok(meta) if meta.is_file() => Some(SymlinkTargetKind::File(canon_target)),
                _ => None,
            }
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => None,
        Err(_) => return Ok(Err(link_target)),
    };
    Ok(Ok(AdmittedSymlink { link_target, target_in_package, target_kind }))
}

/// Classify how `source_link` (placed at `dest_link` under `ctx.dest_root`)
/// is materialized under `ctx.policy`. Returns `Ok(Err(link_target))` when the
/// link target escapes `ctx.dest_root`.
pub fn resolve_symlink_placement(
    source_link: &Path,
    dest_link: &Path,
    ctx: SymlinkPlacementContext<'_>,
) -> io::Result<Result<SymlinkPlacement, PathBuf>> {
    let (link_target, target_in_package) =
        match resolve_internal_link_target(source_link, ctx.dest_root, dest_link)? {
            Ok(resolved) => resolved,
            Err(escaped) => return Ok(Err(escaped)),
        };
    let target_meta = match fs::metadata(source_link) {
        Ok(meta) => Some(meta),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    let is_dir = target_meta.as_ref().is_some_and(fs::Metadata::is_dir);
    let is_selected = ctx.selected_paths.contains(target_in_package.as_str())
        || (ctx.policy == SymlinkTargets::Internal && is_dir);
    if is_selected {
        return Ok(Ok(SymlinkPlacement::Preserve { link_target, target_in_package, is_dir }));
    }
    if target_meta.as_ref().is_some_and(fs::Metadata::is_file) {
        return Ok(Ok(SymlinkPlacement::ImportFile));
    }
    Ok(Ok(SymlinkPlacement::Omit))
}

fn resolve_internal_link_target(
    source_link: &Path,
    pkg_root: &Path,
    link_in_pkg: &Path,
) -> io::Result<Result<(PathBuf, String), PathBuf>> {
    let raw_target = crate::read_symlink_dir(source_link)?;
    let link_target = relative_link_target(source_link, raw_target);
    let Some(mut segments) = relative_parent_segments(pkg_root, link_in_pkg) else {
        return Ok(Err(link_target));
    };
    for component in link_target.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(seg) => segments.push(seg.to_string_lossy().into_owned()),
            Component::ParentDir => {
                if segments.pop().is_none() {
                    return Ok(Err(link_target));
                }
            }
            Component::RootDir | Component::Prefix(_) => return Ok(Err(link_target)),
        }
    }
    Ok(Ok((link_target, segments.join("/"))))
}

fn relative_parent_segments(pkg_root: &Path, link_in_pkg: &Path) -> Option<Vec<String>> {
    let parent = link_in_pkg.parent()?;
    let rel_parent = crate::lexical_normalize(parent)
        .strip_prefix(crate::lexical_normalize(pkg_root))
        .ok()
        .map(Path::to_path_buf)
        .or_else(|| {
            let canon_root = fs::canonicalize(pkg_root).ok()?;
            let canon_parent = fs::canonicalize(parent).ok()?;
            canon_parent
                .strip_prefix(&canon_root)
                .ok()
                .map(Path::to_path_buf)
        })?;
    let mut segments = Vec::new();
    for component in rel_parent.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(seg) => segments.push(seg.to_string_lossy().into_owned()),
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(segments)
}

/// Recreate the internal symlink at `source_path` at `target_path`,
/// relativizing an absolute link target and using a directory link or
/// junction when the source points at a directory.
pub fn recreate_internal_symlink(source_path: &Path, target_path: &Path) -> io::Result<()> {
    let contents =
        crate::read_symlink_dir(source_path).map(|link| relative_link_target(source_path, link))?;
    if fs::metadata(source_path).is_ok_and(|metadata| metadata.is_dir()) {
        let link_dir = target_path.parent().unwrap_or_else(|| Path::new(""));
        let original = crate::lexical_normalize(&link_dir.join(&contents));
        crate::symlink_dir_with_contents(&original, &contents, target_path)
    } else {
        symlink_file_or_copy(&contents, source_path, target_path)
    }
}

/// Create a file link at `link` holding `contents`. A file symlink has no
/// junction to fall back to, so a process Windows refuses symlinks gets a
/// copy of `original`, the file the link points at.
pub fn symlink_file_or_copy(contents: &Path, original: &Path, link: &Path) -> io::Result<()> {
    let result = crate::create_symlink(contents, link, false);
    #[cfg(windows)]
    {
        if let Err(error) = &result
            && error.raw_os_error() == Some(ERROR_PRIVILEGE_NOT_HELD)
        {
            return fs::copy(original, link).map(drop);
        }
    }
    #[cfg(not(windows))]
    let _ = original;
    result
}

#[cfg(windows)]
const ERROR_PRIVILEGE_NOT_HELD: i32 = 1314;
