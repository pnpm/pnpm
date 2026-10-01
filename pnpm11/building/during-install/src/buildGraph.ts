import type { LockfileResolution } from '@pnpm/lockfile.types'
import type { PatchInfo } from '@pnpm/patching.types'
import type { PkgRequestFetchResult } from '@pnpm/store.controller-types'
import type { DepPath, PkgIdWithPatchHash } from '@pnpm/types'
import { filter } from 'ramda'

export interface DependenciesGraphNode<NodeId extends string> {
  children: Record<string, NodeId>
  depPath: DepPath
  pkgIdWithPatchHash: PkgIdWithPatchHash
  name: string
  version: string
  dir: string
  /** The `node_modules` directory `dir` sits in. */
  modules: string
  fetching?: () => Promise<PkgRequestFetchResult>
  filesIndexFile?: string
  hasBin: boolean
  hasBundledDependencies: boolean
  installable?: boolean
  isBuilt?: boolean
  optional: boolean
  optionalDependencies: Set<string>
  // eslint-disable-next-line @typescript-eslint/no-explicit-any -- the graphs passed in by different installers do not agree on this field's type
  requiresBuild?: boolean | any // this is a dirty workaround added in https://github.com/pnpm/pnpm/pull/4898
  patch?: PatchInfo
  resolution?: LockfileResolution
}

export type DependenciesGraph<NodeId extends string> = Record<NodeId, DependenciesGraphNode<NodeId>>

export function buildGraph<NodeId extends string> (
  depGraph: Record<string, Pick<DependenciesGraphNode<NodeId>, 'children' | 'requiresBuild'>>,
  rootDepPaths: NodeId[]
): Map<NodeId, NodeId[]> {
  const nodesToBuild = new Set<NodeId>()
  getSubgraphToBuild(depGraph, rootDepPaths, nodesToBuild, new Set<NodeId>())
  const onlyFromBuildGraph = filter((depPath: NodeId) => nodesToBuild.has(depPath))
  const nodesToBuildArray = Array.from(nodesToBuild)
  return new Map(
    nodesToBuildArray
      .map((depPath) => [depPath, onlyFromBuildGraph(Object.values(depGraph[depPath].children))])
  )
}

function getSubgraphToBuild<NodeId extends string> (
  graph: Record<string, Pick<DependenciesGraphNode<NodeId>, 'children' | 'requiresBuild' | 'patch'>>,
  entryNodes: NodeId[],
  nodesToBuild: Set<NodeId>,
  walked: Set<NodeId>
): boolean {
  let currentShouldBeBuilt = false
  for (const depPath of entryNodes) {
    const node = graph[depPath]
    if (!node) continue // packages that are already in node_modules are skipped
    if (walked.has(depPath)) continue
    walked.add(depPath)
    const childShouldBeBuilt = getSubgraphToBuild(graph, Object.values(node.children), nodesToBuild, walked) ||
      node.requiresBuild ||
      node.patch != null
    if (childShouldBeBuilt) {
      nodesToBuild.add(depPath)
      currentShouldBeBuilt = true
    }
  }
  return currentShouldBeBuilt
}
