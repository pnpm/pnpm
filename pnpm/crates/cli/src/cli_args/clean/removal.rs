use super::{
    CleanScope, LockfileRemoval,
    fs_ops::{
        inspect_existing_path, print_removing, remove_path, resolve_configured_lockfile_dir,
        resolve_inside, resolve_project_modules_dir,
    },
    planned::{ModulesCleanMode, PlannedClean},
};
use miette::{Context, IntoDiagnostic};
use pnpm_config::Config;
use pnpm_fs::{is_subdir, is_symlink_metadata, resolve_path_with_symlinks, to_native_separators};
use serde_json::Value;
use std::{
    borrow::Cow,
    collections::HashSet,
    io::ErrorKind,
    path::{Path, PathBuf},
};

/// The pnpm hidden entries inside `node_modules` that `clean` removes
/// alongside the regular package directories. Any other dotfile (e.g.
/// `.cache`) is left in place.
pub(super) const PNPM_HIDDEN_ENTRIES: &[&str] =
    &[".bin", ".modules.yaml", ".pnpm", ".pnpm-workspace-state-v1.json"];

/// Hidden entries removed from the shared workspace `node_modules` when a
/// narrowed clean targets the root importer: `.pnpm` and `.modules.yaml` back
/// unselected workspace members and stay in place, while
/// `.pnpm-workspace-state-v1.json` is removed so subsequent installs do not
/// short-circuit over the cleaned project.
const PNPM_NARROWED_SHARED_HIDDEN_ENTRIES: &[&str] = &[".bin", ".pnpm-workspace-state-v1.json"];

pub(super) struct CleanRun<'a> {
    pub(super) config: &'a Config,
    pub(super) root_dir: &'a Path,
    pub(super) real_root_dir: &'a Path,
    pub(super) lockfile_removal: LockfileRemoval,
    pub(super) scope: CleanScope,
    pub(super) raw_lockfile_dir: Option<&'a Path>,
    pub(super) protected_member_dirs: &'a [PathBuf],
}

