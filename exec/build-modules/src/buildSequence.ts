import { type Groups, graphSequencer } from '@pnpm/deps.graph-sequencer'
import type { PackageManifest, PatchFile } from '@pnpm/types'
import filter from 'ramda/src/filter'

export interface DependenciesGraphNode {
  children: Record<string, string>
  depPath: string
  name: string
  dir: string
  fetchingBundledManifest?: (() => Promise<PackageManifest | undefined>) | undefined
  filesIndexFile?: string | undefined
  hasBin: boolean
  hasBundledDependencies: boolean
  installable?: boolean | undefined
  isBuilt?: boolean | undefined
  optional: boolean
  optionalDependencies: Set<string>
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  requiresBuild?: boolean | undefined | any // this is a dirty workaround added in https://github.com/pnpm/pnpm/pull/4898
  patchFile?: PatchFile | undefined
}

export interface DependenciesGraph {
  [depPath: string]: DependenciesGraphNode
}

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
      return [
        depPath,
        onlyFromBuildGraph(Object.values(depGraph[depPath].children)),
      ];
    })
  )
  const graphSequencerResult = graphSequencer(graph, nodesToBuildArray)
  const chunks = graphSequencerResult.chunks
  return chunks
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
        Object.values(node.children),
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
