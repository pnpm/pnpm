use crate::{
    base_project::GraphProject,
    graph::{ProjectGraph, ProjectGraphNode},
};
use indexmap::IndexMap;
use node_semver::{Range, Version};
use pacquet_fs::lexical_normalize;
use pacquet_workspace_range_resolver::resolve_workspace_range;
use pacquet_workspace_spec::WorkspaceSpec;
use std::{collections::HashMap, path::PathBuf};

/// Options for [`create_projects_graph()`]. Mirrors upstream's
/// `createProjectsGraph(projects, opts)` second argument.
#[derive(Debug, Default, Clone, Copy)]
pub struct CreateProjectsGraphOptions {
    /// Exclude `devDependencies` from edge computation. Upstream passes
    /// this when building the `--filter-prod` graph so dependency walks
    /// follow production deps only.
    pub ignore_dev_deps: bool,
    /// Whether workspace packages are linked. Maps to upstream's
    /// tri-state `linkWorkspacePackages`:
    ///
    /// - `None` (upstream `undefined`) and `Some(true)`: permissive. A
    ///   dependency whose specifier resolves to a sibling by name +
    ///   semver creates an edge.
    /// - `Some(false)`: strict. Only `workspace:` specifiers create
    ///   edges; a plain-version dependency that happens to name a
    ///   sibling is reported in [`Unmatched`] instead.
    pub link_workspace_packages: Option<bool>,
}

/// A dependency that named a workspace sibling but whose version range
/// no sibling satisfied (or that was rejected by strict
/// `linkWorkspacePackages: false` matching). Mirrors upstream's
/// `unmatched` entries.
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
/// Port of upstream's
/// [`createProjectsGraph`](https://github.com/pnpm/pnpm/blob/3b62f9da31/workspace/projects-graph/src/index.ts#L19-L109).
/// Each project becomes a node keyed by its root directory; its edges
/// are the root directories of the workspace siblings its dependencies
/// resolve to.
///
/// Edge resolution mirrors upstream's `createNode`: a `workspace:`
/// specifier resolves the sibling by name + version (the version token
/// drives [`resolve_workspace_range`]); a local-path specifier
/// (`file:` / `link:` / a relative or absolute path) resolves by
/// directory; a plain semver version or range resolves by name +
/// version; anything else (registry tag, git URL, `npm:` alias, ...)
/// contributes no edge.
///
/// One deliberate divergence from upstream: pacquet classifies a
/// non-`workspace:` specifier with a small local-path / semver check
/// rather than a full `npm-package-arg` resolve. The cases
/// `createProjectsGraph` acts on (`directory`, `version`, `range`) are
/// covered; the on-disk file-vs-directory disambiguation
/// `npm-package-arg` performs for `file:` tarballs is not, because a
/// workspace sibling is always a directory.
#[must_use]
pub fn create_projects_graph<Pkg>(
    projects: Vec<Pkg>,
    opts: &CreateProjectsGraphOptions,
) -> CreateProjectsGraphResult<Pkg>
where
    Pkg: GraphProject,
{
    let count = projects.len();

    // Snapshot every field edge resolution reads before the projects are
    // moved into the graph nodes below, so the lookups own their data and
    // don't contend with the node-building move.
    let node_keys: Vec<PathBuf> =
        projects.iter().map(|project| project.root_dir().to_path_buf()).collect();
    let names: Vec<Option<String>> =
        projects.iter().map(|project| project.manifest_name().map(str::to_string)).collect();
    let versions: Vec<Option<String>> =
        projects.iter().map(|project| project.manifest_version().map(str::to_string)).collect();
    let dependency_lists: Vec<Vec<(String, String)>> =
        projects.iter().map(|project| project.merged_dependencies(opts.ignore_dev_deps)).collect();

    let mut by_name: HashMap<String, Vec<usize>> = HashMap::new();
    for (index, name) in names.iter().enumerate() {
        if let Some(name) = name {
            by_name.entry(name.clone()).or_default().push(index);
        }
    }
    let mut by_dir: HashMap<PathBuf, usize> = HashMap::with_capacity(count);
    for (index, key) in node_keys.iter().enumerate() {
        by_dir.insert(lexical_normalize(key), index);
    }

    let lookups = Lookups {
        node_keys: &node_keys,
        names: &names,
        versions: &versions,
        by_name: &by_name,
        by_dir: &by_dir,
        link_workspace_packages: opts.link_workspace_packages,
    };

    let mut unmatched = Vec::new();
    let mut all_edges: Vec<Vec<PathBuf>> = Vec::with_capacity(count);
    for (importer, dependencies) in dependency_lists.iter().enumerate() {
        let mut edges = Vec::new();
        for (dep_name, raw_spec) in dependencies {
            if let Some(target) =
                resolve_edge(importer, dep_name, raw_spec, &lookups, &mut unmatched)
            {
                edges.push(target);
            }
        }
        all_edges.push(edges);
    }

    let mut graph: ProjectGraph<Pkg> = IndexMap::with_capacity(count);
    for (package, (key, dependencies)) in
        projects.into_iter().zip(node_keys.into_iter().zip(all_edges))
    {
        graph.insert(key, ProjectGraphNode { package, dependencies });
    }

    CreateProjectsGraphResult { graph, unmatched }
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
        // Upstream runs the workspace token through
        // `workspacePrefToNpm` + `npa.resolve`, so a path-style token
        // (`workspace:../foo`) resolves by directory. The `*` / `^` /
        // `~` / version / range tokens fall through to name + version
        // resolution, where `resolve_workspace_range` interprets the
        // wildcard tokens.
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

    // Strict `linkWorkspacePackages: false` only rejects non-`workspace:`
    // specifiers, matching upstream's `linkWorkspacePackages === false &&
    // !isWorkspaceSpec` guard.
    if lookups.link_workspace_packages == Some(false) && !is_workspace_spec {
        unmatched.push(Unmatched { pkg_name: dep_name.to_string(), range: raw_spec.to_string() });
        return None;
    }

    let candidate_versions: Vec<&str> =
        candidates.iter().filter_map(|&index| lookups.versions[index].as_deref()).collect();

    // A `workspace:` dependency on a sibling that declares no version
    // links to that sibling regardless of the version token.
    if is_workspace_spec && candidate_versions.is_empty() {
        let index =
            *candidates.iter().find(|&&index| lookups.names[index].as_deref() == Some(dep_name))?;
        return Some(lookups.node_keys[index].clone());
    }

    // Exact version-string match wins before range resolution, mirroring
    // upstream's `versions.includes(rawSpec)` short-circuit.
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
