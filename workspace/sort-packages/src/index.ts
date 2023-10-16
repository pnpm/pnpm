import type { ProjectsGraph } from '@pnpm/types'
import { graphSequencer } from '@pnpm/deps.graph-sequencer'
import type { Result as GraphSequencerResult } from '@pnpm/deps.graph-sequencer'

export function sequenceGraph (pkgGraph: ProjectsGraph): GraphSequencerResult<string> {
  const keys = Object.keys(pkgGraph)
  const setOfKeys = new Set(keys)
  const graph = new Map(
    keys.map((pkgPath) => [
      pkgPath,
      pkgGraph[pkgPath].dependencies.filter(
        d => d !== pkgPath && setOfKeys.has(d))]
    )
  )
  return graphSequencer(graph, keys)
}

export function sortPackages (pkgGraph: ProjectsGraph): string[][] {
  const graphSequencerResult = sequenceGraph(pkgGraph)
  return graphSequencerResult.chunks
}
