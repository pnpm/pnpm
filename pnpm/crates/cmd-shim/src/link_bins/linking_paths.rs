use super::{LinkBinsError, LinkBinsOptions};
use pnpm_fs::{is_subdir, realpath_missing};
use std::{
    borrow::Cow,
    path::{Path, PathBuf},
};

pub(super) struct LinkingPaths<'a> {
    pub(super) bins_dir: Cow<'a, Path>,
    pub(super) relocatable_root: Option<PathBuf>,
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
