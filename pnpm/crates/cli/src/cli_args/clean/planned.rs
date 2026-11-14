use super::{
    fs_ops::{inspect_existing_path, print_removing, remove_path, resolve_inside},
    removal::{PNPM_HIDDEN_ENTRIES, clean_modules_dir, invalidate_workspace_state, is_pnpm_entry},
};
use pnpm_fs::is_subdir;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ModulesCleanMode {
    AllEntries,
    PreserveSharedVirtualStore,
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
        if let (Some(parent), Some(name)) =
            (install_state_dir.parent(), install_state_dir.file_name())
            && is_pnpm_entry(&name.to_string_lossy(), PNPM_HIDDEN_ENTRIES)
            && let Some(resolved_parent) = resolve_inside(real_root_dir, parent)
            && self.modules_targets
                .iter()
                .any(|(dir, mode)| *mode == ModulesCleanMode::AllEntries && *dir == resolved_parent)
        {
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
            .any(|(dir, mode)| {
                *mode == ModulesCleanMode::AllEntries
                    && removes_subpath_in_all_entries_modules(dir, &real_state_dir)
            })
    }
}

fn removes_subpath_in_all_entries_modules(modules_dir: &Path, target: &Path) -> bool {
    let Ok(rel) = target.strip_prefix(modules_dir) else {
        return false;
    };
    rel.components()
        .next()
        .is_none_or(|first| {
            is_pnpm_entry(&first.as_os_str().to_string_lossy(), PNPM_HIDDEN_ENTRIES)
        })
}
