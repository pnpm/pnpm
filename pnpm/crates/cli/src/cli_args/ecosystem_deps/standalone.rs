use super::cargo_manifest_ops::cargo_project_name;
use crate::{
    cli_args::{
        pipelines::WorkspaceScope,
        recursive::{
            AutoExcludeRoot, RecursiveSelection, filter_against, recursive_filter_options,
            select_recursive_projects_deferring_no_match,
        },
    },
    ecosystem_install::{EcosystemManifest, EcosystemWorkspaceInventory, python},
};
use miette::Result;
use pnpm_config::Config;
use pnpm_workspace_projects_graph::{BaseProject, ProjectGraph, ProjectGraphNode};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::Arc,
};

pub(crate) struct StandaloneEcosystemSelection {
    pub(crate) discovered: usize,
    pub(crate) selected: Vec<PathBuf>,
}

struct StandaloneCargoProject {
    root: PathBuf,
    name: Option<String>,
}

impl BaseProject for StandaloneCargoProject {
    fn root_dir(&self) -> &Path {
        &self.root
    }

    fn manifest_name(&self) -> Option<&str> {
        self.name.as_deref()
    }
}

struct CargoWorkspaceIndex {
    by_norm_dir: HashMap<PathBuf, PathBuf>,
    by_name: HashMap<String, PathBuf>,
}

pub(crate) async fn discover_standalone_ecosystem_dirs(
    workspace_root: &Path,
    existing_dirs: &HashSet<PathBuf>,
    config: &Config,
) -> Result<Vec<PathBuf>> {
    if !config.cargo.enabled && !config.python.enabled {
        return Ok(Vec::new());
    }
    let inventory = EcosystemWorkspaceInventory::new(workspace_root.to_path_buf(), config);
    let mut dirs = Vec::new();
    let mut seen = existing_dirs.clone();
    if config.cargo.enabled {
        extend_manifest_dirs(
            inventory.manifests(EcosystemManifest::Cargo).await?,
            &mut seen,
            &mut dirs,
        );
    }
    if config.python.enabled {
        extend_manifest_dirs(
            inventory.manifests(EcosystemManifest::Python).await?,
            &mut seen,
            &mut dirs,
        );
    }
    Ok(dirs)
}

pub(crate) async fn select_recursive_and_standalone_projects<'a>(
    workspace_root: &Path,
    prefix: &Path,
    projects: &'a [pnpm_workspace::Project],
    config: &'static Config,
) -> Result<(RecursiveSelection<'a>, Vec<PathBuf>)> {
    let (selection, unmatched) = select_recursive_projects_deferring_no_match(
        projects,
        config,
        prefix,
        AutoExcludeRoot::Disabled,
    )?;
    let npm_dirs: HashSet<PathBuf> = projects
        .iter()
        .map(|proj| proj.root_dir.clone())
        .collect();
    let selected_npm_dirs: HashSet<PathBuf> = selection.selected
        .keys()
        .cloned()
        .collect();
    let standalone = select_standalone_ecosystem_dirs(
        workspace_root,
        prefix,
        &npm_dirs,
        &selected_npm_dirs,
        config,
    )
    .await?;
    if let Some(unmatched) = unmatched
        && standalone.selected.is_empty()
    {
        return Err(unmatched.counting(standalone.discovered).report());
    }
    Ok((selection, standalone.selected))
}

