use super::{CleanScope, LockfileRemoval};
use miette::{Context, IntoDiagnostic};
use pnpm_config::Config;
use pnpm_fs::{
    is_subdir, is_symlink_metadata, relative_path, remove_dirent, resolve_path_with_symlinks,
    symlink_metadata_with_retry, to_native_separators,
};
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
const PNPM_HIDDEN_ENTRIES: &[&str] =
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
}

impl CleanRun<'_> {
    pub(super) fn clean_dirs(&self, dirs: &[PathBuf]) -> miette::Result<()> {
        // The `Removing <path>` lines render each target relative to the cwd.
        // It is canonicalized once here so it shares the symlink-resolved
        // representation of the paths built from the canonicalized `--dir`; on
        // Windows the raw `current_dir()` can differ (casing, 8.3 short names,
        // junctions) and defeat the relative-path computation.
        let cwd = std::env::current_dir().and_then(dunce::canonicalize).unwrap_or_default();
        // `pnpm clean` resolves `modulesDir` against every project directory it
        // cleans. `config.modules_dir` is already anchored — at the workspace
        // root, or at a configured `lockfileDir` — so rejoining it per project
        // would send every project back to that one directory. Take the
        // configured leaf instead; an absolute setting survives `join`, as it
        // does pnpm's `pathAbsolute`.
        let modules_leaf = configured_modules_leaf(self.config);
        let cleans_root = self.cleans_root(dirs);
        let resolved_lockfile_dir = self.raw_lockfile_dir.and_then(|lockfile_dir| {
            resolve_configured_lockfile_dir(self.real_root_dir, lockfile_dir)
        });
        self.clean_all_modules(&cwd, dirs, &modules_leaf, resolved_lockfile_dir.as_deref())?;
        if self.lockfile_removal == LockfileRemoval::Remove {
            remove_lockfiles(
                &cwd,
                self.real_root_dir,
                self.root_dir,
                dirs,
                cleans_root,
                resolved_lockfile_dir.as_deref(),
            )?;
        }
        if self.scope == CleanScope::WholeWorkspace {
            remove_external_virtual_store(&cwd, self.config, self.real_root_dir, &modules_leaf)?;
        }
        Ok(())
    }

    fn cleans_root(&self, dirs: &[PathBuf]) -> bool {
        self.scope == CleanScope::WholeWorkspace
            || dirs
                .iter()
                .any(|dir| dunce::canonicalize(dir).ok().as_deref() == Some(self.real_root_dir))
    }

    fn shared_workspace_modules_dir(
        &self,
        modules_leaf: &Path,
        resolved_lockfile_dir: Option<&Path>,
    ) -> Option<PathBuf> {
        if self.scope != CleanScope::Narrowed
            || self.config.workspace_dir.is_none()
            || !self.config.shares_one_lockfile()
        {
            return None;
        }
        let anchor =
            if self.raw_lockfile_dir.is_some() { resolved_lockfile_dir? } else { self.root_dir };
        resolve_project_modules_dir(self.real_root_dir, anchor, modules_leaf)
    }

    fn clean_all_modules(
        &self,
        cwd: &Path,
        dirs: &[PathBuf],
        modules_leaf: &Path,
        resolved_lockfile_dir: Option<&Path>,
    ) -> miette::Result<()> {
        let shared_modules_dir =
            self.shared_workspace_modules_dir(modules_leaf, resolved_lockfile_dir);
        let mut cleaned_modules_dirs = HashSet::new();
        for dir in dirs {
            let project_leaf = modules_leaf_for_project(self.config, dir, modules_leaf);
            if let Some(modules_in_project) =
                resolve_project_modules_dir(self.real_root_dir, dir, &project_leaf)
                && cleaned_modules_dirs.insert(modules_in_project.clone())
            {
                let preserve_shared =
                    shared_modules_dir.as_deref() == Some(modules_in_project.as_path());
                clean_modules_dir(cwd, &modules_in_project, preserve_shared)?;
            }
        }
        if let Some(shared_modules) = shared_modules_dir.as_deref() {
            invalidate_workspace_state(self.real_root_dir, shared_modules)?;
        }
        if self.scope == CleanScope::WholeWorkspace
            && let Some(lockfile_dir) = resolved_lockfile_dir
            && let Some(lockfile_modules_dir) =
                resolve_project_modules_dir(self.real_root_dir, lockfile_dir, modules_leaf)
            && cleaned_modules_dirs.insert(lockfile_modules_dir.clone())
        {
            clean_modules_dir(cwd, &lockfile_modules_dir, false)?;
        }
        Ok(())
    }
}

fn invalidate_workspace_state(real_root_dir: &Path, shared_modules: &Path) -> miette::Result<()> {
    let Some(state_path) =
        resolve_inside(real_root_dir, &shared_modules.join(".pnpm-workspace-state-v1.json"))
    else {
        return Ok(());
    };
    let Ok(metadata) = symlink_metadata_with_retry(&state_path) else {
        return Ok(());
    };
    if !is_symlink_metadata(&metadata) && metadata.is_dir() {
        return Ok(());
    }
    remove_path(&state_path)
}

fn configured_modules_leaf(config: &Config) -> Cow<'_, Path> {
    to_native_separators(
        config.explicit_settings
            .get("modulesDir")
            .and_then(Value::as_str)
            .map(Path::new)
            .unwrap_or_else(|| Path::new("node_modules")),
    )
}

fn modules_leaf_for_project<'a>(
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

