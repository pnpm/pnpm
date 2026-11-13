use super::{
    FilterError, FilterGraphResult, FilterWorkspaceProjectsOptions, FilteredProjects, filter_graph,
};
use crate::parse_project_selector::ProjectSelector;
use indexmap::IndexMap;
use pnpm_workspace_projects_graph::{BaseProject, ProjectGraph};
use std::{collections::HashSet, path::PathBuf};

/// Outcome of filtering across the full workspace graph and an optional
/// production-only graph.
#[derive(Debug, Default, Clone)]
pub struct FilteredGraphProjects {
    /// Selected project root directories, in selection order (projects matched
    /// by a `--filter-prod` selector precede regular-only projects).
    pub selected_projects: Vec<PathBuf>,
    /// Selected project root directories whose active inclusions all came from
    /// `--filter-prod` selectors.
    pub prod_only_projects: HashSet<PathBuf>,
    pub unmatched_filters: Vec<String>,
}

#[derive(Clone, Copy)]
struct SelectionOrigin {
    prod_matched: bool,
    prod_only: bool,
}

struct SelectorChunk<'a> {
    exclude: bool,
    selectors: Vec<&'a ProjectSelector>,
}

struct ChunkEvaluation {
    prod_selected: Vec<PathBuf>,
    all_selected: Vec<PathBuf>,
    unmatched_filters: Vec<String>,
}

/// Filter a pre-built [`ProjectGraph`] by `project_selectors`.
///
/// Selectors are evaluated in ordered chunks of matching polarity. When the
/// first selector is an exclusion, selection begins with every project in the
/// workspace; otherwise it starts empty. Subsequent inclusion selectors re-include
/// projects that were previously excluded.
pub fn filter_workspace_projects<Pkg>(
    projects_graph: &ProjectGraph<Pkg>,
    project_selectors: &[ProjectSelector],
    opts: &FilterWorkspaceProjectsOptions,
) -> Result<FilteredProjects, FilterError>
where
    Pkg: BaseProject,
{
    let result =
        filter_workspace_projects_with_prod_graph(projects_graph, None, project_selectors, opts)?;
    Ok(FilteredProjects {
        selected_projects: result.selected_projects,
        unmatched_filters: result.unmatched_filters,
    })
}

/// Filter `all_graph` and an optional production-pruned `prod_graph` together
/// in a single polarity-chunked pass over `project_selectors`.
///
/// Selectors with `follow_prod_deps_only` walk `prod_graph` (or `all_graph`
/// when `prod_graph` is `None`); regular selectors walk `all_graph`. Evaluating
/// both kinds in one ordered chunk sequence lets exclusions from either flag
/// subtract from inclusions of the other without seeding unselected workspace
/// projects.
pub fn filter_workspace_projects_with_prod_graph<Pkg>(
    all_graph: &ProjectGraph<Pkg>,
    prod_graph: Option<&ProjectGraph<Pkg>>,
    project_selectors: &[ProjectSelector],
    opts: &FilterWorkspaceProjectsOptions,
) -> Result<FilteredGraphProjects, FilterError>
where
    Pkg: BaseProject,
{
    if project_selectors.is_empty() {
        return Ok(FilteredGraphProjects {
            selected_projects: all_graph.keys().cloned().collect(),
            prod_only_projects: HashSet::new(),
            unmatched_filters: Vec::new(),
        });
    }

    let chunks = chunk_selectors(project_selectors);
    let mut selected = initial_selection(all_graph, prod_graph, &chunks);
    let mut unmatched_filters = Vec::new();
    let effective_prod_graph = prod_graph.unwrap_or(all_graph);

    for chunk in chunks {
        let mut eval = evaluate_chunk(all_graph, effective_prod_graph, opts, &chunk)?;
        unmatched_filters.extend(std::mem::take(&mut eval.unmatched_filters));
        apply_chunk(&mut selected, &chunk, eval);
    }

    Ok(finalize_selection(all_graph, prod_graph, selected, unmatched_filters))
}

fn chunk_selectors(project_selectors: &[ProjectSelector]) -> Vec<SelectorChunk<'_>> {
    let mut chunks: Vec<SelectorChunk<'_>> = Vec::new();
    for selector in project_selectors {
        if let Some(last) = chunks
            .last_mut()
            .filter(|last| last.exclude == selector.exclude)
        {
            last.selectors.push(selector);
            continue;
        }
        chunks.push(SelectorChunk { exclude: selector.exclude, selectors: vec![selector] });
    }
    chunks
}

