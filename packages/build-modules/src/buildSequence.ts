import graphSequencer from '@pnpm/graph-sequencer'
import { PackageManifest, PatchFile } from '@pnpm/types'
import filter from 'ramda/src/filter.js'

export interface DependenciesGraphNode {
  children: {[alias: string]: string}
  depPath: string
  dir: string
  fetchingBundledManifest?: () => Promise<PackageManifest | undefined>
  filesIndexFile: string
  hasBin: boolean
  hasBundledDependencies: boolean
  installable?: boolean
  isBuilt?: boolean
  optional: boolean
  optionalDependencies: Set<string>
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  requiresBuild?: boolean | any // this is a durty workaround added in https://github.com/pnpm/pnpm/pull/4898
  patchFile?: PatchFile
}

export interface DependenciesGraph {
  [depPath: string]: DependenciesGraphNode
}

export default function buildSequence (
  depGraph: Record<string, Pick<DependenciesGraphNode, 'children' | 'requiresBuild'>>,
  rootDepPaths: string[]
) {
  const nodesToBuild = new Set<string>()
  getSubgraphToBuild(depGraph, rootDepPaths, nodesToBuild, new Set<string>())
  const onlyFromBuildGraph = filter((depPath: string) => nodesToBuild.has(depPath))
  const nodesToBuildArray = Array.from(nodesToBuild)
  const graph = new Map(
    nodesToBuildArray
      .map((depPath) => [depPath, onlyFromBuildGraph(Object.values(depGraph[depPath].children))])
  )
  const graphSequencerResult = graphSequencer({
    graph,
    groups: [nodesToBuildArray],
  })
  const chunks = graphSequencerResult.chunks as string[][]
  return chunks
}

function getSubgraphToBuild (
  graph: Record<string, Pick<DependenciesGraphNode, 'children' | 'requiresBuild' | 'patchFile'>>,
  entryNodes: string[],
  nodesToBuild: Set<string>,
  walked: Set<string>
) {
  let currentShouldBeBuilt = false
  for (const depPath of entryNodes) {
    const node = graph[depPath]
    if (!node) continue // packages that are already in node_modules are skipped
    if (walked.has(depPath)) continue
    walked.add(depPath)
    const childShouldBeBuilt = getSubgraphToBuild(graph, Object.values(node.children), nodesToBuild, walked) ||
      node.requiresBuild ||
      node.patchFile != null
    if (childShouldBeBuilt) {
      nodesToBuild.add(depPath)
      currentShouldBeBuilt = true
    }
  }
  return currentShouldBeBuilt
}
