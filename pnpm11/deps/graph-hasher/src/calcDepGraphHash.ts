import { hashObject } from '@pnpm/crypto.object-hasher'
import { packageRootLinkTarget } from '@pnpm/deps.path'
import type { SupportedArchitectures } from '@pnpm/types'

import { createFullPkgId } from './lockfileToDepGraph.js'
import type { DepGraphHashContext, DepsGraph, DepsGraphNode, DepsStateCache } from './types.js'

export interface CalcDepGraphHashOptions<NodeId extends string> {
  depsGraph: DepsGraph<NodeId>
  cache: DepsStateCache
  parents: Set<string>
  depPath: NodeId
  context: DepGraphHashContext
}

export function calcDepGraphHash<NodeId extends string> ({
  depsGraph,
  cache,
  parents,
  depPath,
  context,
}: CalcDepGraphHashOptions<NodeId>): string {
  const cacheKey = `${context.cacheKeyPrefix}${depPath}`
  if (cache[cacheKey]) return cache[cacheKey]
  const node = depsGraph[depPath]
  if (!node) return ''

  const fullPkgId = resolveNodeFullPkgId(depPath, node, context.supportedArchitectures)
  const deps = calcChildHashes({ depsGraph, cache, parents, context, node, fullPkgId })

  cache[cacheKey] = hashObject({
    id: fullPkgId,
    deps,
  })
  return cache[cacheKey]
}

function resolveNodeFullPkgId<NodeId extends string> (
  depPath: NodeId,
  node: DepsGraphNode<NodeId>,
  supportedArchitectures?: SupportedArchitectures
): string {
  if (node.pkgIdWithPatchHash != null && node.resolution != null) {
    return createFullPkgId(node.pkgIdWithPatchHash, node.resolution, supportedArchitectures)
  }
  if (node.fullPkgId != null) {
    return node.fullPkgId
  }
  throw new Error(`fullPkgId or resolution metadata is not defined for ${depPath} in depsGraph`)
}

function calcChildHashes<NodeId extends string> ({
  depsGraph,
  cache,
  parents,
  context,
  node,
  fullPkgId,
}: {
  depsGraph: DepsGraph<NodeId>
  cache: DepsStateCache
  parents: Set<string>
  context: DepGraphHashContext
  node: DepsGraphNode<NodeId>
  fullPkgId: string
}): Record<string, string> {
  const deps: Record<string, string> = {}
  if (!Object.keys(node.children).length || parents.has(fullPkgId)) {
    return deps
  }
  const nextParents = new Set([...Array.from(parents), fullPkgId])
  for (const [alias, childId] of Object.entries(node.children)) {
    if (packageRootLinkTarget(childId) != null) continue
    deps[alias] = calcDepGraphHash({
      depsGraph,
      cache,
      parents: nextParents,
      depPath: childId,
      context,
    })
  }
  return deps
}

export function createDepGraphHashContext (supportedArchitectures?: SupportedArchitectures): DepGraphHashContext {
  return {
    supportedArchitectures,
    cacheKeyPrefix: supportedArchitectures == null ? '' : `\0architectures=${hashObject(supportedArchitectures)}\0`,
  }
}