fn initial_selection<Pkg>(
    all_graph: &ProjectGraph<Pkg>,
    prod_graph: Option<&ProjectGraph<Pkg>>,
    chunks: &[SelectorChunk<'_>],
) -> IndexMap<PathBuf, SelectionOrigin> {
    let Some(first) = chunks.first().filter(|chunk| chunk.exclude) else {
        return IndexMap::new();
    };
    let prod_seeded = prod_graph.is_some()
        && first.selectors.iter().all(|selector| selector.follow_prod_deps_only);
    let origin = SelectionOrigin { prod_matched: prod_seeded, prod_only: prod_seeded };
    all_graph
        .keys()
        .map(|dir| (dir.clone(), origin))
        .collect()
}

fn evaluate_chunk<Pkg: BaseProject>(
    all_graph: &ProjectGraph<Pkg>,
    prod_graph: &ProjectGraph<Pkg>,
    opts: &FilterWorkspaceProjectsOptions,
    chunk: &SelectorChunk<'_>,
) -> Result<ChunkEvaluation, FilterError> {
    let (prod_selectors, all_selectors): (Vec<&ProjectSelector>, Vec<&ProjectSelector>) = chunk
        .selectors
        .iter()
        .copied()
        .partition(|selector| selector.follow_prod_deps_only);
    let prod_result = if prod_selectors.is_empty() {
        FilterGraphResult { selected: Vec::new(), unmatched_filters: Vec::new() }
    } else {
        filter_graph(prod_graph, opts, &prod_selectors)?
    };
    let all_result = if all_selectors.is_empty() {
        FilterGraphResult { selected: Vec::new(), unmatched_filters: Vec::new() }
    } else {
        filter_graph(all_graph, opts, &all_selectors)?
    };
    let mut unmatched_filters = prod_result.unmatched_filters;
    unmatched_filters.extend(all_result.unmatched_filters);
    Ok(ChunkEvaluation {
        prod_selected: prod_result.selected,
        all_selected: all_result.selected,
        unmatched_filters,
    })
}

fn apply_chunk(
    selected: &mut IndexMap<PathBuf, SelectionOrigin>,
    chunk: &SelectorChunk<'_>,
    eval: ChunkEvaluation,
) {
    if chunk.exclude {
        let excluded: HashSet<PathBuf> = eval.prod_selected
            .into_iter()
            .chain(eval.all_selected)
            .collect();
        selected.retain(|dir, _| !excluded.contains(dir));
        return;
    }
    for dir in eval.prod_selected {
        selected
            .entry(dir)
            .and_modify(|origin| origin.prod_matched = true)
            .or_insert(SelectionOrigin { prod_matched: true, prod_only: true });
    }
    for dir in eval.all_selected {
        selected
            .entry(dir)
            .and_modify(|origin| origin.prod_only = false)
            .or_insert(SelectionOrigin { prod_matched: false, prod_only: false });
    }
}

fn finalize_selection<Pkg>(
    all_graph: &ProjectGraph<Pkg>,
    prod_graph: Option<&ProjectGraph<Pkg>>,
    selected: IndexMap<PathBuf, SelectionOrigin>,
    unmatched_filters: Vec<String>,
) -> FilteredGraphProjects {
    // Keep graph members only: a `[<since>]` selector can surface a changed
    // directory that no workspace project contains.
    let valid: Vec<(PathBuf, SelectionOrigin)> = selected
        .into_iter()
        .filter(|(dir, _)| all_graph.contains_key(dir))
        .collect();
    let mut selected_projects = Vec::with_capacity(valid.len());
    let mut prod_only_projects = HashSet::new();
    for (dir, origin) in &valid {
        if origin.prod_matched {
            selected_projects.push(dir.clone());
        }
        if prod_graph.is_some() && origin.prod_only {
            prod_only_projects.insert(dir.clone());
        }
    }
    for (dir, origin) in valid {
        if !origin.prod_matched {
            selected_projects.push(dir);
        }
    }
    FilteredGraphProjects { selected_projects, prod_only_projects, unmatched_filters }
}
