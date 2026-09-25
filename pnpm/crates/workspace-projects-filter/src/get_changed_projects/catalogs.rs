use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use indexmap::IndexMap;
use pnpm_catalogs_types::Catalogs;

use crate::{
    filter::FilterError,
    get_changed_projects::{ChangeType, GetChangedProjectsOptions, strip_final_newline},
};

pub fn apply_changed_catalogs(
    project_change_types: &mut IndexMap<PathBuf, Option<ChangeType>>,
    commit: &str,
    opts: &GetChangedProjectsOptions<'_>,
    repo_root: &Path,
) -> Result<(), FilterError> {
    let changed_catalogs = detect_changed_catalogs(commit, opts.workspace_dir, repo_root)?;
    if changed_catalogs.is_empty() {
        return Ok(());
    }
    let working_dir = opts.working_dir.unwrap_or(opts.workspace_dir);
    for (project_dir, change_type) in project_change_types {
        if *change_type == Some(ChangeType::Source) {
            continue;
        }
        if working_dir != opts.workspace_dir
            && !is_project_in_working_dir(working_dir, project_dir, opts.use_glob_dir_filtering)
        {
            continue;
        }
        let deps = opts.project_dependencies
            .and_then(|map| map.get(project_dir).cloned())
            .unwrap_or_else(|| read_manifest_dependencies(project_dir));
        if project_uses_changed_catalogs(&deps, &changed_catalogs) {
            *change_type = Some(ChangeType::Source);
        }
    }
    Ok(())
}

fn is_project_in_working_dir(working_dir: &Path, project_dir: &Path, use_glob: bool) -> bool {
    if project_dir == working_dir || project_dir.starts_with(working_dir) {
        return true;
    }
    if !use_glob {
        return false;
    }
    let working_str = working_dir.to_string_lossy();
    let dir_glob = crate::glob::DirGlob::new(&working_str);
    if dir_glob.is_match(&project_dir.to_string_lossy()) {
        return true;
    }
    if working_str.ends_with("/*") || working_str.ends_with(r"\*") {
        let recursive = format!("{working_str}*");
        let recursive_glob = crate::glob::DirGlob::new(&recursive);
        return recursive_glob.is_match(&project_dir.to_string_lossy());
    }
    false
}

fn detect_changed_catalogs(
    commit: &str,
    workspace_dir: &Path,
    repo_root: &Path,
) -> Result<HashMap<String, HashSet<String>>, FilterError> {
    use pnpm_workspace::WORKSPACE_MANIFEST_FILENAME;

    let manifest_path = workspace_dir.join(WORKSPACE_MANIFEST_FILENAME);
    let rel_manifest_path = pathdiff::diff_paths(&manifest_path, repo_root)
        .unwrap_or_else(|| PathBuf::from(WORKSPACE_MANIFEST_FILENAME));

    let prev_content = read_git_file(commit, workspace_dir, &rel_manifest_path)?;
    let curr_content = match fs::read_to_string(&manifest_path) {
        Ok(c) => Some(c),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => return Err(FilterError::FilterChanged { stderr: err.to_string() }),
    };

    let prev_catalogs = load_workspace_catalogs(prev_content.as_deref())?;
    let curr_catalogs = load_workspace_catalogs(curr_content.as_deref())?;

    Ok(diff_catalogs(&prev_catalogs, &curr_catalogs))
}

fn read_git_file(
    commit: &str,
    workspace_dir: &Path,
    rel_path: &Path,
) -> Result<Option<String>, FilterError> {
    let rel_str = rel_path.to_string_lossy().replace('\\', "/");
    let output = Command::new("git")
        .args(["show", "--end-of-options", &format!("{commit}:{rel_str}")])
        .current_dir(workspace_dir)
        .env("LC_ALL", "C")
        .output()
        .map_err(|err| FilterError::FilterChanged { stderr: err.to_string() })?;
    if output.status.success() {
        let stdout = String::from_utf8(output.stdout)
            .map_err(|err| FilterError::FilterChanged { stderr: err.to_string() })?;
        Ok(Some(stdout))
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("does not exist in") || stderr.contains("exists on disk, but not in") {
            Ok(None)
        } else {
            Err(FilterError::FilterChanged { stderr: strip_final_newline(&stderr).to_string() })
        }
    }
}

fn load_workspace_catalogs(content: Option<&str>) -> Result<Catalogs, FilterError> {
    use pnpm_catalogs_config::get_catalogs_from_workspace_manifest;
    use pnpm_workspace::WorkspaceManifest;

    let Some(text) = content else {
        return Ok(Catalogs::default());
    };
    let manifest: WorkspaceManifest = serde_saphyr::from_str(text)
        .map_err(|err| FilterError::FilterChanged { stderr: err.to_string() })?;
    get_catalogs_from_workspace_manifest(Some(&manifest))
        .map_err(|err| FilterError::FilterChanged { stderr: err.to_string() })
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