impl CleanRun<'_> {
    pub(super) fn plan(&self, dirs: &[PathBuf]) -> PlannedClean {
        let modules_leaf = configured_modules_leaf(self.config);
        let resolved_lockfile_dir = self.raw_lockfile_dir.and_then(|lockfile_dir| {
            resolve_configured_lockfile_dir(self.real_root_dir, lockfile_dir)
        });
        let shared_modules_dir =
            self.shared_workspace_modules_dir(&modules_leaf, resolved_lockfile_dir.as_deref());
        let modules_targets = self.collect_modules_targets(
            dirs,
            &modules_leaf,
            resolved_lockfile_dir.as_deref(),
            shared_modules_dir.as_deref(),
        );
        let external_virtual_store_dir = match self.scope {
            CleanScope::WholeWorkspace => self.external_virtual_store_target(&modules_leaf),
            CleanScope::Narrowed => None,
        };
        PlannedClean { modules_targets, shared_modules_dir, external_virtual_store_dir }
    }

    pub(super) fn clean_dirs(&self, dirs: &[PathBuf]) -> miette::Result<()> {
        // The `Removing <path>` lines render each target relative to the cwd.
        // It is canonicalized once here so it shares the symlink-resolved
        // representation of the paths built from the canonicalized `--dir`; on
        // Windows the raw `current_dir()` can differ (casing, 8.3 short names,
        // junctions) and defeat the relative-path computation.
        let cwd = std::env::current_dir().and_then(dunce::canonicalize).unwrap_or_default();
        let planned = self.plan(dirs);
        for (modules_dir, mode) in &planned.modules_targets {
            clean_modules_dir(&cwd, modules_dir, *mode)?;
        }
        if let Some(shared_modules) = planned.shared_modules_dir.as_deref() {
            invalidate_workspace_state(self.real_root_dir, shared_modules)?;
        }
        if matches!(self.lockfile_removal, LockfileRemoval::Remove) {
            let resolved_lockfile_dir = self.raw_lockfile_dir.and_then(|lockfile_dir| {
                resolve_configured_lockfile_dir(self.real_root_dir, lockfile_dir)
            });
            remove_lockfiles(
                &cwd,
                self.real_root_dir,
                self.root_dir,
                dirs,
                self.cleans_root(dirs),
                resolved_lockfile_dir.as_deref(),
            )?;
        }
        if let Some(virtual_store_in_project) = planned
            .external_virtual_store_dir
            .as_deref()
            && inspect_existing_path(virtual_store_in_project)?.is_some()
        {
            print_removing(&cwd, virtual_store_in_project);
            remove_path(virtual_store_in_project)?;
        }
        Ok(())
    }

    fn cleans_root(&self, dirs: &[PathBuf]) -> bool {
        match self.scope {
            CleanScope::WholeWorkspace => true,
            CleanScope::Narrowed => dirs
                .iter()
                .any(|dir| dunce::canonicalize(dir).ok().as_deref() == Some(self.real_root_dir)),
        }
    }

    fn shared_workspace_modules_dir(
        &self,
        modules_leaf: &Path,
        resolved_lockfile_dir: Option<&Path>,
    ) -> Option<PathBuf> {
        match self.scope {
            CleanScope::WholeWorkspace => return None,
            CleanScope::Narrowed => {}
        }
        if self.config.workspace_dir.is_none() || !self.config.shares_one_lockfile() {
            return None;
        }
        let anchor = self.shared_anchor_dir(resolved_lockfile_dir)?;
        resolve_project_modules_dir(self.real_root_dir, anchor, modules_leaf)
    }

    fn shared_anchor_dir<'b>(
        &'b self,
        resolved_lockfile_dir: Option<&'b Path>,
    ) -> Option<&'b Path> {
        let Some(raw) = self.raw_lockfile_dir else {
            return Some(self.root_dir);
        };
        if let Some(resolved) = resolved_lockfile_dir {
            return Some(resolved);
        }
        let candidate = resolve_path_with_symlinks(&to_native_separators(raw))
            .and_then(dunce::canonicalize)
            .ok()?;
        (candidate == self.real_root_dir).then_some(self.root_dir)
    }

    fn is_cleanable_modules_dir(&self, candidate: &Path, cleaned: &HashSet<PathBuf>) -> bool {
        !cleaned.contains(candidate)
            && !self.protected_member_dirs.iter().any(|member_dir| is_subdir(candidate, member_dir))
    }

    fn collect_modules_targets(
        &self,
        dirs: &[PathBuf],
        modules_leaf: &Path,
        resolved_lockfile_dir: Option<&Path>,
        shared_modules_dir: Option<&Path>,
    ) -> Vec<(PathBuf, ModulesCleanMode)> {
        let mut cleaned_modules_dirs = HashSet::new();
        let mut targets = Vec::new();
        for dir in dirs {
            let project_leaf = modules_leaf_for_project(self.config, dir, modules_leaf);
            if let Some(modules_in_project) =
                resolve_project_modules_dir(self.real_root_dir, dir, &project_leaf)
                && self.is_cleanable_modules_dir(&modules_in_project, &cleaned_modules_dirs)
            {
                let mode = if shared_modules_dir == Some(modules_in_project.as_path()) {
                    ModulesCleanMode::PreserveSharedVirtualStore
                } else {
                    ModulesCleanMode::AllEntries
                };
                cleaned_modules_dirs.insert(modules_in_project.clone());
                targets.push((modules_in_project, mode));
            }
        }
        if matches!(self.scope, CleanScope::WholeWorkspace)
            && let Some(lockfile_dir) = resolved_lockfile_dir
            && let Some(lockfile_modules_dir) =
                resolve_project_modules_dir(self.real_root_dir, lockfile_dir, modules_leaf)
            && self.is_cleanable_modules_dir(&lockfile_modules_dir, &cleaned_modules_dirs)
        {
            targets.push((lockfile_modules_dir, ModulesCleanMode::AllEntries));
        }
        targets
    }

    fn external_virtual_store_target(&self, modules_leaf: &Path) -> Option<PathBuf> {
        let root_dir = self.config.workspace_dir.as_deref().unwrap_or(self.real_root_dir);
        let resolved_virtual_store_dir = configured_virtual_store_dir(root_dir, self.config);
        let virtual_store_in_project =
            resolve_inside(self.real_root_dir, &resolved_virtual_store_dir)?;
        if resolve_inside(self.real_root_dir, &root_dir.join(modules_leaf))
            .is_some_and(|root_modules_dir| is_subdir(&root_modules_dir, &virtual_store_in_project))
            || self.protected_member_dirs
                .iter()
                .any(|member_dir| is_subdir(&virtual_store_in_project, member_dir))
        {
            return None;
        }
        Some(virtual_store_in_project)
    }
}

pub(super) fn invalidate_workspace_state(
    real_root_dir: &Path,
    shared_modules: &Path,
) -> miette::Result<()> {
    let Some(state_path) =
        resolve_inside(real_root_dir, &shared_modules.join(".pnpm-workspace-state-v1.json"))
    else {
        return Ok(());
    };
    let Some(metadata) = inspect_existing_path(&state_path)? else {
        return Ok(());
    };
    if !is_symlink_metadata(&metadata) && metadata.is_dir() {
        return Ok(());
    }
    remove_path(&state_path)
}

pub(super) fn configured_modules_leaf(config: &Config) -> Cow<'_, Path> {
    to_native_separators(
        config.explicit_settings
            .get("modulesDir")
            .and_then(Value::as_str)
            .map(Path::new)
            .unwrap_or_else(|| Path::new("node_modules")),
    )
}

