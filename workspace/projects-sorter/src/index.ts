import type { Result as GraphSequencerResult } from '@pnpm/deps.graph-sequencer'
import { graphSequencer } from '@pnpm/deps.graph-sequencer'
import type { ProjectRootDir, ProjectsGraph } from '@pnpm/types'

export function sequenceGraph (projectsGraph: ProjectsGraph): GraphSequencerResult<ProjectRootDir> {
  const keys = Object.keys(projectsGraph) as ProjectRootDir[]
  const setOfKeys = new Set(keys)
  const graph = new Map(
    keys.map((projectPath) => [
      projectPath,
      projectsGraph[projectPath].dependencies.filter(
        d => d !== projectPath && setOfKeys.has(d))]
    )
  )
  return graphSequencer(graph, keys)
}

export function sortProjects (projectsGraph: ProjectsGraph): ProjectRootDir[][] {
  const graphSequencerResult = sequenceGraph(projectsGraph)
  return graphSequencerResult.chunks
}
