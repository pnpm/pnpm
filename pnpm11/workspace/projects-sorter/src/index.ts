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
      sortedDependencies(fullProjectsGraph, projectDir, sorted),
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
 * The dependencies of `projectDir` that are themselves in `sorted`, reached by
 * walking through `fullProjectsGraph` and tunneling past any project outside
 * `sorted`. A transitive dependency between two sorted projects thus becomes a
 * direct edge.
 */
function sortedDependencies (
  fullProjectsGraph: ProjectsGraph,
  projectDir: ProjectRootDir,
  sorted: ReadonlySet<ProjectRootDir>
): ProjectRootDir[] {
  const dependencies = new Set<ProjectRootDir>()
  const visited = new Set<ProjectRootDir>()
  const stack = [...(fullProjectsGraph[projectDir]?.dependencies ?? [])]
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