fn resolve_configured_lockfile_dir(
    real_root_dir: &Path,
    raw_lockfile_dir: &Path,
) -> Option<PathBuf> {
    let candidate = resolve_inside(real_root_dir, raw_lockfile_dir)?;
    let real_lockfile_dir =
        resolve_path_with_symlinks(&candidate).and_then(dunce::canonicalize).ok()?;
    (real_lockfile_dir.starts_with(real_root_dir) && real_lockfile_dir != real_root_dir).then_some(
        real_lockfile_dir,
    )
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
    let Ok(metadata) = symlink_metadata_with_retry(&lockfile_path) else {
        return Ok(());
    };
    if !is_symlink_metadata(&metadata) && metadata.is_dir() {
        return Ok(());
    }
    print_removing(cwd, &lockfile_path);
    remove_path(&lockfile_path)
}

/// A virtual store dir configured outside `node_modules` (e.g. a custom
/// `virtual-store-dir`) is removed separately; the default
/// `node_modules/.pnpm` is cleaned along with the modules dir's
/// contents. Under a global virtual store, `virtualStoreDir` or
/// `globalVirtualStoreDir` names the shared store's root, which is removed
/// as well; the default store under `storeDir` is kept.
fn remove_external_virtual_store(
    cwd: &Path,
    config: &Config,
    real_root_dir: &Path,
    modules_leaf: &Path,
) -> miette::Result<()> {
    let root_dir = config.workspace_dir.as_deref().unwrap_or(real_root_dir);
    let virtual_store_dir = if ["virtualStoreDir", "globalVirtualStoreDir"]
        .iter()
        .any(|setting| config.explicit_settings.contains_key(*setting))
    {
        config.virtual_store_dir()
    } else {
        &config.install_state_dir
    };
    let resolved_virtual_store_dir = if virtual_store_dir.is_absolute() {
        virtual_store_dir.to_path_buf()
    } else {
        root_dir.join(virtual_store_dir)
    };
    let Some(virtual_store_in_project) = resolve_inside(real_root_dir, &resolved_virtual_store_dir)
    else {
        return Ok(());
    };
    if resolve_inside(real_root_dir, &root_dir.join(modules_leaf))
        .is_some_and(|root_modules_dir| is_subdir(&root_modules_dir, &virtual_store_in_project))
    {
        return Ok(());
    }
    if symlink_metadata_with_retry(&virtual_store_in_project).is_ok() {
        print_removing(cwd, &virtual_store_in_project);
        remove_path(&virtual_store_in_project)?;
    }
    Ok(())
}

fn resolve_project_modules_dir(
    real_root_dir: &Path,
    project_dir: &Path,
    modules_leaf: &Path,
) -> Option<PathBuf> {
    let real_project_dir = dunce::canonicalize(project_dir).ok()?;
    let resolved = resolve_inside(&real_project_dir, &project_dir.join(modules_leaf))?;
    (resolved.starts_with(real_root_dir) && resolved != real_root_dir).then_some(resolved)
}

/// `path` with its parent resolved through symlinks, when it names an entry
/// strictly inside canonical `real_root`. A symlinked ancestor cannot carry
/// the removal out of the project, and the last component stays unresolved so
/// a symlinked target is unlinked rather than followed. `None` when the path
/// cannot be resolved.
fn resolve_inside(real_root: &Path, path: &Path) -> Option<PathBuf> {
    let path = to_native_separators(path);
    let file_name = path.file_name()?;
    // Windows strips trailing dots and spaces on non-verbatim paths, so `. ` or
    // `.. ` would pass lexical leaf checks while naming `real_root` or its parent.
    if file_name
        .to_string_lossy()
        .trim_end_matches(['.', ' '])
        .is_empty()
    {
        return None;
    }
    let resolved = resolve_path_with_symlinks(path.parent()?)
        .and_then(dunce::canonicalize)
        .ok()?
        .join(file_name);
    (resolved.starts_with(real_root) && resolved != real_root).then_some(resolved)
}

fn clean_modules_dir(cwd: &Path, modules_dir: &Path, preserve_shared: bool) -> miette::Result<()> {
    let metadata = match symlink_metadata_with_retry(modules_dir) {
        Ok(metadata) => metadata,
        Err(error) if matches!(error.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) => {
            return Ok(());
        }
        Err(error) => {
            return Err(error)
                .into_diagnostic()
                .wrap_err_with(|| format!("inspecting {}", modules_dir.display()));
        }
    };
    if is_symlink_metadata(&metadata) {
        if preserve_shared {
            return Ok(());
        }
        print_removing(cwd, modules_dir);
        return remove_path(modules_dir);
    }
    if !metadata.is_dir() {
        return Ok(());
    }
    let entries = removable_modules_entries(modules_dir, preserve_shared)?;
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
    preserve_shared: bool,
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
    let hidden_entries =
        if preserve_shared { PNPM_NARROWED_SHARED_HIDDEN_ENTRIES } else { PNPM_HIDDEN_ENTRIES };
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

fn remove_path(path: &Path) -> miette::Result<()> {
    remove_dirent(path)
        .or_else(|error| if error.kind() == ErrorKind::NotFound { Ok(()) } else { Err(error) })
        .into_diagnostic()
        .wrap_err_with(|| format!("removing {}", path.display()))
}

fn is_pnpm_entry(name: &str, hidden_entries: &[&str]) -> bool {
    !name.starts_with('.') || hidden_entries.contains(&name)
}

/// Print `Removing <path>`, with `path` rendered relative to `base` (the
/// canonicalized cwd), matching pnpm's `path.relative(process.cwd(), p)`
/// formatting. An empty relative path renders as `.`.
fn print_removing(base: &Path, path: &Path) {
    let relative = relative_path(base, path);
    let owned: PathBuf =
        if relative.as_os_str().is_empty() { PathBuf::from(".") } else { relative };
    pnpm_diagnostics::println!("Removing {}", owned.display());
}
