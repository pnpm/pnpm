use super::{LinkBinsError, LinkBinsOptions};
use pnpm_fs::{is_subdir, realpath_missing};
use std::{
    borrow::Cow,
    ffi::OsStr,
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
