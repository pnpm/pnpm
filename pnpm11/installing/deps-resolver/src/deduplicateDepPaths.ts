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
  const { depPathsMap, remainingDuplicates } = deduplicateDepPaths(duplicates, depGraph)
  if (remainingDuplicates.length === duplicates.length) {
    return depPathsMap
  }
  for (const node of Object.values(depGraph)) {
    for (const [alias, childDepPath] of Object.entries<DepPath>(node.children)) {
      if (depPathsMap[childDepPath]) {
        node.children[alias] = depPathsMap[childDepPath]
      }
    }
  }
  if (Object.keys(depPathsMap).length > 0) {
    return {
      ...depPathsMap,
      ...deduplicateAll(depGraph, remainingDuplicates),
    }
  }
  return depPathsMap
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
    const unresolvedDepPaths = collapseCompatibleDepPaths(depGraph, { depPaths, depPathsMap, depCountSorter })
    if (unresolvedDepPaths.size) {
      remainingDuplicates.push(unresolvedDepPaths)
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
 * that remain unresolved.
 */
function collapseCompatibleDepPaths<Pkg extends PartialResolvedPackage> (
  depGraph: GenericDependenciesGraphWithResolvedChildren<Pkg>,
  opts: {
    depPaths: Set<DepPath>
    depPathsMap: Record<DepPath, DepPath>
    depCountSorter: DepPathSorter
  }
): Set<DepPath> {
  const unresolvedDepPaths = new Set(opts.depPaths.values())
  let currentDepPaths = [...opts.depPaths].sort(opts.depCountSorter)

  while (currentDepPaths.length) {
    const depPath1 = currentDepPaths.pop()!
    const nextDepPaths = []
    while (currentDepPaths.length) {
      const depPath2 = currentDepPaths.pop()!
      if (isCompatibleAndHasMoreDeps(depGraph, depPath1, depPath2)) {
        opts.depPathsMap[depPath2] = depPath1
        unresolvedDepPaths.delete(depPath1)
        unresolvedDepPaths.delete(depPath2)
      } else {
        nextDepPaths.push(depPath2)
      }
    }
    nextDepPaths.push(...currentDepPaths)
    currentDepPaths = nextDepPaths.sort(opts.depCountSorter)
  }
  return unresolvedDepPaths
}
