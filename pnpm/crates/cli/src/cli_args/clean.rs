use super::{
    dispatch::RunCtx, recursive::discover_workspace_projects, reporter::ReporterType, run::RunArgs,
};
use derive_more::{Display, Error};
use miette::{Context, Diagnostic, IntoDiagnostic};
use pacquet_config::Config;
use pacquet_fs::{is_subdir, lexical_normalize, relative_path};
use pacquet_workspace::read_project_manifest_only;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// `pnpm clean` / `pnpm purge`: safely remove the `node_modules`
/// directories of the current project (or every project in the workspace)
/// without following NTFS junctions into their targets. A `clean` /
/// `purge` script in `package.json` overrides the built-in command,
/// mirroring pnpm's `overridableByScript` flag.
#[derive(Debug, clap::Args)]
pub struct CleanArgs {
    /// Also remove `pnpm-lock.yaml` files.
    #[clap(short = 'l', long = "lockfile")]
    pub lockfile: bool,
}

/// `pnpm clean` was invoked from a subdirectory of a workspace
/// whose root `package.json` declares a `clean` / `purge` script.
/// pnpm refuses to run the built-in from the subdirectory in that
/// case (it would shadow the root script), and directs the user to
/// `pnpm run <script>` at the root.
#[derive(Debug, Display, Error, Diagnostic)]
#[display(
    "The workspace root has a \"{command}\" script, so the built-in \"pnpm {command}\" command cannot run from a subdirectory"
)]
#[diagnostic(
    code(ERR_PNPM_SCRIPT_OVERRIDE_IN_WORKSPACE_ROOT),
    help("Run \"pnpm run {command}\" from the workspace root to execute the script")
)]
struct ScriptOverrideInWorkspaceRoot {
    command: String,
}

/// The pnpm hidden entries inside `node_modules` that `clean` removes
/// alongside the regular package directories. Any other dotfile (e.g.
/// `.cache`) is left in place.
const PNPM_HIDDEN_ENTRIES: &[&str] =
    &[".bin", ".modules.yaml", ".pnpm", ".pnpm-workspace-state-v1.json"];

impl CleanArgs {
    pub fn run(self, ctx: &RunCtx<'_>, command_name: &str) -> miette::Result<()> {
        let config = (ctx.config)()?;
        // A `<command_name>` script in the current project's `package.json`
        // replaces the built-in command.
        if let Some(script) = script_of(read_project_manifest_only(ctx.dir).ok(), command_name)
            && !script.is_empty()
        {
            return RunArgs {
                command: Some(command_name.to_string()),
                args: Vec::new(),
                if_present: false,
                resume_from: None,
                report_summary: false,
                no_bail: false,
                sort: true,
                sequential: false,
            }
            .run(ctx.dir, config, matches!(ctx.reporter, ReporterType::Silent));
        }
        // Inside a workspace subdirectory, a `<command_name>` script at the
        // workspace root must be run from the root rather than shadowed by
        // the built-in command here.
        if let Some(workspace_dir) = config.workspace_dir.as_deref()
            && lexical_normalize(workspace_dir) != lexical_normalize(ctx.dir)
            && let Some(script) =
                script_of(read_project_manifest_only(workspace_dir).ok(), command_name)
            && !script.is_empty()
        {
            return Err(ScriptOverrideInWorkspaceRoot { command: command_name.to_string() }.into());
        }
        clean_builtin(ctx, config, self.lockfile)
    }
}

