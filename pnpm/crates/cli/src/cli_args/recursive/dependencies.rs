//! The workspace-project-graph walks the recursive commands share: the
//! dependency edges among the `--filter`-selected projects, their topological
//! order, and the workspace dependencies a selection reaches.

use super::{RecursiveSelection, filter_against, recursive_filter_options};
use crate::cli_args::{
    catalogs::{configured_catalogs, workspace_catalogs},
    verify_deps::with_dependencies,
};
use indexmap::IndexMap;
use pnpm_catalogs_resolver::{
    CatalogAnchor, CatalogResolutionResult, WantedDependency, resolve_from_catalog,
};
use pnpm_config::Config;
use pnpm_fs::lexical_normalize;
use pnpm_package_manager::{GraphSequencerResult, graph_sequencer};
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_workspace_projects_graph::{ProjectGraph, WorkspaceCatalogs};
use rayon::prelude::*;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

/// The dependency edges among the `--filter`-selected projects, resolved
/// through the full workspace graph so a relationship between two selected
/// projects via an unselected one becomes a direct edge. Keys keep the
/// selection order.
pub fn filtered_projects_dependencies<Pkg: Sync>(
    selected: &ProjectGraph<Pkg>,
    all: &ProjectGraph<Pkg>,
    prod_all: Option<&ProjectGraph<Pkg>>,
    prod_only_selected: &HashSet<PathBuf>,
) -> IndexMap<PathBuf, Vec<PathBuf>> {
    let sorted: HashSet<&Path> = selected
        .keys()
        .map(PathBuf::as_path)
        .collect();
    // Each project's tunneling walk reads only shared references, so
    // the projects fan out across the rayon pool; collecting the
    // parallel iterator into a `Vec` keeps the selection order.
    selected
        .keys()
        .collect::<Vec<_>>()
        .par_iter()
        .map(|&project_dir| {
            let full_graph = match prod_all {
                Some(prod_all) if prod_only_selected.contains(project_dir) => prod_all,
                _ => all,
            };
            (project_dir.clone(), sorted_dependencies(selected, full_graph, project_dir, &sorted))
        })
        .collect::<Vec<_>>()
        .into_iter()
        .collect()
}

/// Sequence `projects_graph` into one deterministic topological order,
/// resolving transitive edges through `full_projects_graph`.
pub fn sequence_graph<Pkg>(
    projects_graph: &ProjectGraph<Pkg>,
    full_projects_graph: &ProjectGraph<Pkg>,
) -> GraphSequencerResult<PathBuf> {
    sequence_graph_by_project(projects_graph, |_| full_projects_graph)
}

/// Sequence `projects_graph`, resolving each project's transitive edges
/// through the full graph that `full_graph_for` returns for it. A
/// `--filter-prod` selection routes its projects to the prod-pruned graph so
/// pruned dev edges stay pruned, while regular projects route to the full
/// graph.
fn sequence_graph_by_project<'g, Pkg: 'g>(
    projects_graph: &ProjectGraph<Pkg>,
    full_graph_for: impl Fn(&Path) -> &'g ProjectGraph<Pkg>,
) -> GraphSequencerResult<PathBuf> {
    let sorted_dirs: Vec<PathBuf> = projects_graph.keys().cloned().collect();
    let sorted: HashSet<&Path> = sorted_dirs
        .iter()
        .map(PathBuf::as_path)
        .collect();
    let dependency_graph: HashMap<PathBuf, Vec<PathBuf>> = projects_graph
        .keys()
        .map(|project_dir| {
            let dependencies = sorted_dependencies(
                projects_graph,
                full_graph_for(project_dir),
                project_dir,
                &sorted,
            );
            (project_dir.clone(), dependencies)
        })
        .collect();
    graph_sequencer(&dependency_graph, &sorted_dirs)
}

/// The dependencies of `project_dir` that are themselves in `sorted`, reached
/// by tunneling past any project outside `sorted`. A transitive dependency
/// between two sorted projects thus becomes a direct edge.
///
/// `project_dir`'s own edges are read from `projects_graph`, so a selection
/// that deliberately narrows them (e.g. a prod-only filter that drops dev
/// edges) is respected; `full_projects_graph` is consulted only to walk
/// through the projects outside `sorted`.
fn sorted_dependencies<Pkg>(
    projects_graph: &ProjectGraph<Pkg>,
    full_projects_graph: &ProjectGraph<Pkg>,
    project_dir: &Path,
    sorted: &HashSet<&Path>,
) -> Vec<PathBuf> {
    let mut dependencies: Vec<PathBuf> = Vec::new();
    // Borrowed paths and an FxHash set: this walk runs once per
    // selected project, and cloning every visited `PathBuf` into a
    // SipHash set dominated it on a workspace-scale graph.
    let mut visited: rustc_hash::FxHashSet<&Path> = rustc_hash::FxHashSet::default();
    let mut stack: Vec<&Path> = projects_graph
        .get(project_dir)
        .map(|node| {
            node.dependencies
                .iter()
                .map(PathBuf::as_path)
                .collect()
        })
        .unwrap_or_default();
    while let Some(dependency_dir) = stack.pop() {
        if dependency_dir == project_dir || !visited.insert(dependency_dir) {
            continue;
        }
        if sorted.contains(dependency_dir) {
            dependencies.push(dependency_dir.to_path_buf());
        } else if let Some(node) = full_projects_graph.get(dependency_dir) {
            stack.extend(node.dependencies.iter().map(PathBuf::as_path));
        }
    }
    dependencies
}

