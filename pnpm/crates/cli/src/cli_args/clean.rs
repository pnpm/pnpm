use super::{dispatch::RunCtx, recursive::discover_workspace_projects};
use miette::{Context, IntoDiagnostic};
use pnpm_config::Config;
use pnpm_fs::{
    is_subdir, is_symlink_metadata, relative_path, remove_dirent, resolve_path_with_symlinks,
    symlink_metadata_with_retry, to_native_separators,
};
use serde_json::Value;
use std::{
    io::ErrorKind,
    path::{Path, PathBuf},
};

/// `pnpm clean` / `pnpm purge`: safely remove the `node_modules`
/// directories of the current project (or every project in the workspace)
/// without following NTFS junctions into their targets. A `clean` /
/// `purge` script in `package.json` overrides the built-in command;
/// `pnpm pm clean` / `pnpm pm purge` runs the built-in regardless.
#[derive(Debug, clap::Args)]
pub struct CleanArgs {
    /// Also remove `pnpm-lock.yaml` files.
    #[clap(short = 'l', long = "lockfile")]
    pub lockfile: bool,
}

/// The pnpm hidden entries inside `node_modules` that `clean` removes
/// alongside the regular package directories. Any other dotfile (e.g.
/// `.cache`) is left in place.
const PNPM_HIDDEN_ENTRIES: &[&str] =
    &[".bin", ".modules.yaml", ".pnpm", ".pnpm-workspace-state-v1.json"];

/// Remove `node_modules` contents and (optionally) lockfiles from the
/// current project or every workspace project.
pub(super) fn run(ctx: &RunCtx<'_>, config: &Config, remove_lockfile: bool) -> miette::Result<()> {
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
    let modules_leaf = to_native_separators(
        config.explicit_settings
            .get("modulesDir")
            .and_then(Value::as_str)
            .map(Path::new)
            .unwrap_or_else(|| Path::new("node_modules")),
    );
    let root_dir = config.workspace_dir.as_deref().unwrap_or(ctx.locations.dir);
    let Ok(real_root_dir) = dunce::canonicalize(root_dir) else {
        return Ok(());
    };
    let dirs: Vec<PathBuf> = if let Some(workspace_dir) = config.workspace_dir.as_deref() {
        let (projects, _patterns) = discover_workspace_projects(workspace_dir, config)?;
        projects
            .into_iter()
            .map(|project| project.root_dir)
            .collect()
    } else {
        vec![ctx.locations.dir.to_path_buf()]
    };
    for dir in &dirs {
        if let Some(modules_in_project) =
            resolve_project_modules_dir(&real_root_dir, dir, &modules_leaf)
        {
            clean_modules_dir(&cwd, &modules_in_project)?;
        }
    }
    if remove_lockfile {
        remove_workspace_lockfile(&cwd, &real_root_dir, root_dir)?;
    }
    remove_external_virtual_store(&cwd, config, &real_root_dir, &modules_leaf)
}

fn remove_workspace_lockfile(
    cwd: &Path,
    real_root_dir: &Path,
    root_dir: &Path,
) -> miette::Result<()> {
    let Some(lockfile_path) = resolve_inside(real_root_dir, &root_dir.join("pnpm-lock.yaml"))
    else {
        return Ok(());
    };
    if symlink_metadata_with_retry(&lockfile_path).is_err() {
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
    let resolved_virtual_store_dir: PathBuf = if virtual_store_dir.is_absolute() {
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
    let resolved = resolve_path_with_symlinks(path.parent()?)
        .and_then(dunce::canonicalize)
        .ok()?
        .join(path.file_name()?);
    (resolved.starts_with(real_root) && resolved != real_root).then_some(resolved)
}

fn clean_modules_dir(cwd: &Path, modules_dir: &Path) -> miette::Result<()> {
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
        print_removing(cwd, modules_dir);
        return remove_path(modules_dir);
    }
    if !metadata.is_dir() {
        return Ok(());
    }
    let entries = removable_modules_entries(modules_dir)?;
    if !entries.is_empty() {
        print_removing(cwd, modules_dir);
        for entry in entries {
            remove_path(&entry)?;
        }
    }
    Ok(())
}

fn removable_modules_entries(modules_dir: &Path) -> miette::Result<Vec<PathBuf>> {
    let entries = match std::fs::read_dir(modules_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(error)
                .into_diagnostic()
                .wrap_err_with(|| format!("reading {}", modules_dir.display()));
        }
    };
    Ok(entries
        .filter_map(Result::ok)
        .filter(|entry| is_pnpm_entry(&entry.file_name().to_string_lossy()))
        .map(|entry| entry.path())
        .collect())
}

fn remove_path(path: &Path) -> miette::Result<()> {
    remove_dirent(path)
        .or_else(|error| if error.kind() == ErrorKind::NotFound { Ok(()) } else { Err(error) })
        .into_diagnostic()
        .wrap_err_with(|| format!("removing {}", path.display()))
}

fn is_pnpm_entry(name: &str) -> bool {
    !name.starts_with('.') || PNPM_HIDDEN_ENTRIES.contains(&name)
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
