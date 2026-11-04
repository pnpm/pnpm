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
  let remaining = duplicates
  const merged: Record<DepPath, DepPath> = {}
  while (true) {
    const { depPathsMap, remainingDuplicates } = deduplicateDepPaths(remaining, depGraph)
    if (Object.keys(depPathsMap).length === 0) return merged
    rewriteChildren(depGraph, depPathsMap)
    for (const [collapsed, target] of Object.entries<DepPath>(merged)) {
      merged[collapsed as DepPath] = depPathsMap[target] ?? target
    }
    Object.assign(merged, depPathsMap)
    remaining = remainingDuplicates
  }
}

function rewriteChildren<Pkg extends PartialResolvedPackage> (
  depGraph: GenericDependenciesGraphWithResolvedChildren<Pkg>,
  depPathsMap: Record<DepPath, DepPath>
): void {
  for (const node of Object.values(depGraph)) {
    for (const [alias, child] of Object.entries<DepPath>(node.children)) {
      node.children[alias] = depPathsMap[child] ?? child
    }
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
