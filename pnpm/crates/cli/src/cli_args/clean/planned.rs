use super::{
    fs_ops::{inspect_existing_path, print_removing, remove_path, resolve_inside},
    removal::{
        PNPM_HIDDEN_ENTRIES, PNPM_NARROWED_SHARED_HIDDEN_ENTRIES, clean_modules_dir,
        invalidate_workspace_state, is_pnpm_entry,
    },
};
use pnpm_fs::{is_subdir, is_symlink_metadata};
use std::{
    borrow::Cow,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy)]
pub(super) enum ModulesCleanMode {
    AllEntries,
    PreserveSharedVirtualStore,
}

impl ModulesCleanMode {
    pub(super) fn hidden_entries(self) -> &'static [&'static str] {
        match self {
            Self::AllEntries => PNPM_HIDDEN_ENTRIES,
            Self::PreserveSharedVirtualStore => PNPM_NARROWED_SHARED_HIDDEN_ENTRIES,
        }
    }

    fn removes_entry(self, modules_dir: &Path, entry_name: &str) -> bool {
        if inspect_existing_path(modules_dir)
            .ok()
            .flatten()
            .is_some_and(|metadata| is_symlink_metadata(&metadata))
        {
            return match self {
                Self::AllEntries => true,
                Self::PreserveSharedVirtualStore => false,
            };
        }
        is_pnpm_entry(entry_name, self.hidden_entries())
    }
}

pub(super) struct PlannedClean {
    pub(super) modules_targets: Vec<(PathBuf, ModulesCleanMode)>,
    pub(super) shared_modules_dir: Option<PathBuf>,
    pub(super) external_virtual_store_dir: Option<PathBuf>,
}

impl PlannedClean {
    pub(super) fn execute(&self, cwd: &Path, real_root_dir: &Path) -> miette::Result<()> {
        for (modules_dir, mode) in &self.modules_targets {
            clean_modules_dir(cwd, modules_dir, *mode)?;
        }
        if let Some(shared_modules) = self.shared_modules_dir.as_deref() {
            invalidate_workspace_state(real_root_dir, shared_modules)?;
        }
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
    ) -> bool {
        if self.removes_via_ancestor(real_root_dir, install_state_dir) {
            return true;
        }
        let Some(real_state_dir) = resolve_inside(real_root_dir, install_state_dir) else {
            return false;
        };
        if self.external_virtual_store_dir
            .as_ref()
            .is_some_and(|vs_dir| is_subdir(vs_dir, &real_state_dir))
        {
            return true;
        }
        self.modules_targets
            .iter()
            .any(|(dir, mode)| removes_subpath_in_modules(dir, *mode, &real_state_dir))
    }

    fn removes_via_ancestor(&self, real_root_dir: &Path, install_state_dir: &Path) -> bool {
        let mut current = install_state_dir;
        while let (Some(parent), Some(name)) = (current.parent(), current.file_name()) {
            let entry_name = name.to_string_lossy();
            if let Some(resolved_parent) = resolve_inside(real_root_dir, parent)
                && self.modules_targets
                    .iter()
                    .any(|(dir, mode)| {
                        *dir == resolved_parent && mode.removes_entry(dir, &entry_name)
                    })
            {
                return true;
            }
            current = parent;
        }
        false
    }
}

fn removes_subpath_in_modules(modules_dir: &Path, mode: ModulesCleanMode, target: &Path) -> bool {
    let Ok(rel) = target.strip_prefix(modules_dir) else {
        return false;
    };
    let entry_name = rel
        .components()
        .next()
        .map_or(Cow::Borrowed("lock.yaml"), |first| first.as_os_str().to_string_lossy());
    mode.removes_entry(modules_dir, &entry_name)
}
