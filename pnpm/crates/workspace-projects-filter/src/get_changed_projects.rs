use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Command,
};

use indexmap::IndexMap;
use wax::{Glob, Program};

use crate::filter::FilterError;

mod catalogs;

/// Options for [`get_changed_projects`].
pub struct GetChangedProjectsOptions<'a> {
    /// Workspace root directory where `pnpm-workspace.yaml` resides.
    pub workspace_dir: &'a Path,
    /// Working directory for `git diff`: the selector's `{dir}` part when present,
    /// else the workspace root.
    pub working_dir: Option<&'a Path>,
    pub test_pattern: &'a [String],
    pub changed_files_ignore_pattern: &'a [String],
    pub project_dependencies: Option<&'a HashMap<PathBuf, Vec<(String, String)>>>,
    pub use_glob_dir_filtering: bool,
}

/// The two selection groups a `[<since>]` diff produces.
pub struct ChangedProjects {
    /// Projects with at least one changed file outside `test_pattern`;
    /// selected with the selector's full dependency/dependent walk.
    pub changed_projects: Vec<PathBuf>,
    /// Projects whose changed files all match `test_pattern`; selected
    /// themselves, but their dependents are not.
    pub ignore_dependent_for_projects: Vec<PathBuf>,
}

/// Split `project_dirs` into the projects changed since `commit` and
/// the projects whose only changes are test files.
pub fn get_changed_projects(
    project_dirs: Vec<PathBuf>,
    commit: &str,
    opts: &GetChangedProjectsOptions<'_>,
) -> Result<ChangedProjects, FilterError> {
    let repo_root = find_repo_root(opts.workspace_dir);
    let base = merge_base(commit, opts.workspace_dir)?;
    let ChangedDirsResult {
        changed_dirs,
        workspace_manifest_changed,
    } = get_changed_dirs_since_commit(&base, opts, &repo_root)?;

    let mut project_change_types: IndexMap<PathBuf, Option<ChangeType>> = project_dirs
        .into_iter()
        .map(|dir| (dir, None))
        .collect();
    for (changed_dir, change_type) in changed_dirs {
        let owner = owning_project(&repo_root, &changed_dir, &project_change_types);
        let entry = project_change_types.entry(owner).or_insert(None);
        if *entry != Some(ChangeType::Source) {
            *entry = Some(change_type);
        }
    }

    if workspace_manifest_changed {
        catalogs::apply_changed_catalogs(&mut project_change_types, &base, opts, &repo_root)?;
    }

    let mut changed_projects: Vec<PathBuf> = Vec::new();
    let mut ignore_dependent_for_projects: Vec<PathBuf> = Vec::new();
    for (dir, change_type) in project_change_types {
        match change_type {
            Some(ChangeType::Source) => changed_projects.push(dir),
            Some(ChangeType::Test) => ignore_dependent_for_projects.push(dir),
            None => {}
        }
    }
    Ok(ChangedProjects { changed_projects, ignore_dependent_for_projects })
}

fn owning_project(
    repo_root: &Path,
    changed_dir: &Path,
    project_change_types: &IndexMap<PathBuf, Option<ChangeType>>,
) -> PathBuf {
    let mut current = if changed_dir.as_os_str().is_empty() {
        repo_root.to_path_buf()
    } else {
        repo_root.join(changed_dir)
    };
    while !project_change_types.contains_key(&current) {
        let Some(parent) = current.parent() else { break };
        current = parent.to_path_buf();
    }
    current
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChangeType {
    Source,
    Test,
}

struct ChangedDirsResult {
    changed_dirs: IndexMap<PathBuf, ChangeType>,
    workspace_manifest_changed: bool,
}

struct DiffFilterContext<'a> {
    repo_root: &'a Path,
    workspace_manifest: &'a Path,
    working_dir: &'a Path,
    workspace_dir: &'a Path,
    ignore_globs: &'a [Glob<'a>],
    test_globs: &'a [Glob<'a>],
}

