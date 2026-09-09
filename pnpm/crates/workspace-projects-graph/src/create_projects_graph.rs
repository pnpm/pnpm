use crate::{
    base_project::GraphProject,
    graph::{ProjectGraph, ProjectGraphNode},
};
use indexmap::IndexMap;
use node_semver::{Range, Version};
use pnpm_fs::lexical_normalize;
use pnpm_workspace_range_resolver::resolve_workspace_range;
use pnpm_workspace_spec::WorkspaceSpec;
use rayon::prelude::*;
use std::{collections::HashMap, path::PathBuf};

/// Options for [`create_projects_graph()`].
#[derive(Debug, Default, Clone, Copy)]
pub struct CreateProjectsGraphOptions {
    /// Exclude `devDependencies` from edge computation. Set when building
    /// the `--filter-prod` graph so dependency walks follow production
    /// deps only.
    pub ignore_dev_deps: bool,
    /// Whether workspace packages are linked. The tri-state mirrors the
    /// `linkWorkspacePackages` setting.
    pub link_workspace_packages: Option<bool>,
}

/// A dependency that named a workspace sibling but whose version range
/// no sibling satisfied (or that was rejected by strict
/// `linkWorkspacePackages: false` matching).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unmatched {
    pub pkg_name: String,
    pub range: String,
}

/// Return value of [`create_projects_graph()`]: the graph plus the list
/// of dependencies that named a sibling but matched no version.
#[derive(Debug, Clone)]
pub struct CreateProjectsGraphResult<Pkg> {
    pub graph: ProjectGraph<Pkg>,
    pub unmatched: Vec<Unmatched>,
}

/// Build the workspace dependency graph from a project list.
///
/// Each project becomes a node keyed by its root directory; its edges
/// are the root directories of the workspace siblings its dependencies
/// resolve to.
///
/// A non-`workspace:` specifier is classified with a small local-path /
/// semver check rather than a full `npm-package-arg` resolve. The cases
/// this graph acts on (`directory`, `version`, `range`) are covered; the
/// on-disk file-vs-directory disambiguation `npm-package-arg` performs
/// for `file:` tarballs is not, because a workspace sibling is always a
/// directory.
#[must_use]
pub fn create_projects_graph<Pkg>(
    projects: Vec<Pkg>,
    opts: &CreateProjectsGraphOptions,
) -> CreateProjectsGraphResult<Pkg>
where
    Pkg: GraphProject,
{
    let count = projects.len();
    let fields = snapshot_project_fields(&projects, opts.ignore_dev_deps);
    let by_name = index_by_name(&fields.names);
    let by_dir = index_by_dir(&fields.node_keys);
    let lookups = Lookups {
        node_keys: &fields.node_keys,
        names: &fields.names,
        versions: &fields.versions,
        by_name: &by_name,
        by_dir: &by_dir,
        link_workspace_packages: opts.link_workspace_packages,
    };
    let (all_edges, unmatched) = resolve_all_edges(&fields.dependency_lists, &lookups);

    let mut graph: ProjectGraph<Pkg> = IndexMap::with_capacity(count);
    for (package, (key, dependencies)) in
        projects.into_iter().zip(fields.node_keys.into_iter().zip(all_edges))
    {
        graph.insert(key, ProjectGraphNode { package, dependencies });
    }

    CreateProjectsGraphResult { graph, unmatched }
}

/// The per-project fields edge resolution reads, taken before the projects
/// are moved into the graph nodes so the lookups own their data and don't
/// contend with the node-building move.
struct ProjectFields {
    node_keys: Vec<PathBuf>,
    names: Vec<Option<String>>,
    versions: Vec<Option<String>>,
    dependency_lists: Vec<Vec<(String, String)>>,
}

fn snapshot_project_fields<Pkg>(projects: &[Pkg], ignore_dev_deps: bool) -> ProjectFields
where
    Pkg: GraphProject,
{
    ProjectFields {
        node_keys: projects.iter().map(|project| project.root_dir().to_path_buf()).collect(),
        names: projects.iter().map(|project| project.manifest_name().map(str::to_string)).collect(),
        versions: projects
            .iter()
            .map(|project| project.manifest_version().map(str::to_string))
            .collect(),
        dependency_lists: projects
            .iter()
            .map(|project| project.merged_dependencies(ignore_dev_deps))
            .collect(),
    }
}