/// The projects a gated recursive command holds to the modules-directory
/// requirement: `project_dirs` themselves, plus every workspace dependency
/// they reach that the installs the gate spawns cover.
///
/// A workspace dependency that is not installed leaves the selection out of
/// date too. With one shared lockfile, the install the gate spawns selects the
/// dependencies of the selected projects, and a negated selector reaches it
/// unchanged, so a dependency it excludes is not required; `project_dirs`
/// themselves always are. With a lockfile per project the gate checks and
/// installs each project on its own, and a workspace dependency the project
/// injects is part of that project's own install. `prefix` is where the
/// command's path selectors resolve.
pub fn projects_to_verify<'dirs>(
    project_dirs: impl IntoIterator<Item = &'dirs Path>,
    selection: &RecursiveSelection<'_>,
    config: &Config,
    prefix: &Path,
) -> miette::Result<Vec<PathBuf>> {
    if !config.verify_deps_before_run.is_enabled() {
        return Ok(Vec::new());
    }
    let project_dirs: Vec<&Path> = project_dirs.into_iter().collect();
    // An injected workspace dependency is a copy inside the project that
    // depends on it, which that project's own install covers; explicit `link:`
    // dependencies remain symlinks and still need their target project installed.
    let only_linked = !config.shares_one_lockfile() && config.inject_workspace_packages;
    let catalogs = if only_linked { Some(configured_catalogs(config)?) } else { None };
    let ws_catalogs = catalogs.as_ref().and_then(|catalogs| workspace_catalogs(config, catalogs));
    let mut projects = projects_with_workspace_dependencies_filtered(
        project_dirs.iter().copied(),
        selection.full_graph(),
        selection.prod_all.as_ref(),
        &selection.prod_only_selected,
        |parent_dir, pkg, deps| {
            retained_dependency_dirs(
                parent_dir,
                &pkg.project.manifest,
                deps,
                only_linked,
                ws_catalogs,
            )
        },
    );
    if config.shares_one_lockfile() && has_negated_selector(config) {
        let mut kept = install_selected_projects(selection, config, prefix)?;
        kept.extend(project_dirs.iter().map(|dir| dir.to_path_buf()));
        projects.retain(|project_dir| kept.contains(project_dir));
    }
    Ok(projects)
}

fn retained_dependency_dirs<'graph>(
    parent_dir: &Path,
    manifest: &PackageManifest,
    deps: &'graph [PathBuf],
    only_linked: bool,
    catalogs: Option<WorkspaceCatalogs<'_>>,
) -> Vec<&'graph Path> {
    if !only_linked || deps.is_empty() {
        return deps
            .iter()
            .map(PathBuf::as_path)
            .collect();
    }
    let linked = linked_dependency_dirs(parent_dir, manifest, catalogs);
    deps.iter()
        .map(PathBuf::as_path)
        .filter(|dep_dir| linked.contains(&lexical_normalize(dep_dir)))
        .collect()
}

fn linked_dependency_dirs(
    parent_dir: &Path,
    manifest: &PackageManifest,
    catalogs: Option<WorkspaceCatalogs<'_>>,
) -> rustc_hash::FxHashSet<PathBuf> {
    manifest
        .dependencies([
            DependencyGroup::Prod,
            DependencyGroup::Dev,
            DependencyGroup::Optional,
            DependencyGroup::Peer,
        ])
        .filter_map(|(dep_name, raw_spec)| {
            let resolved = resolve_catalog_spec(parent_dir, dep_name, raw_spec, catalogs);
            let spec = resolved.as_deref().unwrap_or(raw_spec);
            let rel = spec.strip_prefix("link:")?;
            Some(lexical_normalize(&parent_dir.join(rel.replace('\\', "/"))))
        })
        .collect()
}

fn resolve_catalog_spec(
    consumer_dir: &Path,
    dep_name: &str,
    raw_spec: &str,
    catalogs: Option<WorkspaceCatalogs<'_>>,
) -> Option<String> {
    let catalogs = catalogs?;
    if !raw_spec.starts_with("catalog:") {
        return None;
    }
    let wanted =
        WantedDependency { alias: dep_name.to_string(), bare_specifier: raw_spec.to_string() };
    let anchor = CatalogAnchor::Reanchor {
        workspace_dir: catalogs.workspace_dir,
        consumer_dir: Some(consumer_dir),
    };
    match resolve_from_catalog(catalogs.catalogs, &wanted, anchor) {
        CatalogResolutionResult::Found(found) => Some(found.resolution.specifier),
        CatalogResolutionResult::Misconfiguration(_) | CatalogResolutionResult::Unused => None,
    }
}