fn classify_changed_path(changed_file: &str, test_globs: &[Glob<'_>]) -> (PathBuf, ChangeType) {
    let dir = Path::new(changed_file)
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .to_path_buf();
    let change_type = if test_globs
        .iter()
        .any(|glob| glob.is_match(changed_file))
    {
        ChangeType::Test
    } else {
        ChangeType::Source
    };
    (dir, change_type)
}

fn process_diff_line(
    raw_line: &str,
    ctx: &DiffFilterContext<'_>,
    manifest_changed: &mut bool,
    changed_dirs: &mut IndexMap<PathBuf, ChangeType>,
) {
    let file = raw_line.strip_prefix('"').unwrap_or(raw_line);
    let file = file.strip_suffix('"').unwrap_or(file);
    if file.is_empty() {
        return;
    }
    if ctx.repo_root.join(file) == ctx.workspace_manifest {
        *manifest_changed = true;
        if ctx.working_dir != ctx.workspace_dir {
            return;
        }
    }
    if ctx.ignore_globs
        .iter()
        .any(|glob| glob.is_match(file))
    {
        return;
    }
    let (dir, change_type) = classify_changed_path(file, ctx.test_globs);
    if changed_dirs.get(&dir) != Some(&ChangeType::Source) {
        changed_dirs.insert(dir, change_type);
    }
}
fn get_changed_dirs_since_commit(
    commit: &str,
    opts: &GetChangedProjectsOptions<'_>,
    repo_root: &Path,
) -> Result<ChangedDirsResult, FilterError> {
    let working_dir = opts.working_dir.unwrap_or(opts.workspace_dir);
    let manifest_path = opts.workspace_dir.join(pnpm_workspace::WORKSPACE_MANIFEST_FILENAME);

    let stdout = git_diff_names(commit, opts.workspace_dir, working_dir, &manifest_path)?;
    let diff = strip_final_newline(&stdout);
    if diff.is_empty() {
        return Ok(ChangedDirsResult {
            changed_dirs: IndexMap::new(),
            workspace_manifest_changed: false,
        });
    }

    let ignore_globs = compile_globs(opts.changed_files_ignore_pattern)?;
    let test_globs = compile_globs(opts.test_pattern)?;
    let ctx = DiffFilterContext {
        repo_root,
        workspace_manifest: &manifest_path,
        working_dir,
        workspace_dir: opts.workspace_dir,
        ignore_globs: &ignore_globs,
        test_globs: &test_globs,
    };

    let mut workspace_manifest_changed = false;
    let mut changed_dirs: IndexMap<PathBuf, ChangeType> = IndexMap::new();
    for line in diff.split('\n') {
        process_diff_line(line, &ctx, &mut workspace_manifest_changed, &mut changed_dirs);
    }
    Ok(ChangedDirsResult { changed_dirs, workspace_manifest_changed })
}

/// The commit `commit` and `HEAD` share. Diffing against it keeps commits
/// made only on the `<since>` side out of the result. git exits with 1 when
/// there is no merge base (a shallow clone or unrelated histories) and with
/// 128 for an invalid `<since>`. Both fall back to `commit` itself, so
/// `git diff` reports the bad revision.
fn merge_base(commit: &str, workspace_dir: &Path) -> Result<String, FilterError> {
    let output = Command::new("git")
        .args(["merge-base", "--end-of-options", commit, "HEAD"])
        .current_dir(workspace_dir)
        .output()
        .map_err(|err| FilterError::FilterChanged { stderr: err.to_string() })?;
    match output.status.code() {
        Some(0) => {
            let base = String::from_utf8_lossy(&output.stdout).trim().to_string();
            Ok(if base.is_empty() { commit.to_string() } else { base })
        }
        Some(1 | 128) => Ok(commit.to_string()),
        _ => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(FilterError::FilterChanged { stderr: strip_final_newline(&stderr).to_string() })
        }
    }
}

fn git_diff_names(
    commit: &str,
    workspace_dir: &Path,
    working_dir: &Path,
    manifest_path: &Path,
) -> Result<String, FilterError> {
    let mut cmd = Command::new("git");
    cmd.args([
        "diff",
        "--name-only",
        "--no-relative",
        "--no-renames",
        "--end-of-options",
        commit,
        "--",
    ])
    .arg(working_dir);
    if working_dir != workspace_dir {
        cmd.arg(manifest_path);
    }
    cmd.current_dir(workspace_dir);

    let output = cmd
        .output()
        .map_err(|err| FilterError::FilterChanged { stderr: err.to_string() })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(FilterError::FilterChanged {
            stderr: strip_final_newline(&stderr).to_string(),
        });
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub(crate) fn strip_final_newline(text: &str) -> &str {
    let text = text.strip_suffix('\n').unwrap_or(text);
    text.strip_suffix('\r').unwrap_or(text)
}

fn compile_globs(patterns: &[String]) -> Result<Vec<Glob<'_>>, FilterError> {
    patterns
        .iter()
        .filter(|pattern| !pattern.is_empty())
        .map(|pattern| {
            Glob::new(pattern)
                .map_err(|err| FilterError::InvalidPattern {
                    pattern: pattern.clone(),
                    message: err.to_string(),
                })
        })
        .collect()
}

fn find_repo_root(start: &Path) -> PathBuf {
    let mut current = start;
    loop {
        if current.join(".git").exists() {
            return current.to_path_buf();
        }
        let Some(parent) = current.parent() else { break };
        current = parent;
    }
    start.to_path_buf()
}
