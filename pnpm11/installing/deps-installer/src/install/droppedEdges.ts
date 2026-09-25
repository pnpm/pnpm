import * as dp from '@pnpm/deps.path'
import type { LockfileObject } from '@pnpm/lockfile.types'

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
 * Whether no surviving package resolves a peer through one of `dropped`.
 *
 * A dropped package that some package reaches as a peer is embedded in that
 * package's key, so removing it would rekey the dependent rather than only
 * prune. A package the same alias still provides at another version is no
 * such peer: the suffix names the version, not the alias. A peer suffix pnpm
 * shortened into a hash names nothing that can be ruled out.
 */
export function peerSuffixesAreIndependentOf (lockfile: LockfileObject, dropped: DroppedEdges): boolean {
  return Object.keys(lockfile.packages ?? {}).every((depPath) => {
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
