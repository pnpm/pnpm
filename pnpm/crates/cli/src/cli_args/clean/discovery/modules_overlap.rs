use super::{
    super::{
        super::recursive::discover_workspace_projects,
        removal::{
            configured_modules_leaf, configured_virtual_store_dir, modules_leaf_for_project,
        },
    },
    pattern_reachability::{literal_pattern_subpaths, patterns_entering_root_dir},
};
use miette::{Context, IntoDiagnostic};
use pnpm_config::Config;
use pnpm_fs::{is_symlink_metadata, lexical_normalize};
use pnpm_modules_yaml::{Host, ReadModulesError, read_modules_manifest};
use pnpm_workspace::{
    FindWorkspaceProjectsOpts, Project, find_workspace_projects, read_workspace_manifest,
    workspace_package_patterns,
};
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    path::{Component, Path, PathBuf},
};

pub(super) fn discover_workspace_for_clean(
    workspace_dir: &Path,
    config: &Config,
) -> miette::Result<Vec<Project>> {
    let custom_leaves = custom_relative_modules_leaves(config);
    let root_clean_dirs = custom_root_clean_dirs(workspace_dir, config);
    if custom_leaves.is_empty() && root_clean_dirs.is_empty() {
        let (projects, _) = discover_workspace_projects(workspace_dir, config)?;
        return Ok(projects);
    }
    let patterns = resolve_workspace_patterns(workspace_dir, config)?;
    let mut projects =
        initial_projects(workspace_dir, config, patterns.as_deref(), &custom_leaves)?;
    if !custom_leaves.is_empty() {
        refine_projects(&mut projects, workspace_dir, config, patterns.as_deref(), None)?;
    }
    for root_clean_dir in &root_clean_dirs {
        if let Some(recovery_patterns) = patterns_entering_root_dir(
            workspace_dir,
            root_clean_dir,
            patterns.as_deref(),
            &custom_leaves,
        ) {
            refine_projects(
                &mut projects,
                workspace_dir,
                config,
                Some(&recovery_patterns),
                Some(root_clean_dir),
            )?;
        }
    }
    Ok(projects)
}

pub(super) fn custom_root_clean_dirs(workspace_dir: &Path, config: &Config) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if config.modules_dir != workspace_dir.join("node_modules") {
        dirs.push(config.modules_dir.clone());
    }
    let virtual_store_dir = configured_virtual_store_dir(workspace_dir, config);
    if virtual_store_dir != config.modules_dir.join(".pnpm")
        && !dirs
            .iter()
            .any(|existing| lexical_normalize(existing) == lexical_normalize(&virtual_store_dir))
    {
        dirs.push(virtual_store_dir);
    }
    dirs
}

fn resolve_workspace_patterns(
    workspace_dir: &Path,
    config: &Config,
) -> miette::Result<Option<Vec<String>>> {
    match &config.workspace_package_patterns {
        Some(patterns) => Ok(Some(patterns.clone())),
        None if config.ignore_workspace => Ok(None),
        None => Ok(read_workspace_manifest(workspace_dir)
            .into_diagnostic()
            .wrap_err("reading pnpm-workspace.yaml")?
            .map(|manifest| workspace_package_patterns(&manifest))),
    }
}

fn initial_projects(
    workspace_dir: &Path,
    config: &Config,
    patterns: Option<&[String]>,
    custom_leaves: &[String],
) -> miette::Result<Vec<Project>> {
    let effective_patterns = if custom_leaves.is_empty() {
        patterns.map(<[String]>::to_vec)
    } else {
        let mut list = patterns
            .map(<[String]>::to_vec)
            .unwrap_or_else(|| vec![".".to_string(), "**".to_string()]);
        for leaf in custom_leaves {
            list.push(format!("!**/{}/**", escape_wax_path(leaf)));
        }
        Some(list)
    };
    find_workspace_projects(
        workspace_dir,
        &FindWorkspaceProjectsOpts {
            patterns: effective_patterns,
            ignored_directories: config.managed_directories(),
        },
    )
    .wrap_err("finding workspace projects")
}

fn refine_projects(
    projects: &mut Vec<Project>,
    workspace_dir: &Path,
    config: &Config,
    patterns: Option<&[String]>,
    entered_root_dir: Option<&Path>,
) -> miette::Result<()> {
    let default_leaf = configured_modules_leaf(config);
    let workspace_norm = lexical_normalize(workspace_dir);
    let entered_norm = entered_root_dir.map(lexical_normalize);
    let hoisted_ignores = match entered_root_dir {
        Some(entered) => hoisted_dirs_under_root(workspace_dir, entered)?,
        None => Vec::new(),
    };
    loop {
        let mut ignored_directories: Vec<PathBuf> = config
            .managed_directories()
            .into_iter()
            .filter(|dir| entered_norm.as_deref() != Some(lexical_normalize(dir).as_path()))
            .collect();
        ignored_directories.extend(hoisted_ignores.iter().cloned());
        ignored_directories.extend(
            non_root_owned_modules_dirs(projects, &[], &workspace_norm, config, &default_leaf)
                .into_values(),
        );
        let found = find_workspace_projects(
            workspace_dir,
            &FindWorkspaceProjectsOpts {
                patterns: patterns.map(<[String]>::to_vec),
                ignored_directories,
            },
        )
        .wrap_err("finding workspace projects")?;
        if !merge_projects_outside_owned_modules(projects, found, workspace_dir, config) {
            return Ok(());
        }
    }
}