/// Each importer's edges resolve against the immutable lookup tables only,
/// so the importers fan out across the rayon pool; the per-importer
/// unmatched lists are flattened in importer order, keeping the reported
/// set and its order deterministic.
fn resolve_all_edges(
    dependency_lists: &[Vec<(String, String)>],
    lookups: &Lookups<'_>,
) -> (Vec<Vec<PathBuf>>, Vec<Unmatched>) {
    let per_importer: Vec<(Vec<PathBuf>, Vec<Unmatched>)> = dependency_lists
        .par_iter()
        .enumerate()
        .map(|(importer, dependencies)| resolve_importer_edges(importer, dependencies, lookups))
        .collect();
    let mut all_edges: Vec<Vec<PathBuf>> = Vec::with_capacity(per_importer.len());
    let mut unmatched = Vec::new();
    for (edges, importer_unmatched) in per_importer {
        all_edges.push(edges);
        unmatched.extend(importer_unmatched);
    }
    (all_edges, unmatched)
}

/// Every importer that declares each manifest name.
fn index_by_name(names: &[Option<String>]) -> HashMap<String, Vec<usize>> {
    let mut by_name: HashMap<String, Vec<usize>> = HashMap::new();
    for (index, name) in names.iter().enumerate() {
        if let Some(name) = name {
            by_name.entry(name.clone()).or_default().push(index);
        }
    }
    by_name
}

/// The importer each root directory belongs to, keyed by its normalized path.
fn index_by_dir(node_keys: &[PathBuf]) -> HashMap<PathBuf, usize> {
    let mut by_dir = HashMap::with_capacity(node_keys.len());
    for (index, key) in node_keys.iter().enumerate() {
        by_dir.insert(lexical_normalize(key), index);
    }
    by_dir
}

/// The sibling projects one importer's dependencies resolve to, and the
/// specifiers that matched none.
fn resolve_importer_edges(
    importer: usize,
    dependencies: &[(String, String)],
    lookups: &Lookups<'_>,
) -> (Vec<PathBuf>, Vec<Unmatched>) {
    let mut edges = Vec::new();
    let mut unmatched = Vec::new();
    for (dep_name, raw_spec) in dependencies {
        if let Some(target) = resolve_edge(importer, dep_name, raw_spec, lookups, &mut unmatched) {
            edges.push(target);
        }
    }
    (edges, unmatched)
}

/// Immutable lookup tables shared across edge resolution, snapshotted
/// from the project list so the helpers borrow rather than re-read.
struct Lookups<'a> {
    node_keys: &'a [PathBuf],
    names: &'a [Option<String>],
    versions: &'a [Option<String>],
    by_name: &'a HashMap<String, Vec<usize>>,
    by_dir: &'a HashMap<PathBuf, usize>,
    link_workspace_packages: Option<bool>,
}

/// How a non-`workspace:` specifier is matched against siblings.
enum SpecKind<'a> {
    /// A local path (`file:` / `link:` / relative / absolute), matched
    /// by directory. Carries the path portion to resolve against the
    /// importer's root directory.
    Directory(&'a str),
    /// A semver version or range, matched by name + version.
    VersionOrRange,
    /// Neither (tag, git URL, `npm:` alias, ...) — no edge.
    Skip,
}

fn resolve_edge(
    importer: usize,
    dep_name: &str,
    raw_spec: &str,
    lookups: &Lookups,
    unmatched: &mut Vec<Unmatched>,
) -> Option<PathBuf> {
    let is_workspace_spec = raw_spec.starts_with("workspace:");
    let (effective_name, effective_spec) = if is_workspace_spec {
        let spec = WorkspaceSpec::parse(raw_spec)?;
        (spec.alias.unwrap_or_else(|| dep_name.to_string()), spec.version)
    } else {
        (dep_name.to_string(), raw_spec.to_string())
    };

    if is_workspace_spec {
        if let SpecKind::Directory(path) = classify(&effective_spec) {
            return resolve_directory(importer, path, lookups);
        }
        return resolve_by_name_version(&effective_name, &effective_spec, true, lookups, unmatched);
    }

    match classify(&effective_spec) {
        SpecKind::Directory(path) => resolve_directory(importer, path, lookups),
        SpecKind::VersionOrRange => {
            resolve_by_name_version(&effective_name, &effective_spec, false, lookups, unmatched)
        }
        SpecKind::Skip => None,
    }
}