pub(crate) async fn select_standalone_ecosystem_dirs(
    workspace_root: &Path,
    prefix: &Path,
    npm_project_dirs: &HashSet<PathBuf>,
    selected_npm_dirs: &HashSet<PathBuf>,
    config: &'static Config,
) -> Result<StandaloneEcosystemSelection> {
    if !config.cargo.enabled && !config.python.enabled {
        return Ok(StandaloneEcosystemSelection { discovered: 0, selected: Vec::new() });
    }
    if config.filter.is_empty() && config.filter_prod.is_empty() {
        let selected =
            discover_standalone_ecosystem_dirs(workspace_root, npm_project_dirs, config).await?;
        return Ok(StandaloneEcosystemSelection { discovered: selected.len(), selected });
    }
    let inventory = EcosystemWorkspaceInventory::new(workspace_root.to_path_buf(), config);
    let mut seen = npm_project_dirs.clone();
    let mut selected = Vec::new();
    let mut discovered = 0;
    if config.cargo.enabled {
        let (cargo_discovered, cargo_selected) = select_standalone_cargo_dirs(
            inventory.manifests(EcosystemManifest::Cargo).await?,
            npm_project_dirs,
            config,
            prefix,
        )?;
        discovered += cargo_discovered;
        extend_unique_dirs(cargo_selected, &mut seen, &mut selected);
    }
    if config.python.enabled {
        let (py_discovered, py_selected) = select_standalone_python_dirs(
            &inventory,
            npm_project_dirs,
            selected_npm_dirs,
            config,
            prefix,
        )
        .await?;
        discovered += py_discovered;
        extend_unique_dirs(py_selected, &mut seen, &mut selected);
    }
    Ok(StandaloneEcosystemSelection { discovered, selected })
}

fn select_standalone_cargo_dirs(
    manifests: &[PathBuf],
    npm_project_dirs: &HashSet<PathBuf>,
    config: &Config,
    prefix: &Path,
) -> Result<(usize, Vec<PathBuf>)> {
    let mut dirs = Vec::new();
    let mut seen = HashSet::new();
    extend_manifest_dirs(manifests, &mut seen, &mut dirs);
    let discovered = dirs
        .iter()
        .filter(|dir| !npm_project_dirs.contains(*dir))
        .count();
    if discovered == 0 {
        return Ok((0, Vec::new()));
    }
    let entries: Vec<(PathBuf, Option<String>)> = dirs
        .iter()
        .map(|dir| (dir.clone(), cargo_project_name(dir)))
        .collect();
    let index = build_cargo_index(&entries);
    let all_graph = build_cargo_graph(&entries, true, &index);
    let prod_graph = build_cargo_graph(&entries, false, &index);
    let matched = filter_standalone_graph(&all_graph, &prod_graph, config, prefix)?;
    let selected = dirs
        .into_iter()
        .filter(|dir| !npm_project_dirs.contains(dir) && matched.contains(dir))
        .collect();
    Ok((discovered, selected))
}

fn build_cargo_index(entries: &[(PathBuf, Option<String>)]) -> CargoWorkspaceIndex {
    let mut by_norm_dir = HashMap::new();
    let mut by_name = HashMap::new();
    for (dir, name) in entries {
        by_norm_dir.insert(pnpm_fs::lexical_normalize(dir), dir.clone());
        if let Some(pkg_name) = name {
            by_name.insert(pkg_name.clone(), dir.clone());
        }
    }
    CargoWorkspaceIndex { by_norm_dir, by_name }
}

fn build_cargo_graph(
    entries: &[(PathBuf, Option<String>)],
    include_dev: bool,
    index: &CargoWorkspaceIndex,
) -> ProjectGraph<StandaloneCargoProject> {
    entries
        .iter()
        .map(|(dir, name)| {
            let package = StandaloneCargoProject { root: dir.clone(), name: name.clone() };
            let dependencies = cargo_project_dependencies(dir, include_dev, index);
            (dir.clone(), ProjectGraphNode { package, dependencies })
        })
        .collect()
}

fn cargo_project_dependencies(
    dir: &Path,
    include_dev: bool,
    index: &CargoWorkspaceIndex,
) -> Vec<PathBuf> {
    let Ok(contents) = fs::read_to_string(dir.join("Cargo.toml")) else {
        return Vec::new();
    };
    let Ok(doc) = toml::from_str::<toml::Table>(&contents) else {
        return Vec::new();
    };
    let mut deps = Vec::new();
    collect_doc_cargo_edges(&doc, dir, include_dev, index, &mut deps);
    deps
}

fn collect_doc_cargo_edges(
    doc: &toml::Table,
    dir: &Path,
    include_dev: bool,
    index: &CargoWorkspaceIndex,
    out: &mut Vec<PathBuf>,
) {
    let sections: &[&str] = if include_dev {
        &["dependencies", "dev-dependencies", "build-dependencies"]
    } else {
        &["dependencies", "build-dependencies"]
    };
    for &section in sections {
        if let Some(table) = doc.get(section).and_then(toml::Value::as_table) {
            collect_table_cargo_edges(table, dir, index, out);
        }
    }
    let Some(targets) = doc.get("target").and_then(toml::Value::as_table) else {
        return;
    };
    for target_cfg in targets.values().filter_map(toml::Value::as_table) {
        collect_doc_cargo_edges(target_cfg, dir, include_dev, index, out);
    }
}