/// Resolve the `<command_name>` script body from an optional manifest
/// (`None` when the manifest is absent), mirroring pnpm's
/// `safeReadProjectManifestOnly` tolerance for a missing `package.json`.
fn script_of(
    manifest: Option<pacquet_package_manifest::PackageManifest>,
    command_name: &str,
) -> Option<String> {
    manifest?
        .value()
        .get("scripts")
        .and_then(Value::as_object)
        .and_then(|scripts| scripts.get(command_name))
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// Remove `node_modules` contents and (optionally) lockfiles from the
/// current project or every workspace project.
fn clean_builtin(ctx: &RunCtx<'_>, config: &Config, remove_lockfile: bool) -> miette::Result<()> {
    // The `Removing <path>` lines render each target relative to the cwd.
    // It is canonicalized once here so it shares the symlink-resolved
    // representation of the paths built from the canonicalized `--dir`; on
    // Windows the raw `current_dir()` can differ (casing, 8.3 short names,
    // junctions) and defeat the relative-path computation.
    let cwd = std::env::current_dir().and_then(dunce::canonicalize).unwrap_or_default();
    // `pnpm clean` resolves the modules dir relative to each project
    // directory, not against a single absolute prefix, so strip the
    // config anchor back to the leaf and rejoin per project.
    let modules_leaf = config.modules_dir.strip_prefix(ctx.dir).unwrap_or(&config.modules_dir);
    let root_dir = config.workspace_dir.as_deref().unwrap_or(ctx.dir);
    let dirs: Vec<PathBuf> = if let Some(workspace_dir) = config.workspace_dir.as_deref() {
        let (projects, _patterns) = discover_workspace_projects(workspace_dir)?;
        projects.into_iter().map(|project| project.root_dir).collect()
    } else {
        vec![ctx.dir.to_path_buf()]
    };
    for dir in &dirs {
        let full_modules_dir = dir.join(modules_leaf);
        if has_contents_to_remove(&full_modules_dir) {
            print_removing(&cwd, &full_modules_dir);
            remove_modules_dir_contents(&full_modules_dir)?;
        }
    }
    if remove_lockfile {
        let lockfile_path = root_dir.join("pnpm-lock.yaml");
        if lockfile_path.exists() {
            print_removing(&cwd, &lockfile_path);
            std::fs::remove_file(&lockfile_path)
                .or_else(|error| {
                    if error.kind() == std::io::ErrorKind::NotFound { Ok(()) } else { Err(error) }
                })
                .into_diagnostic()
                .wrap_err_with(|| format!("removing {}", lockfile_path.display()))?;
        }
    }
    // A virtual store dir configured outside `node_modules` (e.g. a
    // custom `virtual-store-dir`) is removed separately; the default
    // `node_modules/.pnpm` is cleaned as part of the contents above.
    let resolved_virtual_store_dir: PathBuf = if config.virtual_store_dir.is_absolute() {
        config.virtual_store_dir.clone()
    } else {
        root_dir.join(&config.virtual_store_dir)
    };
    let root_modules_dir = root_dir.join(modules_leaf);
    if !is_subdir(&root_modules_dir, &resolved_virtual_store_dir)
        && is_subdir(root_dir, &resolved_virtual_store_dir)
        && resolved_virtual_store_dir.exists()
    {
        print_removing(&cwd, &resolved_virtual_store_dir);
        remove_path(&resolved_virtual_store_dir)?;
    }
    Ok(())
}

/// Whether `modules_dir` holds anything `clean` removes: a regular
/// package directory or one of the pnpm hidden entries. Other dotfiles
/// (e.g. `.cache`) mean "nothing to clean" on their own.
fn has_contents_to_remove(modules_dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(modules_dir) else {
        return false;
    };
    entries.filter_map(Result::ok).any(|entry| is_pnpm_entry(&entry.file_name().to_string_lossy()))
}

fn remove_modules_dir_contents(modules_dir: &Path) -> miette::Result<()> {
    let Ok(entries) = std::fs::read_dir(modules_dir) else {
        return Ok(());
    };
    for entry in entries.filter_map(Result::ok) {
        if !is_pnpm_entry(&entry.file_name().to_string_lossy()) {
            continue;
        }
        remove_path(&entry.path())?;
    }
    Ok(())
}

fn remove_path(path: &Path) -> miette::Result<()> {
    let result =
        if path.is_dir() { std::fs::remove_dir_all(path) } else { std::fs::remove_file(path) };
    result
        .or_else(
            |error| {
                if error.kind() == std::io::ErrorKind::NotFound { Ok(()) } else { Err(error) }
            },
        )
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
    println!("Removing {}", owned.display());
}
