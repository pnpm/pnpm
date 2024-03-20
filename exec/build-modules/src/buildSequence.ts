import { type Groups, graphSequencer } from '@pnpm/deps.graph-sequencer'
import type { DependenciesGraphNode } from '@pnpm/types'
import filter from 'ramda/src/filter'

export function buildSequence(
  depGraph: Record<
    string,
    Pick<DependenciesGraphNode, 'children' | 'requiresBuild'>
  >,
  rootDepPaths: string[]
): Groups<string> {
  const nodesToBuild = new Set<string>()

  getSubgraphToBuild(depGraph, rootDepPaths, nodesToBuild, new Set<string>())

  const onlyFromBuildGraph = filter((depPath: string) =>
    nodesToBuild.has(depPath)
  )

  const nodesToBuildArray = Array.from(nodesToBuild)

  const graph = new Map(
    nodesToBuildArray.map((depPath: string): [string, string[]] => {
      const arr = onlyFromBuildGraph(Object.values(depGraph[depPath].children ?? {}).filter(Boolean))

      return [
        depPath,
        Array.isArray((arr)) ? arr : [],
      ];
    })
  )

  const graphSequencerResult = graphSequencer(graph, nodesToBuildArray)

  return graphSequencerResult.chunks
}

function getSubgraphToBuild(
  graph: Record<
    string,
    Pick<DependenciesGraphNode, 'children' | 'requiresBuild' | 'patchFile'>
  >,
  entryNodes: string[],
  nodesToBuild: Set<string>,
  walked: Set<string>
): boolean {
  let currentShouldBeBuilt = false

  for (const depPath of entryNodes) {
    const node = graph[depPath]

    if (!node) continue // packages that are already in node_modules are skipped

    if (walked.has(depPath)) continue

    walked.add(depPath)

    const childShouldBeBuilt =
      getSubgraphToBuild(
        graph,
        Object.values(node.children ?? {}).filter((Boolean)) ?? [],
        nodesToBuild,
        walked
      ) ||
      node.requiresBuild ||
      node.patchFile != null
    if (childShouldBeBuilt) {
      nodesToBuild.add(depPath)
      currentShouldBeBuilt = true
    }
  }
  return currentShouldBeBuilt
}
