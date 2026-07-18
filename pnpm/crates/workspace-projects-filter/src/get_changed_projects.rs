//! Resolve a `[<since>]` changed-packages selector to the workspace
//! projects the git diff touches, upstream's `getChangedProjects`.

use crate::filter::FilterError;
use indexmap::{IndexMap, IndexSet};
use pacquet_catalogs_protocol_parser::parse_catalog_protocol;
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use wax::{Glob, Program};

/// Options for [`get_changed_projects`].
pub struct GetChangedProjectsOptions<'a> {
    /// Directory the `git diff` runs in and is path-restricted to: the
    /// selector's `{dir}` part when present, else the workspace root.
    pub workspace_dir: &'a Path,
    pub workspace_root: &'a Path,
    pub catalog_users: &'a CatalogUsers,
    pub test_pattern: &'a [String],
    pub changed_files_ignore_pattern: &'a [String],
}

pub type CatalogUsers = BTreeMap<(String, String), IndexSet<PathBuf>>;

pub fn collect_catalog_users(
    projects: impl IntoIterator<Item = (PathBuf, Vec<(String, String)>)>,
) -> CatalogUsers {
    let mut users = CatalogUsers::new();
    for (project_dir, dependencies) in projects {
        for (dependency_name, specifier) in dependencies {
            let Some(catalog_name) = parse_catalog_protocol(&specifier) else {
                continue;
            };
            users
                .entry((catalog_name.to_string(), dependency_name))
                .or_default()
                .insert(project_dir.clone());
        }
    }
    users
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
/// the projects whose only changes are test files. Both lists keep the
/// `project_dirs` order. A changed file belongs to the nearest
/// enclosing project directory; a project's change type is `source` as
/// soon as any of its changed files is a source change.
pub fn get_changed_projects(
    project_dirs: Vec<PathBuf>,
    commit: &str,
    opts: &GetChangedProjectsOptions<'_>,
) -> Result<ChangedProjects, FilterError> {
    let repo_root = find_repo_root(opts.workspace_dir);
    let changed_dirs = get_changed_dirs_since_commit(commit, opts)?;

    let mut project_change_types: IndexMap<PathBuf, Option<ChangeType>> =
        project_dirs.into_iter().map(|dir| (dir, None)).collect();
    for (changed_dir, change_type) in changed_dirs {
        let mut current = if changed_dir.as_os_str().is_empty() {
            repo_root.clone()
        } else {
            repo_root.join(&changed_dir)
        };
        while !project_change_types.contains_key(&current) {
            let Some(parent) = current.parent() else { break };
            current = parent.to_path_buf();
        }
        let entry = project_change_types.entry(current).or_insert(None);
        if *entry != Some(ChangeType::Source) {
            *entry = Some(change_type);
        }
    }
    for project_dir in projects_using_changed_catalog_entries(commit, &repo_root, opts)? {
        project_change_types.insert(project_dir, Some(ChangeType::Source));
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

#[derive(Default, Deserialize)]
struct CatalogManifest {
    #[serde(default)]
    catalog: Option<BTreeMap<String, String>>,
    #[serde(default)]
    catalogs: Option<BTreeMap<String, BTreeMap<String, String>>>,
}

fn projects_using_changed_catalog_entries(
    commit: &str,
    repo_root: &Path,
    opts: &GetChangedProjectsOptions<'_>,
) -> Result<IndexSet<PathBuf>, FilterError> {
    let manifest_path = opts.workspace_root.join("pnpm-workspace.yaml");
    let relative_manifest_path = manifest_path.strip_prefix(repo_root).unwrap_or(&manifest_path);
    let before = read_catalogs_at_commit(repo_root, commit, relative_manifest_path);
    let after = match fs::read_to_string(&manifest_path) {
        Ok(source) => parse_catalogs(&source)?,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
        Err(err) => {
            return Err(FilterError::FilterChanged {
                stderr: format!("Failed to read {}: {err}", manifest_path.display()),
            });
        }
    };
    let changed = changed_catalog_entries(&before, &after);
    let mut projects = IndexSet::new();
    for key in changed {
        if let Some(users) = opts.catalog_users.get(&key) {
            projects.extend(users.iter().cloned());
        }
    }
    Ok(projects)
}

fn read_catalogs_at_commit(
    repo_root: &Path,
    commit: &str,
    relative_manifest_path: &Path,
) -> BTreeMap<String, BTreeMap<String, String>> {
    let revision = format!("{commit}:{}", relative_manifest_path.to_string_lossy());
    let Ok(output) = Command::new("git").args(["show", &revision]).current_dir(repo_root).output()
    else {
        return BTreeMap::new();
    };
    if !output.status.success() {
        return BTreeMap::new();
    }
    parse_catalogs(&String::from_utf8_lossy(&output.stdout)).unwrap_or_default()
}

fn parse_catalogs(source: &str) -> Result<BTreeMap<String, BTreeMap<String, String>>, FilterError> {
    let manifest: CatalogManifest = serde_saphyr::from_str(source).map_err(|err| {
        FilterError::FilterChanged { stderr: format!("Failed to parse pnpm-workspace.yaml: {err}") }
    })?;
    let mut catalogs = manifest.catalogs.unwrap_or_default();
    if let Some(default) = manifest.catalog {
        catalogs.insert("default".to_string(), default);
    }
    Ok(catalogs)
}

fn changed_catalog_entries(
    before: &BTreeMap<String, BTreeMap<String, String>>,
    after: &BTreeMap<String, BTreeMap<String, String>>,
) -> BTreeSet<(String, String)> {
    let mut changed = BTreeSet::new();
    for catalog_name in before.keys().chain(after.keys()) {
        let before_catalog = before.get(catalog_name);
        let after_catalog = after.get(catalog_name);
        let dependency_names = before_catalog
            .into_iter()
            .flat_map(|catalog| catalog.keys())
            .chain(after_catalog.into_iter().flat_map(|catalog| catalog.keys()));
        for dependency_name in dependency_names {
            if before_catalog.and_then(|catalog| catalog.get(dependency_name))
                != after_catalog.and_then(|catalog| catalog.get(dependency_name))
            {
                changed.insert((catalog_name.clone(), dependency_name.clone()));
            }
        }
    }
    changed
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChangeType {
    Source,
    Test,
}

/// Run `git diff --name-only <commit> -- <workspace_dir>` and bucket
/// the changed files' directories (relative to the repository root) by
/// change type. `source` is sticky: once a directory has a source
/// change, later test changes don't downgrade it.
fn get_changed_dirs_since_commit(
    commit: &str,
    opts: &GetChangedProjectsOptions<'_>,
) -> Result<IndexMap<PathBuf, ChangeType>, FilterError> {
    // `--end-of-options` keeps an option-like `<since>` (`--output=...`)
    // from being parsed as a git option — git rejects it as a bad
    // revision instead.
    let output = Command::new("git")
        .args(["diff", "--name-only", "--end-of-options", commit, "--"])
        .arg(opts.workspace_dir)
        .current_dir(opts.workspace_dir)
        .output()
        .map_err(|err| FilterError::FilterChanged { stderr: err.to_string() })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(FilterError::FilterChanged {
            stderr: strip_final_newline(&stderr).to_string(),
        });
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let diff = strip_final_newline(&stdout);
    if diff.is_empty() {
        return Ok(IndexMap::new());
    }

    let ignore_globs = compile_globs(opts.changed_files_ignore_pattern)?;
    let test_globs = compile_globs(opts.test_pattern)?;

    let mut changed_dirs: IndexMap<PathBuf, ChangeType> = IndexMap::new();
    for line in diff.split('\n') {
        // git wraps paths with non-ASCII characters in quotes.
        let changed_file = line.strip_prefix('"').unwrap_or(line);
        let changed_file = changed_file.strip_suffix('"').unwrap_or(changed_file);
        if ignore_globs.iter().any(|glob| glob.is_match(changed_file)) {
            continue;
        }
        let dir = Path::new(changed_file).parent().unwrap_or_else(|| Path::new("")).to_path_buf();
        if changed_dirs.get(&dir) == Some(&ChangeType::Source) {
            continue;
        }
        let change_type = if test_globs.iter().any(|glob| glob.is_match(changed_file)) {
            ChangeType::Test
        } else {
            ChangeType::Source
        };
        changed_dirs.insert(dir, change_type);
    }
    Ok(changed_dirs)
}

/// Strip one final `\n` (and a preceding `\r`, if any) — execa's
/// `stripFinalNewline` behavior, which upstream's process output goes
/// through before it is parsed or embedded in an error message.
fn strip_final_newline(text: &str) -> &str {
    let text = text.strip_suffix('\n').unwrap_or(text);
    text.strip_suffix('\r').unwrap_or(text)
}

fn compile_globs(patterns: &[String]) -> Result<Vec<Glob<'_>>, FilterError> {
    patterns
        .iter()
        .filter(|pattern| !pattern.is_empty())
        .map(|pattern| {
            Glob::new(pattern).map_err(|err| FilterError::InvalidPattern {
                pattern: pattern.clone(),
                message: err.to_string(),
            })
        })
        .collect()
}

/// The directory git-diff paths are relative to: the parent of the
/// nearest `.git` entry up from `workspace_dir` — a directory in
/// regular repositories, a file in worktrees — else the parent of
/// `workspace_dir` itself. The nearest entry of *either* kind wins, so
/// a worktree checked out inside another repository's tree resolves to
/// the worktree root, matching where git anchors its diff paths.
fn find_repo_root(workspace_dir: &Path) -> PathBuf {
    let git_path =
        workspace_dir.ancestors().map(|dir| dir.join(".git")).find(|candidate| candidate.exists());
    match git_path {
        Some(git_path) => git_path.parent().expect("a `.git` path has a parent").to_path_buf(),
        None => workspace_dir.parent().unwrap_or(workspace_dir).to_path_buf(),
    }
}