fn hoisted_dirs_under_root(
    workspace_dir: &Path,
    root_modules_dir: &Path,
) -> miette::Result<Vec<PathBuf>> {
    let manifest = match read_modules_manifest::<Host>(root_modules_dir) {
        Ok(Some(manifest)) => manifest,
        Ok(None) => return Ok(Vec::new()),
        Err(ReadModulesError::ReadFile { source, .. })
            if source.kind() == std::io::ErrorKind::NotADirectory =>
        {
            return Ok(Vec::new());
        }
        Err(err) => return Err(err).into_diagnostic(),
    };
    let Some(locations) = manifest.hoisted_locations else {
        return Ok(Vec::new());
    };
    let root_norm = lexical_normalize(root_modules_dir);
    Ok(locations
        .into_values()
        .flatten()
        .map(|loc| lexical_normalize(&workspace_dir.join(loc)))
        .filter(|dir| dir.starts_with(&root_norm) && dir != &root_norm)
        .collect())
}

fn custom_relative_modules_leaves(config: &Config) -> Vec<String> {
    let mut leaves = BTreeSet::new();
    if let Some(posix) = relative_subpath_posix(&configured_modules_leaf(config)) {
        leaves.insert(posix);
    }
    if let Some(configs) = config.package_configs.as_ref() {
        for entry in configs.values() {
            if let Some(dir) = entry.modules_dir.as_deref()
                && let Some(posix) = relative_subpath_posix(Path::new(dir))
            {
                leaves.insert(posix);
            }
        }
    }
    leaves.into_iter().collect()
}

pub(super) fn relative_subpath_posix(raw: &Path) -> Option<String> {
    if raw.is_absolute() {
        return None;
    }
    let mut segments = Vec::new();
    for component in raw.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(seg) => segments.push(seg.to_string_lossy().into_owned()),
            Component::ParentDir => {
                segments.pop()?;
            }
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    let posix = segments.join("/");
    (!posix.is_empty()
        && !paths_match_on_host(&posix, "node_modules")
        && !paths_match_on_host(&posix, "bower_components"))
    .then_some(posix)
}

pub(super) fn escape_wax_path(posix: &str) -> String {
    posix
        .split('/')
        .map(|seg| wax::escape(seg).into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

pub(super) fn paths_match_on_host(left: &str, right: &str) -> bool {
    if cfg!(any(windows, target_os = "macos")) {
        left.eq_ignore_ascii_case(right)
    } else {
        left == right
    }
}

fn merge_projects_outside_owned_modules(
    known: &mut Vec<Project>,
    found: Vec<Project>,
    workspace_dir: &Path,
    config: &Config,
) -> bool {
    let default_leaf = configured_modules_leaf(config);
    let workspace_norm = lexical_normalize(workspace_dir);
    let known_dirs: HashSet<PathBuf> = known
        .iter()
        .map(|project| project.root_dir.clone())
        .collect();
    let owned_modules_dirs =
        non_root_owned_modules_dirs(known, &found, &workspace_norm, config, &default_leaf);
    let prev_len = known.len();
    for candidate in found {
        if !known_dirs.contains(&candidate.root_dir)
            && !has_symlink_component_under(workspace_dir, &candidate.root_dir)
            && !is_inside_owned_modules_dir(&candidate.root_dir, &owned_modules_dirs)
        {
            known.push(candidate);
        }
    }
    known.len() > prev_len
}

fn non_root_owned_modules_dirs(
    known: &[Project],
    found: &[Project],
    workspace_norm: &Path,
    config: &Config,
    default_leaf: &Path,
) -> HashMap<PathBuf, PathBuf> {
    let mut owned = HashMap::new();
    for project in known.iter().chain(found.iter()) {
        if lexical_normalize(&project.root_dir) == workspace_norm {
            continue;
        }
        let leaf = modules_leaf_for_project(config, &project.root_dir, default_leaf);
        if !leaf.is_absolute() {
            owned.insert(project.root_dir.clone(), project.root_dir.join(leaf));
        }
    }
    owned
}

fn has_symlink_component_under(workspace_dir: &Path, target_dir: &Path) -> bool {
    let Ok(rel) = target_dir.strip_prefix(workspace_dir) else {
        return false;
    };
    let mut current = workspace_dir.to_path_buf();
    for component in rel.components() {
        current.push(component);
        if std::fs::symlink_metadata(&current).is_ok_and(|meta| is_symlink_metadata(&meta)) {
            return true;
        }
    }
    false
}

fn is_inside_owned_modules_dir(dir: &Path, owned_modules_dirs: &HashMap<PathBuf, PathBuf>) -> bool {
    let mut current = dir.parent();
    while let Some(ancestor) = current {
        if owned_modules_dirs
            .get(ancestor)
            .is_some_and(|modules_dir| dir.starts_with(modules_dir) && dir != modules_dir)
        {
            return true;
        }
        current = ancestor.parent();
    }
    false
}

pub(super) fn explicit_literal_member_dirs(
    workspace_dir: &Path,
    config: &Config,
) -> miette::Result<Vec<PathBuf>> {
    let Some(patterns) = resolve_workspace_patterns(workspace_dir, config)? else {
        return Ok(Vec::new());
    };
    let mut dirs = Vec::new();
    for subpath in literal_pattern_subpaths(&patterns) {
        let candidate = workspace_dir.join(&subpath);
        if !has_symlink_component_under(workspace_dir, &candidate) && has_manifest_file(&candidate)
        {
            dirs.push(candidate);
        }
    }
    Ok(dirs)
}

fn has_manifest_file(dir: &Path) -> bool {
    ["package.json", "package.json5", "package.yaml"]
        .iter()
        .any(|name| {
            std::fs::symlink_metadata(dir.join(name))
                .is_ok_and(|meta| !is_symlink_metadata(&meta) && meta.is_file())
        })
}