/// Resolve a local-path dependency to a sibling by directory: join the
/// path onto the importer's root, normalize, and look it up in the
/// by-directory index.
fn resolve_directory(importer: usize, path: &str, lookups: &Lookups) -> Option<PathBuf> {
    let resolved = lexical_normalize(&lookups.node_keys[importer].join(path));
    lookups.by_dir.get(&resolved).map(|&index| lookups.node_keys[index].clone())
}

fn resolve_by_name_version(
    dep_name: &str,
    raw_spec: &str,
    is_workspace_spec: bool,
    lookups: &Lookups,
    unmatched: &mut Vec<Unmatched>,
) -> Option<PathBuf> {
    let candidates = lookups.by_name.get(dep_name)?;

    if lookups.link_workspace_packages == Some(false) && !is_workspace_spec {
        unmatched.push(Unmatched { pkg_name: dep_name.to_string(), range: raw_spec.to_string() });
        return None;
    }

    let candidate_versions: Vec<&str> =
        candidates.iter().filter_map(|&index| lookups.versions[index].as_deref()).collect();

    if is_workspace_spec && candidate_versions.is_empty() {
        let index =
            *candidates.iter().find(|&&index| lookups.names[index].as_deref() == Some(dep_name))?;
        return Some(lookups.node_keys[index].clone());
    }

    if candidate_versions.contains(&raw_spec) {
        let index = *candidates
            .iter()
            .find(|&&index| lookups.versions[index].as_deref() == Some(raw_spec))?;
        return Some(lookups.node_keys[index].clone());
    }

    let owned_versions: Vec<String> =
        candidate_versions.iter().map(|&version| version.to_string()).collect();
    match resolve_workspace_range(raw_spec, &owned_versions) {
        None => {
            unmatched
                .push(Unmatched { pkg_name: dep_name.to_string(), range: raw_spec.to_string() });
            None
        }
        Some(matched) => {
            let index = *candidates
                .iter()
                .find(|&&index| lookups.versions[index].as_deref() == Some(matched.as_str()))?;
            Some(lookups.node_keys[index].clone())
        }
    }
}

/// Classify a non-`workspace:` specifier into the three shapes
/// [`create_projects_graph()`] acts on. See the function's doc comment
/// for why this is a focused check rather than a full
/// `npm-package-arg` resolve.
fn classify(spec: &str) -> SpecKind<'_> {
    if let Some(rest) = spec.strip_prefix("file:").or_else(|| spec.strip_prefix("link:")) {
        return SpecKind::Directory(rest);
    }
    if is_path_like(spec) {
        return SpecKind::Directory(spec);
    }
    if Version::parse(spec).is_ok() || Range::parse(spec).is_ok() {
        return SpecKind::VersionOrRange;
    }
    SpecKind::Skip
}

/// Whether `spec` looks like a filesystem path rather than a version or
/// protocol-prefixed selector.
fn is_path_like(spec: &str) -> bool {
    matches!(spec, "." | "..")
        || spec.starts_with("./")
        || spec.starts_with("../")
        || spec.starts_with(r".\")
        || spec.starts_with(r"..\")
        || spec.starts_with('/')
        || spec.starts_with('\\')
        || spec.starts_with("~/")
        || has_windows_drive_prefix(spec)
}

/// `C:\` / `C:/` style Windows drive prefixes.
fn has_windows_drive_prefix(spec: &str) -> bool {
    let mut chars = spec.chars();
    matches!(
        (chars.next(), chars.next(), chars.next()),
        (Some(letter), Some(':'), Some('/' | '\\')) if letter.is_ascii_alphabetic(),
    )
}

#[cfg(test)]
mod tests;
