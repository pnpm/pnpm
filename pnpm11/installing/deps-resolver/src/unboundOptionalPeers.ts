import type { DepPath } from '@pnpm/types'

import type { UnboundOptionalPeers } from './depPathCompatibility.js'
import type {
  DependenciesByProjectId,
  GenericDependenciesGraphNodeWithResolvedChildren,
  GenericDependenciesGraphWithResolvedChildren,
  PartialResolvedPackage,
  ProjectToResolve,
} from './peersResolutionTypes.js'

/**
 * Every variant in a project's closure that leaves unbound one of the
 * project's `rejectedOptionalPeers`, with the names it leaves unbound.
 */
export function collectUnboundOptionalPeers<Pkg extends PartialResolvedPackage> (
  depGraph: GenericDependenciesGraphWithResolvedChildren<Pkg>,
  opts: {
    dependenciesByProjectId: DependenciesByProjectId
    projects: ProjectToResolve[]
  }
): UnboundOptionalPeers {
  const unboundOptionalPeers: UnboundOptionalPeers = new Map()
  for (const { id, rejectedOptionalPeers } of opts.projects) {
    if (!rejectedOptionalPeers?.size) continue
    for (const depPath of walkClosure(depGraph, opts.dependenciesByProjectId[id].values())) {
      recordUnboundPeerNames(unboundOptionalPeers, depPath, unboundPeerNames(depGraph[depPath], rejectedOptionalPeers))
    }
  }
  return unboundOptionalPeers
}

function recordUnboundPeerNames (unboundOptionalPeers: UnboundOptionalPeers, depPath: DepPath, names: string[]): void {
  if (names.length === 0) return
  const recorded = unboundOptionalPeers.get(depPath) ?? new Set<string>()
  for (const name of names) recorded.add(name)
  unboundOptionalPeers.set(depPath, recorded)
}

function * walkClosure<Pkg extends PartialResolvedPackage> (
  depGraph: GenericDependenciesGraphWithResolvedChildren<Pkg>,
  roots: Iterable<DepPath>
): Generator<DepPath> {
  const pending = [...roots]
  const visited = new Set<DepPath>(pending)
  for (let depPath = pending.pop(); depPath != null; depPath = pending.pop()) {
    const node = depGraph[depPath]
    if (node == null) continue
    yield depPath
    const children = Object.values(node.children).filter((child) => !visited.has(child))
    for (const child of children) visited.add(child)
    pending.push(...children)
  }
}

function unboundPeerNames (
  node: PartialResolvedPackage & GenericDependenciesGraphNodeWithResolvedChildren,
  peerNames: Set<string>
): string[] {
  return [...peerNames].filter((name) =>
    node.peers.taken[name]?.optional === true && !node.resolvedPeerNames.has(name)
  )
}
