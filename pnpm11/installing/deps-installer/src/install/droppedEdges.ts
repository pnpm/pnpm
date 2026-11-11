import * as dp from '@pnpm/deps.path'
import type { LockfileObject, PackageSnapshot } from '@pnpm/lockfile.types'
import type { DepPath } from '@pnpm/types'

/**
 * What the edges a fast update severed pointed at, each written the way a
 * peer suffix names a package: `name@version`, or a bare `name@` when the
 * reference pins no version a suffix segment can be compared against — a
 * link or a tarball, whose suffix segment carries the resolved manifest
 * version instead — which makes every suffix naming that name suspect.
 */
export type DroppedEdges = Set<string>

/** Record the edge from `alias` to `reference` as severed. */
export function recordDroppedEdge (dropped: DroppedEdges, alias: string, reference: string | undefined): void {
  dropped.add(droppedPeerId(alias, reference))
}

/**
 * Whether surviving peer contexts are independent of the removed subtrees.
 * Shared peer dependents can change bindings after deduplication even when
 * their provider remains reachable.
 */
export function peerContextsAreIndependentOf (lockfile: LockfileObject, dropped: DroppedEdges): boolean {
  return Object.entries(lockfile.packages ?? {}).every(([depPath, snapshot]) => {
    const { name } = dp.parse(depPath)
    if (name != null && Object.keys(snapshot.peerDependencies ?? {}).length > 0 &&
        (dropped.has(droppedPeerId(name, depPath)) || dropped.has(`${name}@`))) return false
    const { peersIndex } = dp.indexOfDepPathSuffix(depPath)
    return peersIndex === -1 || peerSuffixIsIndependentOf(depPath.substring(peersIndex), dropped)
  })
}

/**
 * The id a peer suffix would name the target of `alias` -> `reference` by.
 */
function droppedPeerId (alias: string, reference: string | undefined): string {
  const depPath = reference == null ? null : dp.refToRelative(reference, alias)
  if (depPath == null) return `${alias}@`
  const { name, version, registryName } = dp.parse(depPath)
  if (name == null) return `${alias}@`
  if (version == null) return `${name}@`
  return registryName == null ? `${name}@${version}` : `${name}@${registryName}:${version}`
}

function peerSuffixIsIndependentOf (peers: string, dropped: DroppedEdges): boolean {
  return peerIdsIn(peers).every((peerId) => {
    const versionIndex = peerId.indexOf('@', 1) + 1
    if (versionIndex === 0) return false
    return !dropped.has(peerId) && !dropped.has(peerId.substring(0, versionIndex))
  })
}

/**
 * The ids a peer suffix names, at every nesting depth: without `dedupePeers`
 * a peer is named by its whole dep path, and the peers that path pins are as
 * much a part of the dependent's key as the top-level ones. A `patch_hash=`
 * segment names no package.
 */
function peerIdsIn (peers: string): string[] {
  return peers
    .split('(')
    .flatMap((segment) => segment.split(')'))
    .filter((segment) => segment !== '' && !segment.startsWith('patch_hash='))
}

export function hasPeerContext (lockfile: LockfileObject, root: DepPath): boolean {
  for (const [, snapshot] of walkSnapshots(lockfile, [root])) {
    if (Object.keys(snapshot.peerDependencies ?? {}).length > 0) return true
  }
  return false
}

export function includeDroppedSubtrees (lockfile: LockfileObject, dropped: DroppedEdges): DroppedEdges {
  const affected = new Set(dropped)
  const roots = Object.keys(lockfile.packages ?? {}).filter((depPath) => {
    const { name } = dp.parse(depPath)
    return name != null && (dropped.has(droppedPeerId(name, depPath)) || dropped.has(`${name}@`))
  }) as DepPath[]
  for (const [depPath] of walkSnapshots(lockfile, roots)) {
    const { name } = dp.parse(depPath)
    if (name != null) recordDroppedEdge(affected, name, depPath)
  }
  return affected
}

function * walkSnapshots (lockfile: LockfileObject, roots: DepPath[]): Generator<[DepPath, PackageSnapshot]> {
  const pending = [...roots]
  const visited = new Set<DepPath>()
  while (pending.length > 0) {
    const depPath = pending.pop()!
    if (visited.has(depPath)) continue
    visited.add(depPath)
    const snapshot = lockfile.packages?.[depPath]
    if (snapshot == null) continue
    yield [depPath, snapshot]
    for (const child of childPaths(snapshot)) pending.push(child)
  }
}

function * childPaths (snapshot: PackageSnapshot): Generator<DepPath> {
  for (const dependencies of [snapshot.dependencies, snapshot.optionalDependencies]) {
    for (const [alias, reference] of Object.entries(dependencies ?? {})) {
      const child = dp.refToRelative(reference, alias)
      if (child != null) yield child
    }
  }
}
