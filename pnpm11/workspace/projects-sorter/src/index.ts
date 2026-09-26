import type { Result as GraphSequencerResult } from '@pnpm/deps.graph-sequencer'
import { graphSequencer } from '@pnpm/deps.graph-sequencer'
import type { ProjectRootDir, ProjectsGraph } from '@pnpm/types'

/**
 * Returns a deterministic topological order and any dependency cycles in
 * `projectsGraph`.
 */
export function sequenceGraph (projectsGraph: ProjectsGraph): GraphSequencerResult<ProjectRootDir> {
  const graph = projectsDependencies(projectsGraph)
  const projectDirs = [...graph.keys()]
  return graphSequencer(graph, projectDirs)
}

export function projectsDependencies (projectsGraph: ProjectsGraph): Map<ProjectRootDir, ProjectRootDir[]> {
  const projectDirs = Object.keys(projectsGraph) as ProjectRootDir[]
  const included = new Set(projectDirs)
  return new Map(projectDirs.map((projectDir) => [
    projectDir,
    projectsGraph[projectDir].dependencies.filter((dependency) => dependency !== projectDir && included.has(dependency)),
  ]))
}

export interface FilteredProjectsGraphOptions {
  selectedProjectsGraph: ProjectsGraph
  allProjectsGraph?: ProjectsGraph
  prodAllProjectsGraph?: ProjectsGraph
  prodOnlySelectedProjectDirs?: ProjectRootDir[]
}

/**
 * The dependency edges among the projects selected by a `--filter`ed recursive
 * command, resolved through the full workspace graph so a relationship between
 * two selected projects via an unselected one becomes a direct edge.
 *
 * `prodAllProjectsGraph` is the prod-pruned full graph. In mixed selections,
 * `prodOnlySelectedProjectDirs` marks the selected projects that should resolve
 * through it; regular-selected projects still resolve through `allProjectsGraph`.
 */
export function filteredProjectsDependencies (opts: FilteredProjectsGraphOptions): Map<ProjectRootDir, ProjectRootDir[]> {
  const fullProjectsGraph = opts.allProjectsGraph ?? opts.selectedProjectsGraph
  const prodAllProjectsGraph = opts.prodAllProjectsGraph
  if (!prodAllProjectsGraph) {
    return dependenciesByProject(opts.selectedProjectsGraph, () => fullProjectsGraph)
  }
  const prodOnlySelectedProjectDirs = new Set(opts.prodOnlySelectedProjectDirs)
  return dependenciesByProject(
    opts.selectedProjectsGraph,
    (projectDir) => prodOnlySelectedProjectDirs.has(projectDir)
      ? prodAllProjectsGraph
      : fullProjectsGraph
  )
}

/**
 * Resolves each key of `projectsGraph` to its edges to the other keys through
 * the graph that `fullProjectsGraphByProject` returns for it. Letting that
 * graph vary per project lets a prod-only selected project tunnel through the
 * prod-pruned graph while the rest tunnel through the full one.
 */
function dependenciesByProject (
  projectsGraph: ProjectsGraph,
  fullProjectsGraphByProject: (projectDir: ProjectRootDir) => ProjectsGraph
): Map<ProjectRootDir, ProjectRootDir[]> {
  const sortedProjectDirs = Object.keys(projectsGraph) as ProjectRootDir[]
  const sorted = new Set(sortedProjectDirs)
  return new Map<ProjectRootDir, ProjectRootDir[]>(
    sortedProjectDirs.map((projectDir) => [
      projectDir,
      sortedDependencies(projectsGraph, fullProjectsGraphByProject(projectDir), projectDir, sorted),
    ])
  )
}

/**
 * The dependencies of `projectDir` that are themselves in `sorted`, reached by
 * tunneling past any project outside `sorted`. A transitive dependency between
 * two sorted projects thus becomes a direct edge.
 *
 * `projectDir`'s own edges are read from `projectsGraph`, so a selection that
 * deliberately narrows them (e.g. a prod-only filter that drops dev edges) is
 * respected; `fullProjectsGraph` is consulted only to walk through projects
 * outside `sorted`, which are absent from `projectsGraph`.
 */
function sortedDependencies (
  projectsGraph: ProjectsGraph,
  fullProjectsGraph: ProjectsGraph,
  projectDir: ProjectRootDir,
  sorted: ReadonlySet<ProjectRootDir>
): ProjectRootDir[] {
  const dependencies = new Set<ProjectRootDir>()
  const visited = new Set<ProjectRootDir>()
  const stack = [...(projectsGraph[projectDir]?.dependencies ?? [])]
  while (stack.length > 0) {
    const dependencyDir = stack.pop()!
    if (dependencyDir === projectDir || visited.has(dependencyDir)) continue
    visited.add(dependencyDir)
    if (sorted.has(dependencyDir)) {
      dependencies.add(dependencyDir)
    } else {
      const transitiveDeps = fullProjectsGraph[dependencyDir]?.dependencies
      if (transitiveDeps) stack.push(...transitiveDeps)
    }
  }
  return Array.from(dependencies)
}
