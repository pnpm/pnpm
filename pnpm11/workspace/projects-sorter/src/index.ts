import type { Result as GraphSequencerResult } from '@pnpm/deps.graph-sequencer'
import { graphSequencer } from '@pnpm/deps.graph-sequencer'
import type { ProjectRootDir, ProjectsGraph } from '@pnpm/types'

/**
 * Topologically sequences the projects in `projectsGraph`.
 *
 * Transitive edges are resolved through `fullProjectsGraph`, so a dependency
 * relationship between two of the sorted projects is honored even when the
 * projects connecting them are absent from `projectsGraph` — e.g. when a
 * `--filter` selects two projects but not the one between them. It defaults to
 * `projectsGraph`, in which case resolution is limited to the edges among the
 * sorted projects themselves.
 */
export function sequenceGraph (
  projectsGraph: ProjectsGraph,
  fullProjectsGraph: ProjectsGraph = projectsGraph
): GraphSequencerResult<ProjectRootDir> {
  const sortedProjectDirs = Object.keys(projectsGraph) as ProjectRootDir[]
  const sorted = new Set(sortedProjectDirs)
  const graph = new Map<ProjectRootDir, ProjectRootDir[]>(
    sortedProjectDirs.map((projectDir) => [
      projectDir,
      sortedDependencies(projectsGraph, fullProjectsGraph, projectDir, sorted),
    ])
  )
  return graphSequencer(graph, sortedProjectDirs)
}

export function sortProjects (
  projectsGraph: ProjectsGraph,
  fullProjectsGraph?: ProjectsGraph
): ProjectRootDir[][] {
  return sequenceGraph(projectsGraph, fullProjectsGraph).chunks
}

/**
 * Topologically chunks the projects selected by a `--filter`ed recursive
 * command. Order is resolved through `allProjectsGraph` so a relationship
 * between two selected projects via an unselected one is honored.
 *
 * A prod-only filter (`--filter-prod`) builds `selectedProjectsGraph` from a
 * graph with dev dependencies pruned; tunneling through `allProjectsGraph`,
 * which keeps them, would reintroduce edges the filter dropped. So when a
 * prod-only filter is in effect, order is resolved among the selected projects
 * alone.
 */
export function sortFilteredProjects (opts: {
  selectedProjectsGraph: ProjectsGraph
  allProjectsGraph?: ProjectsGraph
  filterProd?: string[]
}): ProjectRootDir[][] {
  return sortProjects(opts.selectedProjectsGraph, opts.filterProd?.length ? undefined : opts.allProjectsGraph)
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
