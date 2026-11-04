import type { DepPath } from '@pnpm/types'

import { isCompatibleAndHasMoreDeps, nodeDepsCount } from './depPathCompatibility.js'
import type {
  GenericDependenciesGraphWithResolvedChildren,
  PartialResolvedPackage,
} from './peersResolutionTypes.js'

type DepPathSorter = (depPath1: DepPath, depPath2: DepPath) => number

export function deduplicateAll<Pkg extends PartialResolvedPackage> (
  depGraph: GenericDependenciesGraphWithResolvedChildren<Pkg>,
  duplicates: Array<Set<DepPath>>
): Record<DepPath, DepPath> {
  const incoming = incomingEdges(depGraph)
  let remaining = duplicates
  const merged: Record<DepPath, DepPath> = {}
  while (true) {
    const { depPathsMap, remainingDuplicates } = deduplicateRound(remaining, depGraph)
    if (Object.keys(depPathsMap).length === 0) return merged
    rewriteChildren(depGraph, depPathsMap, incoming)
    for (const [collapsed, target] of Object.entries<DepPath>(merged)) {
      merged[collapsed as DepPath] = depPathsMap[target] ?? target
    }
    Object.assign(merged, depPathsMap)
    remaining = remainingDuplicates
  }
}

function deduplicateRound<Pkg extends PartialResolvedPackage> (
  duplicates: Array<Set<DepPath>>,
  depGraph: GenericDependenciesGraphWithResolvedChildren<Pkg>
): DeduplicateDepPathsResult {
  const result = deduplicateDepPaths(duplicates, depGraph)
  if (Object.keys(result.depPathsMap).length > 0) return result
  const depPathsMap = collapseCycle(depGraph, duplicates)
  return {
    depPathsMap,
    remainingDuplicates: result.remainingDuplicates.map(group => new Set([...group].filter(path => !depPathsMap[path]))),
  }
}

// A cycle has no leaf whose collapse can start the next round. Collapse one
// compatible pair to break that dependency, then settle its children again.
function collapseCycle<Pkg extends PartialResolvedPackage> (
  depGraph: GenericDependenciesGraphWithResolvedChildren<Pkg>,
  duplicates: Array<Set<DepPath>>
): Record<DepPath, DepPath> {
  const sorter = createDepCountSorter(depGraph)
  const groups = duplicates.filter(group => group.size > 1).sort((left, right) => {
    const leftId = depGraph[left.values().next().value!].pkgIdWithPatchHash
    const rightId = depGraph[right.values().next().value!].pkgIdWithPatchHash
    return leftId < rightId ? -1 : leftId > rightId ? 1 : 0
  })
  for (const group of groups) {
    const paths = [...group].sort(sorter)
    while (paths.length) {
      const largest = paths.pop()!
      const candidate = [...paths].reverse().find(path => isCompatibleAndHasMoreDeps(depGraph, largest, path))
      if (candidate) return { [candidate]: largest }
    }
  }
  return {}
}

type IncomingEdges = Map<DepPath, Array<[DepPath, string]>>

function incomingEdges<Pkg extends PartialResolvedPackage> (
  depGraph: GenericDependenciesGraphWithResolvedChildren<Pkg>
): IncomingEdges {
  const incoming: IncomingEdges = new Map()
  for (const node of Object.values(depGraph)) {
    for (const [alias, child] of Object.entries<DepPath>(node.children)) {
      const edges = incoming.get(child) ?? []
      edges.push([node.depPath, alias])
      incoming.set(child, edges)
    }
  }
  return incoming
}

function rewriteChildren<Pkg extends PartialResolvedPackage> (
  depGraph: GenericDependenciesGraphWithResolvedChildren<Pkg>,
  depPathsMap: Record<DepPath, DepPath>,
  incoming: IncomingEdges
): void {
  for (const [collapsed, target] of Object.entries<DepPath>(depPathsMap)) {
    const edges = incoming.get(collapsed as DepPath) ?? []
    incoming.delete(collapsed as DepPath)
    const targetEdges = incoming.get(target) ?? []
    for (const edge of edges) {
      const [parent, alias] = edge
      depGraph[parent].children[alias] = target
      targetEdges.push(edge)
    }
    incoming.set(target, targetEdges)
  }
}

interface DeduplicateDepPathsResult {
  depPathsMap: Record<DepPath, DepPath>
  remainingDuplicates: Array<Set<DepPath>>
}

function deduplicateDepPaths<Pkg extends PartialResolvedPackage> (
  duplicates: Array<Set<DepPath>>,
  depGraph: GenericDependenciesGraphWithResolvedChildren<Pkg>
): DeduplicateDepPathsResult {
  const depCountSorter = createDepCountSorter(depGraph)
  const depPathsMap: Record<DepPath, DepPath> = {}
  const remainingDuplicates: Array<Set<DepPath>> = []

  for (const depPaths of duplicates) {
    const survivingDepPaths = collapseCompatibleDepPaths(depGraph, { depPaths, depPathsMap, depCountSorter })
    if (survivingDepPaths.size > 1) {
      remainingDuplicates.push(survivingDepPaths)
    }
  }
  return {
    depPathsMap,
    remainingDuplicates,
  }
}

// The dep paths arrive in resolution order, which varies between platforms.
// Tie-break equal dependency counts on the dep path itself so the chosen
// collapse target is the same everywhere — when a variant is a subset of
// several incompatible larger variants, the winner must not hinge on
// iteration order, or the lockfile becomes machine-dependent.
function createDepCountSorter<Pkg extends PartialResolvedPackage> (
  depGraph: GenericDependenciesGraphWithResolvedChildren<Pkg>
): DepPathSorter {
  return (depPath1: DepPath, depPath2: DepPath) => {
    const countDiff = nodeDepsCount(depGraph[depPath1]) - nodeDepsCount(depGraph[depPath2])
    if (countDiff !== 0) return countDiff
    return depPath1 < depPath2 ? -1 : depPath1 > depPath2 ? 1 : 0
  }
}

/**
 * Maps every dep path of `depPaths` that a compatible variant with more
 * dependencies can replace into `depPathsMap`, and returns the dep paths
 * that survive the round.
 */
function collapseCompatibleDepPaths<Pkg extends PartialResolvedPackage> (
  depGraph: GenericDependenciesGraphWithResolvedChildren<Pkg>,
  opts: {
    depPaths: Set<DepPath>
    depPathsMap: Record<DepPath, DepPath>
    depCountSorter: DepPathSorter
  }
): Set<DepPath> {
  const survivingDepPaths = new Set(opts.depPaths.values())
  let currentDepPaths = [...opts.depPaths].sort(opts.depCountSorter)

  while (currentDepPaths.length) {
    const depPath1 = currentDepPaths.pop()!
    const nextDepPaths = []
    while (currentDepPaths.length) {
      const depPath2 = currentDepPaths.pop()!
      // Child variants must agree before a parent can collapse into its sibling.
      const sameChildren = Object.entries(depGraph[depPath2].children).every(
        ([alias, child]) => depGraph[depPath1].children[alias] === child
      )
      if (sameChildren && isCompatibleAndHasMoreDeps(depGraph, depPath1, depPath2)) {
        opts.depPathsMap[depPath2] = depPath1
        survivingDepPaths.delete(depPath2)
      } else {
        nextDepPaths.push(depPath2)
      }
    }
    nextDepPaths.push(...currentDepPaths)
    currentDepPaths = nextDepPaths.sort(opts.depCountSorter)
  }
  return survivingDepPaths
}