fn collect_table_cargo_edges(
    table: &toml::Table,
    dir: &Path,
    index: &CargoWorkspaceIndex,
    out: &mut Vec<PathBuf>,
) {
    for (key, value) in table {
        let Some(dep_table) = value.as_table() else {
            continue;
        };
        if let Some(target_dir) = resolve_cargo_edge_target(key, dep_table, dir, index)
            && target_dir != dir
            && !out.contains(&target_dir)
        {
            out.push(target_dir);
        }
    }
}

fn resolve_cargo_edge_target(
    key: &str,
    dep_table: &toml::Table,
    dir: &Path,
    index: &CargoWorkspaceIndex,
) -> Option<PathBuf> {
    if let Some(rel_path) = dep_table.get("path").and_then(toml::Value::as_str) {
        let norm = pnpm_fs::lexical_normalize(&dir.join(rel_path));
        return index.by_norm_dir.get(&norm).cloned();
    }
    if dep_table.get("workspace").and_then(toml::Value::as_bool) == Some(true) {
        let pkg_name = dep_table
            .get("package")
            .and_then(toml::Value::as_str)
            .unwrap_or(key);
        return index.by_name.get(pkg_name).cloned();
    }
    None
}

fn filter_standalone_graph<Pkg: BaseProject>(
    all_graph: &ProjectGraph<Pkg>,
    prod_graph: &ProjectGraph<Pkg>,
    config: &Config,
    prefix: &Path,
) -> Result<HashSet<PathBuf>> {
    let root_selector = AutoExcludeRoot::Disabled.root_selector(config, prefix);
    let prod = !config.filter_prod.is_empty();
    let options = recursive_filter_options(config, prefix);
    let mut selected: HashSet<PathBuf> = filter_against(
        all_graph,
        &config.filter,
        root_selector.as_deref().filter(|_| !prod),
        false,
        prefix,
        &options,
    )?
    .into_iter()
    .collect();
    if prod {
        selected.extend(filter_against(
            prod_graph,
            &config.filter_prod,
            root_selector.as_deref().filter(|_| prod),
            true,
            prefix,
            &options,
        )?);
    }
    Ok(selected)
}

async fn select_standalone_python_dirs(
    inventory: &EcosystemWorkspaceInventory,
    npm_project_dirs: &HashSet<PathBuf>,
    selected_npm_dirs: &HashSet<PathBuf>,
    config: &'static Config,
    prefix: &Path,
) -> Result<(usize, Vec<PathBuf>)> {
    let discovery = python::discover(config, inventory).await?;
    let discovered = discovery
        .project_roots()
        .filter(|root| !npm_project_dirs.contains(*root))
        .count();
    let scope = WorkspaceScope {
        projects: Arc::new(npm_project_dirs.clone()),
        selected: Arc::new(selected_npm_dirs.clone()),
        root_selector: AutoExcludeRoot::Disabled.root_selector(config, prefix),
    };
    let selected = python::selected_projects(config, prefix, &discovery, Some(&scope))?
        .into_iter()
        .filter(|dir| !npm_project_dirs.contains(dir))
        .collect();
    Ok((discovered, selected))
}

fn extend_manifest_dirs(
    manifests: &[PathBuf],
    seen: &mut HashSet<PathBuf>,
    dirs: &mut Vec<PathBuf>,
) {
    for manifest in manifests {
        if let Some(dir) = manifest.parent()
            && seen.insert(dir.to_path_buf())
        {
            dirs.push(dir.to_path_buf());
        }
    }
}

fn extend_unique_dirs(
    candidates: Vec<PathBuf>,
    seen: &mut HashSet<PathBuf>,
    out: &mut Vec<PathBuf>,
) {
    for dir in candidates {
        if seen.insert(dir.clone()) {
            out.push(dir);
        }
    }
}
