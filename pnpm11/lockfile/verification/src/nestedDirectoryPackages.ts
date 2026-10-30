import { refToRelative } from '@pnpm/deps.path'
import type { PackageSnapshot, PackageSnapshots } from '@pnpm/lockfile.types'
import type { DepPath } from '@pnpm/types'

/**
 * A directory package that a package other than a directory depends on, such
 * as a directory an override or a package extension gives a registry package.
 * A project's check descends through directory packages only, so it never
 * reaches one.
 */
export interface NestedDirectoryPackage {
  depPath: DepPath
  snapshot: PackageSnapshot
  /** The ref a parent records it by. */
  ref: string
  /** The dependency paths of the packages that depend on it, directly or not. */
  ancestors: Set<DepPath>
  /**
   * The aliases of its ancestors, which supply its peers. Ancestors on
   * different paths add up, which only widens the peers the check takes as
   * supplied.
   */
  ancestorAliases: Record<string, true>
}

export interface GraphEdges {
  /**
   * Whether the walk follows `optionalDependencies` edges, which an install
   * without optional dependencies leaves out.
   */
  includesOptional: boolean
}

const nestedByPackages = new WeakMap<PackageSnapshots, Map<boolean, NestedDirectoryPackage[]>>()

export function findNestedDirectoryPackages (packages: PackageSnapshots, edges: GraphEdges): NestedDirectoryPackage[] {
  let byEdges = nestedByPackages.get(packages)
  if (byEdges == null) {
    byEdges = new Map()
    nestedByPackages.set(packages, byEdges)
  }
  let nested = byEdges.get(edges.includesOptional)
  if (nested == null) {
    nested = collectNestedDirectoryPackages(packages, edges)
    byEdges.set(edges.includesOptional, nested)
  }
  return nested
}

function collectNestedDirectoryPackages (packages: PackageSnapshots, edges: GraphEdges): NestedDirectoryPackage[] {
  if (!Object.values(packages).some(isDirectoryPackage)) return []
  const refs = nestedDirectoryRefs(packages, edges)
  if (refs.size === 0) return []
  const parents = parentsByDepPath(packages, edges)
  return Array.from(refs, ([depPath, ref]) => {
    const ancestors = ancestorsOf(parents, depPath)
    const ancestorAliases = Object.fromEntries(Array.from(ancestors).flatMap((ancestor) =>
      childrenOf(packages[ancestor], edges).map(({ alias }) => [alias, true as const])
    ))
    return { depPath, snapshot: packages[depPath], ref, ancestors, ancestorAliases }
  })
}

/** The ref of each nested directory package, by its dependency path. */
function nestedDirectoryRefs (packages: PackageSnapshots, edges: GraphEdges): Map<DepPath, string> {
  const refs = new Map<DepPath, string>()
  for (const parent of Object.values(packages)) {
    if (isDirectoryPackage(parent)) continue
    // A directory package is always locked by a `file:` ref.
    for (const { depPath, ref } of childrenOf(parent, edges, (ref) => ref.includes('file:'))) {
      if (!refs.has(depPath) && isDirectoryPackage(packages[depPath])) {
        refs.set(depPath, ref)
      }
    }
  }
  return refs
}

function isDirectoryPackage (snapshot: PackageSnapshot | undefined): boolean {
  return snapshot != null && 'directory' in (snapshot.resolution ?? {})
}

function childrenOf (
  snapshot: PackageSnapshot | undefined,
  edges: GraphEdges,
  refFilter?: (ref: string) => boolean
): Array<{ alias: string, ref: string, depPath: DepPath }> {
  const optional = edges.includesOptional ? snapshot?.optionalDependencies : undefined
  return [snapshot?.dependencies, optional].flatMap((deps) =>
    Object.entries(deps ?? {}).flatMap(([alias, ref]) => {
      if (refFilter != null && !refFilter(ref)) return []
      const depPath = refToRelative(ref, alias)
      return depPath == null ? [] : [{ alias, ref, depPath }]
    })
  )
}

function parentsByDepPath (packages: PackageSnapshots, edges: GraphEdges): Map<DepPath, DepPath[]> {
  const parents = new Map<DepPath, DepPath[]>()
  for (const [parentDepPath, parent] of Object.entries(packages) as Array<[DepPath, PackageSnapshot]>) {
    for (const { depPath } of childrenOf(parent, edges)) {
      const depParents = parents.get(depPath)
      if (depParents == null) {
        parents.set(depPath, [parentDepPath])
      } else {
        depParents.push(parentDepPath)
      }
    }
  }
  return parents
}

function ancestorsOf (parents: Map<DepPath, DepPath[]>, depPath: DepPath): Set<DepPath> {
  const ancestors = new Set<DepPath>()
  const queue = [depPath]
  for (let current = queue.pop(); current != null; current = queue.pop()) {
    for (const parent of parents.get(current) ?? []) {
      if (parent !== depPath && !ancestors.has(parent)) {
        ancestors.add(parent)
        queue.push(parent)
      }
    }
  }
  return ancestors
}
