import type { DepPath } from '@pnpm/types'

import type { DepsGraph } from './types.js'

/**
 * Expand `builtDepPaths` to every node that is, or transitively depends
 * on, one of them.
 *
 * The result gates the engine string in {@link calcGraphNodeHash}: only a
 * package that may run a build script — or that depends on one — keeps the
 * engine in its GVS hash. It is computed as one graph-wide reverse closure,
 * so a package inside a dependency cycle gets the same answer no matter
 * which node the hasher reaches it from.
 *
 * A path with no node in `graph` is kept and still marks whatever depends on
 * it: the built set comes from the allowBuild policy rather than the graph,
 * so the two can disagree.
 */
export function computeBuildRequiredDepPaths (
  graph: DepsGraph<DepPath>,
  builtDepPaths: Set<DepPath>
): Set<DepPath> {
  const buildRequiredDepPaths = new Set(builtDepPaths)
  if (builtDepPaths.size === 0) return buildRequiredDepPaths

  const parentsByChild = buildParentsByChildMap(graph)
  expandReverseClosure(buildRequiredDepPaths, parentsByChild, Array.from(builtDepPaths))

  return buildRequiredDepPaths
}

function buildParentsByChildMap (graph: DepsGraph<DepPath>): Map<DepPath, DepPath[]> {
  const parentsByChild = new Map<DepPath, DepPath[]>()
  for (const parent of Object.keys(graph) as DepPath[]) {
    for (const child of Object.values(graph[parent].children)) {
      const parents = parentsByChild.get(child)
      if (parents == null) {
        parentsByChild.set(child, [parent])
      } else {
        parents.push(parent)
      }
    }
  }
  return parentsByChild
}

function expandReverseClosure (
  buildRequiredDepPaths: Set<DepPath>,
  parentsByChild: Map<DepPath, DepPath[]>,
  pending: DepPath[]
): void {
  while (pending.length > 0) {
    const child = pending.pop()!
    for (const parent of parentsByChild.get(child) ?? []) {
      if (!buildRequiredDepPaths.has(parent)) {
        buildRequiredDepPaths.add(parent)
        pending.push(parent)
      }
    }
  }
}