pub(super) fn modules_leaf_for_project<'a>(
    config: &'a Config,
    project_dir: &Path,
    default_leaf: &'a Path,
) -> Cow<'a, Path> {
    if config.applies_package_configs()
        && let Some(project_name) = pnpm_workspace::read_project_name(project_dir)
        && let Some(raw_modules_dir) = config.package_configs
            .as_ref()
            .and_then(|configs| configs.get(&project_name))
            .and_then(|project_config| project_config.modules_dir.as_deref())
    {
        return to_native_separators(Path::new(raw_modules_dir));
    }
    Cow::Borrowed(default_leaf)
}

fn remove_lockfiles(
    cwd: &Path,
    real_root_dir: &Path,
    root_dir: &Path,
    dirs: &[PathBuf],
    cleans_root: bool,
    resolved_lockfile_dir: Option<&Path>,
) -> miette::Result<()> {
    let mut removed = HashSet::new();
    for dir in dirs {
        remove_lockfile_at(cwd, real_root_dir, &dir.join("pnpm-lock.yaml"), &mut removed)?;
    }
    if cleans_root {
        remove_lockfile_at(cwd, real_root_dir, &root_dir.join("pnpm-lock.yaml"), &mut removed)?;
        if let Some(lockfile_dir) = resolved_lockfile_dir {
            remove_lockfile_at(
                cwd,
                real_root_dir,
                &lockfile_dir.join("pnpm-lock.yaml"),
                &mut removed,
            )?;
        }
    }
    Ok(())
}

fn remove_lockfile_at(
    cwd: &Path,
    real_root_dir: &Path,
    candidate: &Path,
    removed: &mut HashSet<PathBuf>,
) -> miette::Result<()> {
    let Some(lockfile_path) = resolve_inside(real_root_dir, candidate) else {
        return Ok(());
    };
    if !removed.insert(lockfile_path.clone()) {
        return Ok(());
    }
    let Some(metadata) = inspect_existing_path(&lockfile_path)? else {
        return Ok(());
    };
    if !is_symlink_metadata(&metadata) && metadata.is_dir() {
        return Ok(());
    }
    print_removing(cwd, &lockfile_path);
    remove_path(&lockfile_path)
}

pub(super) fn configured_virtual_store_dir(root_dir: &Path, config: &Config) -> PathBuf {
    let virtual_store_dir = if ["virtualStoreDir", "globalVirtualStoreDir"]
        .iter()
        .any(|setting| config.explicit_settings.contains_key(*setting))
    {
        config.virtual_store_dir()
    } else {
        &config.install_state_dir
    };
    if virtual_store_dir.is_absolute() {
        virtual_store_dir.to_path_buf()
    } else {
        root_dir.join(virtual_store_dir)
    }
}

pub(super) fn clean_modules_dir(
    cwd: &Path,
    modules_dir: &Path,
    mode: ModulesCleanMode,
) -> miette::Result<()> {
    let Some(metadata) = inspect_existing_path(modules_dir)? else {
        return Ok(());
    };
    if is_symlink_metadata(&metadata) {
        match mode {
            ModulesCleanMode::PreserveSharedVirtualStore => return Ok(()),
            ModulesCleanMode::AllEntries => {
                print_removing(cwd, modules_dir);
                return remove_path(modules_dir);
            }
        }
    }
    if !metadata.is_dir() {
        return Ok(());
    }
    let entries = removable_modules_entries(modules_dir, mode)?;
    if !entries.is_empty() {
        print_removing(cwd, modules_dir);
        for entry in entries {
            remove_path(&entry)?;
        }
    }
    Ok(())
}

fn removable_modules_entries(
    modules_dir: &Path,
    mode: ModulesCleanMode,
) -> miette::Result<Vec<PathBuf>> {
    let entries = match std::fs::read_dir(modules_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(error)
                .into_diagnostic()
                .wrap_err_with(|| format!("reading {}", modules_dir.display()));
        }
    };
    let hidden_entries = match mode {
        ModulesCleanMode::PreserveSharedVirtualStore => PNPM_NARROWED_SHARED_HIDDEN_ENTRIES,
        ModulesCleanMode::AllEntries => PNPM_HIDDEN_ENTRIES,
    };
    let mut removable = Vec::new();
    for entry in entries {
        let entry = entry
            .into_diagnostic()
            .wrap_err_with(|| format!("reading {}", modules_dir.display()))?;
        if is_pnpm_entry(&entry.file_name().to_string_lossy(), hidden_entries) {
            removable.push(entry.path());
        }
    }
    Ok(removable)
}

pub(super) fn is_pnpm_entry(name: &str, hidden_entries: &[&str]) -> bool {
    !name.starts_with('.') || hidden_entries.contains(&name)
}
