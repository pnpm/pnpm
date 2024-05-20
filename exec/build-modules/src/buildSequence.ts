import { graphSequencer } from '@pnpm/deps.graph-sequencer'
import { type PkgIdWithPatchHash, type DepPath, type PackageManifest, type PatchFile } from '@pnpm/types'
import filter from 'ramda/src/filter'

export interface DependenciesGraphNode<T extends string> {
  children: Record<string, T>
  depPath: DepPath
  pkgIdWithPatchHash: PkgIdWithPatchHash
  name: string
  dir: string
  fetchingBundledManifest?: () => Promise<PackageManifest | undefined>
  filesIndexFile?: string
  hasBin: boolean
  hasBundledDependencies: boolean
  installable?: boolean
  isBuilt?: boolean
  optional: boolean
  optionalDependencies: Set<string>
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  requiresBuild?: boolean | any // this is a dirty workaround added in https://github.com/pnpm/pnpm/pull/4898
  patchFile?: PatchFile
}

export type DependenciesGraph<T extends string> = Record<T, DependenciesGraphNode<T>>

export function buildSequence<T extends string> (
  depGraph: Record<string, Pick<DependenciesGraphNode<T>, 'children' | 'requiresBuild'>>,
  rootDepPaths: string[]
): T[][] {
  const nodesToBuild = new Set<string>()
  getSubgraphToBuild(depGraph, rootDepPaths, nodesToBuild, new Set<string>())
  const onlyFromBuildGraph = filter((depPath: string) => nodesToBuild.has(depPath))
  const nodesToBuildArray = Array.from(nodesToBuild)
  const graph = new Map(
    nodesToBuildArray
      .map((depPath) => [depPath, onlyFromBuildGraph(Object.values(depGraph[depPath].children))])
  )
  const graphSequencerResult = graphSequencer(graph, nodesToBuildArray)
  const chunks = graphSequencerResult.chunks as T[][]
  return chunks
}

function getSubgraphToBuild<T extends string> (
  graph: Record<string, Pick<DependenciesGraphNode<T>, 'children' | 'requiresBuild' | 'patchFile'>>,
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
