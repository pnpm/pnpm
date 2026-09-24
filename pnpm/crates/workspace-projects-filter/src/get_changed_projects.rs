//! Resolve a `[<since>]` changed-packages selector to the workspace
//! projects the git diff touches, upstream's `getChangedProjects`.

use crate::filter::FilterError;
use indexmap::IndexMap;
use pnpm_catalogs_types::Catalogs;
use std::{
    collections::{HashMap, HashSet},
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
    pub test_pattern: &'a [String],
    pub changed_files_ignore_pattern: &'a [String],
    pub project_dependencies: Option<&'a HashMap<PathBuf, Vec<(String, String)>>>,
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
    let ChangedDirsResult {
        changed_dirs,
        workspace_manifest_changed,
    } = get_changed_dirs_since_commit(commit, opts, &repo_root)?;

    let mut project_change_types: IndexMap<PathBuf, Option<ChangeType>> = project_dirs
        .into_iter()
        .map(|dir| (dir, None))
        .collect();
    for (changed_dir, change_type) in changed_dirs {
        let owner = owning_project(&repo_root, &changed_dir, &project_change_types);
        let entry = project_change_types.entry(owner).or_insert(None);
        // `source` is sticky: a later test change never downgrades it.
        if *entry != Some(ChangeType::Source) {
            *entry = Some(change_type);
        }
    }

    if workspace_manifest_changed {
        apply_changed_catalogs(&mut project_change_types, commit, opts, &repo_root);
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

fn apply_changed_catalogs(
    project_change_types: &mut IndexMap<PathBuf, Option<ChangeType>>,
    commit: &str,
    opts: &GetChangedProjectsOptions<'_>,
    repo_root: &Path,
) {
    let changed_catalogs = detect_changed_catalogs(commit, opts.workspace_dir, repo_root);
    if changed_catalogs.is_empty() {
        return;
    }
    for (project_dir, change_type) in project_change_types {
        if *change_type == Some(ChangeType::Source) {
            continue;
        }
        let deps = opts.project_dependencies
            .and_then(|map| map.get(project_dir).cloned())
            .unwrap_or_else(|| read_manifest_dependencies(project_dir));
        if project_uses_changed_catalogs(&deps, &changed_catalogs) {
            *change_type = Some(ChangeType::Source);
        }
    }
}

/// The project a changed directory belongs to: itself if it is one, else the
/// nearest ancestor that is. A path under no project climbs to the filesystem
/// root, which owns nothing.
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
enum ChangeType {
    Source,
    Test,
}

struct ChangedDirsResult {
    changed_dirs: IndexMap<PathBuf, ChangeType>,
    workspace_manifest_changed: bool,
}

fn classify_changed_path(changed_file: &str, test_globs: &[Glob]) -> (PathBuf, ChangeType) {
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

/// Run `git diff --name-only <commit> -- <workspace_dir>` and bucket
/// the changed files' directories (relative to the repository root) by
/// change type. `source` is sticky: once a directory has a source
/// change, later test changes don't downgrade it.
fn get_changed_dirs_since_commit(
    commit: &str,
    opts: &GetChangedProjectsOptions<'_>,
    repo_root: &Path,
) -> Result<ChangedDirsResult, FilterError> {
    let stdout = git_diff_names(commit, opts.workspace_dir)?;
    let diff = strip_final_newline(&stdout);
    if diff.is_empty() {
        return Ok(ChangedDirsResult {
            changed_dirs: IndexMap::new(),
            workspace_manifest_changed: false,
        });
    }

    let ignore_globs = compile_globs(opts.changed_files_ignore_pattern)?;
    let test_globs = compile_globs(opts.test_pattern)?;
    let workspace_manifest_path =
        opts.workspace_dir.join(pnpm_workspace::WORKSPACE_MANIFEST_FILENAME);
    let mut workspace_manifest_changed = false;
    let mut changed_dirs: IndexMap<PathBuf, ChangeType> = IndexMap::new();

    for line in diff.split('\n') {
        let changed_file = line.strip_prefix('"').unwrap_or(line);
        let changed_file = changed_file.strip_suffix('"').unwrap_or(changed_file);
        if ignore_globs
            .iter()
            .any(|glob| glob.is_match(changed_file))
        {
            continue;
        }
        if repo_root.join(changed_file) == workspace_manifest_path {
            workspace_manifest_changed = true;
        }
        let (dir, change_type) = classify_changed_path(changed_file, &test_globs);
        if changed_dirs.get(&dir) != Some(&ChangeType::Source) {
            changed_dirs.insert(dir, change_type);
        }
    }
    Ok(ChangedDirsResult { changed_dirs, workspace_manifest_changed })
}

fn read_git_file(commit: &str, workspace_dir: &Path, rel_path: &Path) -> Option<String> {
    let rel_str = rel_path.to_string_lossy().replace('\\', "/");
    let output = Command::new("git")
        .args(["show", "--end-of-options", &format!("{commit}:{rel_str}")])
        .current_dir(workspace_dir)
        .output()
        .ok()?;
    output.status
        .success()
        .then(|| String::from_utf8(output.stdout).ok())?
}

fn load_workspace_catalogs(content: Option<&str>) -> Catalogs {
    use pnpm_catalogs_config::get_catalogs_from_workspace_manifest;
    use pnpm_workspace::WorkspaceManifest;

    let manifest: Option<WorkspaceManifest> =
        content.and_then(|text| serde_saphyr::from_str(text).ok());
    get_catalogs_from_workspace_manifest(manifest.as_ref()).unwrap_or_default()
}

fn diff_catalogs(
    prev_catalogs: &Catalogs,
    curr_catalogs: &Catalogs,
) -> HashMap<String, HashSet<String>> {
    let mut changed_catalogs: HashMap<String, HashSet<String>> = HashMap::new();
    let all_names: HashSet<&str> = prev_catalogs
        .keys()
        .chain(curr_catalogs.keys())
        .map(String::as_str)
        .collect();

    for catalog_name in all_names {
        let prev = prev_catalogs.get(catalog_name);
        let curr = curr_catalogs.get(catalog_name);
        let mut all_deps: HashSet<&str> = HashSet::new();
        if let Some(c) = prev {
            all_deps.extend(c.keys().map(String::as_str));
        }
        if let Some(c) = curr {
            all_deps.extend(c.keys().map(String::as_str));
        }

        for dep_name in all_deps {
            if prev.and_then(|c| c.get(dep_name)) != curr.and_then(|c| c.get(dep_name)) {
                changed_catalogs
                    .entry(catalog_name.to_string())
                    .or_default()
                    .insert(dep_name.to_string());
            }
        }
    }

    changed_catalogs
}

fn detect_changed_catalogs(
    commit: &str,
    workspace_dir: &Path,
    repo_root: &Path,
) -> HashMap<String, HashSet<String>> {
    use pnpm_workspace::WORKSPACE_MANIFEST_FILENAME;

    let manifest_path = workspace_dir.join(WORKSPACE_MANIFEST_FILENAME);
    let rel_manifest_path = pathdiff::diff_paths(&manifest_path, repo_root)
        .unwrap_or_else(|| PathBuf::from(WORKSPACE_MANIFEST_FILENAME));

    let prev_content = read_git_file(commit, workspace_dir, &rel_manifest_path);
    let curr_content = fs::read_to_string(&manifest_path).ok();

    let prev_catalogs = load_workspace_catalogs(prev_content.as_deref());
    let curr_catalogs = load_workspace_catalogs(curr_content.as_deref());

    diff_catalogs(&prev_catalogs, &curr_catalogs)
}

fn parse_catalog_dep<'a>(dep_name: &'a str, specifier: &'a str) -> (Option<&'a str>, &'a str) {
    use pnpm_catalogs_protocol_parser::parse_catalog_protocol;

    if let Some(catalog_name) = parse_catalog_protocol(specifier) {
        return (Some(catalog_name), dep_name);
    }
    if let Some(rest) = specifier.strip_prefix("npm:")
        && let Some(last_at) = rest.rfind('@')
        && last_at > 0
    {
        let sub_spec = &rest[last_at + 1..];
        if let Some(catalog_name) = parse_catalog_protocol(sub_spec) {
            let lookup_name = &rest[..last_at];
            return (Some(catalog_name), lookup_name);
        }
    }
    (None, dep_name)
}

fn project_uses_changed_catalogs(
    dependencies: &[(String, String)],
    changed_catalogs: &HashMap<String, HashSet<String>>,
) -> bool {
    for (dep_name, specifier) in dependencies {
        let (catalog_name, lookup_name) = parse_catalog_dep(dep_name, specifier);
        if let Some(catalog_name) = catalog_name
            && changed_catalogs
                .get(catalog_name)
                .is_some_and(|deps| deps.contains(lookup_name))
        {
            return true;
        }
    }
    false
}

fn read_manifest_dependencies(project_dir: &Path) -> Vec<(String, String)> {
    let manifest_path = project_dir.join("package.json");
    let Ok(content) = fs::read_to_string(manifest_path) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) else {
        return Vec::new();
    };
    let mut deps: IndexMap<String, String> = IndexMap::new();
    for field in ["peerDependencies", "devDependencies", "optionalDependencies", "dependencies"] {
        collect_manifest_field_deps(&value, field, &mut deps);
    }
    deps.into_iter().collect()
}

fn collect_manifest_field_deps(
    manifest: &serde_json::Value,
    field: &str,
    deps: &mut IndexMap<String, String>,
) {
    let Some(map) = manifest.get(field).and_then(|v| v.as_object()) else {
        return;
    };
    for (k, v) in map {
        if let Some(spec) = v.as_str() {
            deps.insert(k.clone(), spec.to_string());
        }
    }
}

/// The paths `git diff --name-only <commit>` lists under `workspace_dir`.
fn git_diff_names(commit: &str, workspace_dir: &Path) -> Result<String, FilterError> {
    // `--end-of-options` keeps an option-like `<since>` (`--output=...`)
    // from being parsed as a git option — git rejects it as a bad
    // revision instead.
    let output = Command::new("git")
        .args(["diff", "--name-only", "--end-of-options", commit, "--"])
        .arg(workspace_dir)
        .current_dir(workspace_dir)
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
            Glob::new(pattern)
                .map_err(|err| FilterError::InvalidPattern {
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
    let git_path = workspace_dir
        .ancestors()
        .map(|dir| dir.join(".git"))
        .find(|candidate| candidate.exists());
    match git_path {
        Some(git_path) => git_path
            .parent()
            .expect("a `.git` path has a parent")
            .to_path_buf(),
        None => workspace_dir
            .parent()
            .unwrap_or(workspace_dir)
            .to_path_buf(),
    }
}
