use super::{
    fs_ops::{inspect_existing_path, print_removing, remove_path, resolve_inside},
    removal::{
        PNPM_HIDDEN_ENTRIES, PNPM_NARROWED_SHARED_HIDDEN_ENTRIES, clean_modules_dir,
        invalidate_workspace_state, is_pnpm_entry,
    },
};
use pnpm_fs::{is_subdir, is_symlink_metadata};
use std::{
    ffi::{OsStr, OsString},
    path::{Component, Path, PathBuf},
};

#[derive(Clone)]
pub(super) enum ModulesCleanMode {
    AllEntries { preserved_entry: Option<OsString> },
    PreserveSharedVirtualStore { preserved_entry: Option<OsString> },
}

impl ModulesCleanMode {
    pub(super) fn for_target(
        modules_dir: &Path,
        shared_modules_dir: Option<&Path>,
        shared_virtual_store_dir: Option<&Path>,
    ) -> Self {
        let preserved_entry = shared_virtual_store_dir
            .and_then(|vs_dir| vs_dir.strip_prefix(modules_dir).ok())
            .and_then(|rel| match rel.components().next()? {
                Component::Normal(first) => Some(first.to_os_string()),
                _ => None,
            });
        if shared_modules_dir == Some(modules_dir) {
            Self::PreserveSharedVirtualStore { preserved_entry }
        } else {
            Self::AllEntries { preserved_entry }
        }
    }

    pub(super) fn should_remove_dir_entry(&self, name: &OsStr) -> bool {
        let lossy = name.to_string_lossy();
        match self {
            Self::AllEntries { preserved_entry } => {
                preserved_entry.as_deref() != Some(name)
                    && is_pnpm_entry(&lossy, PNPM_HIDDEN_ENTRIES)
            }
            Self::PreserveSharedVirtualStore { preserved_entry } => {
                preserved_entry.as_deref() != Some(name)
                    && is_pnpm_entry(&lossy, PNPM_NARROWED_SHARED_HIDDEN_ENTRIES)
            }
        }
    }

    fn removes_entry(&self, modules_dir: &Path, entry_name: &OsStr) -> miette::Result<bool> {
        if let Some(metadata) = inspect_existing_path(modules_dir)?
            && is_symlink_metadata(&metadata)
        {
            return Ok(match self {
                Self::AllEntries { preserved_entry: None } => true,
                Self::AllEntries { preserved_entry: Some(_) }
                | Self::PreserveSharedVirtualStore { .. } => false,
            });
        }
        Ok(self.should_remove_dir_entry(entry_name))
    }
}

pub(super) struct PlannedClean {
    pub(super) modules_targets: Vec<(PathBuf, ModulesCleanMode)>,
    pub(super) shared_modules_dir: Option<PathBuf>,
    pub(super) external_virtual_store_dir: Option<PathBuf>,
}

impl PlannedClean {
    pub(super) fn execute(&self, cwd: &Path, real_root_dir: &Path) -> miette::Result<()> {
        self.execute_modules(cwd, real_root_dir)?;
        self.execute_external_virtual_store(cwd)
    }

    pub(super) fn execute_modules(&self, cwd: &Path, real_root_dir: &Path) -> miette::Result<()> {
        for (modules_dir, mode) in &self.modules_targets {
            clean_modules_dir(cwd, modules_dir, mode)?;
        }
        if let Some(shared_modules) = self.shared_modules_dir.as_deref() {
            invalidate_workspace_state(real_root_dir, shared_modules)?;
        }
        Ok(())
    }

    pub(super) fn execute_external_virtual_store(&self, cwd: &Path) -> miette::Result<()> {
        if let Some(virtual_store_in_project) = self.external_virtual_store_dir.as_deref()
            && inspect_existing_path(virtual_store_in_project)?.is_some()
        {
            print_removing(cwd, virtual_store_in_project);
            remove_path(virtual_store_in_project)?;
        }
        Ok(())
    }

    pub(super) fn removes_install_state(
        &self,
        real_root_dir: &Path,
        install_state_dir: &Path,
    ) -> miette::Result<bool> {
        if self.removes_via_ancestor(real_root_dir, install_state_dir)? {
            return Ok(true);
        }
        let Some(real_state_dir) = resolve_inside(real_root_dir, install_state_dir) else {
            return Ok(false);
        };
        if self.external_virtual_store_dir
            .as_ref()
            .is_some_and(|vs_dir| is_subdir(vs_dir, &real_state_dir))
        {
            return Ok(true);
        }
        for (dir, mode) in &self.modules_targets {
            if removes_subpath_in_modules(dir, mode, &real_state_dir)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn removes_via_ancestor(
        &self,
        real_root_dir: &Path,
        install_state_dir: &Path,
    ) -> miette::Result<bool> {
        let mut current = install_state_dir;
        while let (Some(parent), Some(name)) = (current.parent(), current.file_name()) {
            if let Some(resolved_parent) = resolve_inside(real_root_dir, parent)
                && self.target_removes_child(&resolved_parent, name)?
            {
                return Ok(true);
            }
            current = parent;
        }
        Ok(false)
    }

    fn target_removes_child(&self, resolved_parent: &Path, name: &OsStr) -> miette::Result<bool> {
        for (dir, mode) in &self.modules_targets {
            if dir == resolved_parent && mode.removes_entry(dir, name)? {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

fn removes_subpath_in_modules(
    modules_dir: &Path,
    mode: &ModulesCleanMode,
    target: &Path,
) -> miette::Result<bool> {
    let Ok(rel) = target.strip_prefix(modules_dir) else {
        return Ok(false);
    };
    let entry_name = rel
        .components()
        .next()
        .map(Component::as_os_str)
        .unwrap_or_else(|| OsStr::new("lock.yaml"));
    mode.removes_entry(modules_dir, entry_name)
}