fn has_negated_selector(config: &Config) -> bool {
    config.filter
        .iter()
        .chain(&config.filter_prod)
        .any(|selector| selector.starts_with('!'))
}

/// The projects the install the gate spawns selects, evaluated against the
/// graphs the command's own selection was drawn from.
fn install_selected_projects(
    selection: &RecursiveSelection<'_>,
    config: &Config,
    prefix: &Path,
) -> miette::Result<HashSet<PathBuf>> {
    let walk_opts = recursive_filter_options(config, prefix);
    let filter = with_dependencies_selectors(&config.filter);
    let mut installed: HashSet<PathBuf> =
        filter_against(selection.full_graph(), &filter, None, false, prefix, &walk_opts)?
            .into_iter()
            .collect();
    if let Some(prod_all) = &selection.prod_all {
        let filter_prod = with_dependencies_selectors(&config.filter_prod);
        installed.extend(filter_against(prod_all, &filter_prod, None, true, prefix, &walk_opts)?);
    }
    Ok(installed)
}

fn with_dependencies_selectors(selectors: &[String]) -> Vec<String> {
    selectors
        .iter()
        .map(|selector| with_dependencies(selector))
        .collect()
}

/// `project_dirs` together with every workspace project they depend on,
/// directly or transitively, along the dependency edges that `filter_edges`
/// retains for each parent node `(parent_dir, &parent_node.package,
/// &parent_node.dependencies)`.
///
/// The install a gated command spawns selects the dependencies of the projects
/// the command selected, so a project whose workspace dependency has no modules
/// directory is not up to date either. `all` is the graph the walk reads;
/// a project named by `prod_only_selected` reads `prod_all` instead, as
/// [`filtered_projects_dependencies`] does, so a dev-only workspace dependency
/// of a `--filter-prod` selection is not required.
///
/// Each graph keeps one visited set across all roots, so a dependency shared
/// by several selected projects is walked once.
pub(super) fn projects_with_workspace_dependencies_filtered<'graph, 'dirs, Pkg, Filter>(
    project_dirs: impl IntoIterator<Item = &'dirs Path>,
    all: &'graph ProjectGraph<Pkg>,
    prod_all: Option<&'graph ProjectGraph<Pkg>>,
    prod_only_selected: &HashSet<PathBuf>,
    filter_edges: Filter,
) -> Vec<PathBuf>
where
    Filter: Fn(&Path, &Pkg, &'graph [PathBuf]) -> Vec<&'graph Path>,
{
    let mut all_walk = DependencyWalk::new(all, &filter_edges);
    let mut prod_walk = prod_all.map(|graph| DependencyWalk::new(graph, &filter_edges));
    let mut outside_graph: Vec<&Path> = Vec::new();
    for project_dir in project_dirs {
        let walk = match &mut prod_walk {
            Some(prod_walk) if prod_only_selected.contains(project_dir) => prod_walk,
            _ => &mut all_walk,
        };
        if !walk.visit_from(project_dir) {
            outside_graph.push(project_dir);
        }
    }
    let mut collected: rustc_hash::FxHashSet<&Path> = rustc_hash::FxHashSet::default();
    all_walk.visited
        .into_iter()
        .chain(prod_walk.into_iter().flat_map(|walk| walk.visited))
        .chain(outside_graph)
        .filter(|project_dir| collected.insert(project_dir))
        .map(Path::to_path_buf)
        .collect()
}

/// A walk along the dependency edges of one graph, from any number of roots.
struct DependencyWalk<'graph, 'filter, Pkg, Filter> {
    graph: &'graph ProjectGraph<Pkg>,
    filter_edges: &'filter Filter,
    seen: rustc_hash::FxHashSet<&'graph Path>,
    /// The projects reached, in the order they were first reached.
    visited: Vec<&'graph Path>,
}

impl<'graph, 'filter, Pkg, Filter> DependencyWalk<'graph, 'filter, Pkg, Filter>
where
    Filter: Fn(&Path, &Pkg, &'graph [PathBuf]) -> Vec<&'graph Path>,
{
    fn new(graph: &'graph ProjectGraph<Pkg>, filter_edges: &'filter Filter) -> Self {
        DependencyWalk {
            graph,
            filter_edges,
            seen: rustc_hash::FxHashSet::default(),
            visited: Vec::new(),
        }
    }

    /// Visit `root` and every project reachable from it that no earlier root
    /// reached. Returns `false`, visiting nothing, when `root` is not in the
    /// graph.
    fn visit_from(&mut self, root: &Path) -> bool {
        let Some((root, node)) = self.graph.get_key_value(root) else {
            return false;
        };
        if !self.seen.insert(root.as_path()) {
            return true;
        }
        self.visited.push(root.as_path());
        let mut stack: Vec<&'graph Path> =
            (self.filter_edges)(root, &node.package, &node.dependencies);
        while let Some(project_dir) = stack.pop() {
            if !self.seen.insert(project_dir) {
                continue;
            }
            self.visited.push(project_dir);
            if let Some(node) = self.graph.get(project_dir) {
                stack.extend((self.filter_edges)(project_dir, &node.package, &node.dependencies));
            }
        }
        true
    }
}
