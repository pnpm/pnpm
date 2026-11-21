import * as dp from '@pnpm/deps.path'
import { linkPathToPeerVersion } from '@pnpm/installing.deps-resolver'
import type { PackageSnapshot, PackageSnapshots, ResolvedDependencies } from '@pnpm/lockfile.fs'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile.utils'
import type { DepPath } from '@pnpm/types'

import {
  isCyclicLockfilePeer,
  recoverHashedPeerProviders,
} from './verifyLockfileExoticSubdepsPeersSearch.js'
import { lookupPackageSnapshot, ownStringProp } from './verifyLockfileExoticSubdepsScope.js'

const MAX_VISITED_SNAPSHOTS = 1024
const MAX_CHILD_INSPECTIONS = 65_536
const MAX_NAME_CHECKS = 262_144

export type CyclicPredicate = (node: DepPath, alias: string, peerNode: DepPath) => boolean

export type PeerCandidate =
  | { kind: 'depPath', depPath: DepPath, formatted?: string }
  | { kind: 'link', peerId: string }

export interface PeerSlot {
  peerName: string
  targets: PeerCandidate[]
}

export interface ParentPeerState {
  parentDepPath: DepPath
  parentSnapshot: PackageSnapshot
  bindings?: dp.PeerBindings
  resolvedProviders?: Set<string> | null
}

export interface PeerCheckContext {
  packages: PackageSnapshots
  allowedPeerProviders: Set<DepPath>
  dedupePeers: boolean
  cyclicPredicate?: CyclicPredicate | null
}

export function createParentPeerState (
  parentDepPath: DepPath,
  parentSnapshot: PackageSnapshot
): ParentPeerState {
  return {
    parentDepPath,
    parentSnapshot,
    bindings: dp.parsePeerBindings(parentDepPath),
  }
}

export function isPeerSatisfactionEdge (
  edge: { alias: string, childDepPath: DepPath, childSnapshot: PackageSnapshot | undefined },
  parent: ParentPeerState,
  peerCtx: PeerCheckContext
): boolean {
  const { alias, childDepPath, childSnapshot } = edge
  if (parent.bindings == null || !peerCtx.allowedPeerProviders.has(childDepPath)) {
    return false
  }
  if (ownStringProp(parent.parentSnapshot.peerDependencies, alias) == null) {
    return false
  }
  const providers = getOrResolveBoundProviders(parent, peerCtx)
  if (providers == null) return false
  if (providers.has(childDepPath) || providers.has(dp.removePeersSuffix(childDepPath))) {
    return true
  }
  if (!peerCtx.dedupePeers && !isCyclicLockfilePeer(peerCtx, { parentDepPath: parent.parentDepPath, alias, peerDepPath: childDepPath })) {
    return false
  }
  const childNameVer = snapshotPeerNameVer(childDepPath, childSnapshot)
  return childNameVer != null && providers.has(childNameVer)
}

function getOrResolveBoundProviders (
  parent: ParentPeerState,
  peerCtx: PeerCheckContext
): Set<string> | null {
  if (parent.resolvedProviders !== undefined) return parent.resolvedProviders
  const bindings = parent.bindings
  if (bindings == null) {
    parent.resolvedProviders = null
    return null
  }
  if (bindings.kind === 'explicit') {
    parent.resolvedProviders = new Set(dp.getPeerBindingProviders(bindings))
    return parent.resolvedProviders
  }
  const slots = collectPeerSlots(peerCtx.packages, parent.parentDepPath, parent.parentSnapshot)
  parent.resolvedProviders = (slots == null || slots.length === 0)
    ? null
    : recoverHashedPeerProviders(
      peerCtx,
      { parentDepPath: parent.parentDepPath, expectedHash: `(${bindings.hash})` },
      slots
    )
  return parent.resolvedProviders
}

export function snapshotPeerNameVer (
  depPath: DepPath,
  snapshot: PackageSnapshot | undefined
): string | undefined {
  const info = snapshot != null ? nameVerFromPkgSnapshot(depPath, snapshot) : dp.parse(depPath)
  if (!info.name || !info.version) return undefined
  const qualified = info.registryName != null ? `${info.registryName}:${info.version}` : info.version
  return `${info.name}@${qualified}`
}

function collectPeerSlots (
  packages: PackageSnapshots,
  parentDepPath: DepPath,
  parentSnapshot: PackageSnapshot
): PeerSlot[] | null {
  const peerNames = collectSnapshotPeerNames(parentSnapshot)
  if (peerNames.length === 0) return []
  const descendants = resolveTransitivePeerDescendants(packages, parentSnapshot, peerNames)
  if (descendants == null) return null

  const selfIdentity = dp.removePeersSuffix(parentDepPath)
  const selfName = dp.parse(dp.removeSuffix(parentDepPath)).name
  const result: PeerSlot[] = []
  for (const peerName of peerNames) {
    const targets = buildTargetsForPeer({ parentSnapshot, peerName, descendants, selfName, selfIdentity })
    if (targets.length > 0) result.push({ peerName, targets })
  }
  return result
}

function resolveTransitivePeerDescendants (
  packages: PackageSnapshots,
  parentSnapshot: PackageSnapshot,
  peerNames: string[]
): Map<string, PeerCandidate[]> | null {
  const transitiveSet = new Set(parentSnapshot.transitivePeerDependencies ?? [])
  const transitiveNames = peerNames.filter((name) => transitiveSet.has(name))
  return transitiveNames.length > 0
    ? collectDescendantTargets(packages, parentSnapshot, transitiveNames)
    : new Map<string, PeerCandidate[]>()
}

function buildTargetsForPeer (opts: {
  parentSnapshot: PackageSnapshot
  peerName: string
  descendants: Map<string, PeerCandidate[]>
  selfName: string | undefined
  selfIdentity: string
}): PeerCandidate[] {
  const { parentSnapshot, peerName, descendants, selfName, selfIdentity } = opts
  const targets: PeerCandidate[] = []
  const direct = resolveSnapshotPeerCandidate(parentSnapshot, peerName)
  if (direct != null) pushUniqueCandidate(targets, direct)
  for (const desc of descendants.get(peerName) ?? []) {
    pushUniqueCandidate(targets, desc)
  }
  return peerName === selfName
    ? targets.filter((item) => item.kind !== 'depPath' || dp.removePeersSuffix(item.depPath) !== selfIdentity)
    : targets
}

export function collectSnapshotPeerNames (snapshot: PackageSnapshot): string[] {
  const names = new Set<string>([
    ...Object.keys(snapshot.peerDependencies ?? {}),
    ...Object.keys(snapshot.peerDependenciesMeta ?? {}),
    ...(snapshot.transitivePeerDependencies ?? []),
  ])
  return Array.from(names).sort()
}

interface DescendantWalk {
  packages: PackageSnapshots
  transitiveNames: string[]
  peerIndexByName: Map<string, number>
  byPeer: Map<string, PeerCandidate[]>
  visited: Map<DepPath, Set<number>>
  pending: Array<[PackageSnapshot, Set<number>]>
  inspections: number
  nameChecks: number
}

function collectDescendantTargets (
  packages: PackageSnapshots,
  root: PackageSnapshot,
  transitiveNames: string[]
): Map<string, PeerCandidate[]> | null {
  const walk: DescendantWalk = {
    packages,
    transitiveNames,
    peerIndexByName: new Map(transitiveNames.map((name, idx) => [name, idx])),
    byPeer: new Map<string, PeerCandidate[]>(),
    visited: new Map<DepPath, Set<number>>(),
    pending: [[root, new Set(transitiveNames.map((_, idx) => idx))]],
    inspections: 0,
    nameChecks: 0,
  }
  while (walk.pending.length > 0) {
    if (!expandDescendantStep(walk, walk.pending.pop()!)) return null
  }
  return walk.byPeer
}

function expandDescendantStep (
  walk: DescendantWalk,
  [current, active]: [PackageSnapshot, Set<number>]
): boolean {
  for (const [alias, ref] of rawSnapshotEntries(current)) {
    walk.inspections += 1
    if (walk.inspections > MAX_CHILD_INSPECTIONS) return false
    const resolved = resolveChildSnapshot(walk.packages, alias, ref)
    if (resolved == null) continue
    const next = recordDescendantPeers(walk, { ...resolved, active })
    if (next == null || walk.visited.size > MAX_VISITED_SNAPSHOTS) return false
    if (next.size > 0) walk.pending.push([resolved.childSnap, next])
  }
  return true
}

function resolveChildSnapshot (
  packages: PackageSnapshots,
  alias: string,
  ref: unknown
): { childDepPath: DepPath, childSnap: PackageSnapshot } | undefined {
  if (typeof ref !== 'string') return undefined
  const childDepPath = dp.refToRelative(ref, alias)
  const childSnap = childDepPath != null ? lookupPackageSnapshot(packages, childDepPath) : undefined
  return (childDepPath != null && childSnap != null) ? { childDepPath, childSnap } : undefined
}

function recordDescendantPeers (
  walk: DescendantWalk,
  child: { childDepPath: DepPath, childSnap: PackageSnapshot, active: Set<number> }
): Set<number> | null {
  const { childDepPath, childSnap, active } = child
  const hits = collectChildPeerHits(walk, childSnap, active)
  if (hits == null) return null
  const next = new Set<number>()
  if (hits.matched.size === 0) return next
  const sortedIndices = Array.from(hits.matched).sort((a, b) => a - b)
  for (const idx of sortedIndices) {
    if (visitChildPeerHit(walk, { childDepPath, childSnap, idx, propagates: hits.propagated.has(idx) })) {
      next.add(idx)
    }
  }
  return next
}

function collectChildPeerHits (
  walk: DescendantWalk,
  childSnap: PackageSnapshot,
  active: Set<number>
): { matched: Set<number>, propagated: Set<number> } | null {
  const matched = new Set<number>()
  for (const [depAlias] of rawSnapshotEntries(childSnap)) {
    const idx = lookupChargedActivePeer(walk, depAlias, active)
    if (idx === false) return null
    if (idx != null) matched.add(idx)
  }
  const propagated = collectPropagatedPeerHits(walk, childSnap.transitivePeerDependencies, active)
  if (propagated == null) return null
  for (const idx of propagated) matched.add(idx)
  return { matched, propagated }
}

function collectPropagatedPeerHits (
  walk: DescendantWalk,
  transitiveNames: string[] | undefined,
  active: Set<number>
): Set<number> | null {
  const propagated = new Set<number>()
  for (const transName of transitiveNames ?? []) {
    const idx = lookupChargedActivePeer(walk, transName, active)
    if (idx === false) return null
    if (idx != null) propagated.add(idx)
  }
  return propagated
}

function lookupChargedActivePeer (
  walk: DescendantWalk,
  name: string,
  active: Set<number>
): number | undefined | false {
  walk.nameChecks += 1
  if (walk.nameChecks > MAX_NAME_CHECKS) return false
  const idx = walk.peerIndexByName.get(name)
  return (idx != null && active.has(idx)) ? idx : undefined
}

function visitChildPeerHit (
  walk: DescendantWalk,
  hit: { childDepPath: DepPath, childSnap: PackageSnapshot, idx: number, propagates: boolean }
): boolean {
  const { childDepPath, childSnap, idx, propagates } = hit
  if (walk.visited.get(childDepPath)?.has(idx)) return false
  const peerName = walk.transitiveNames[idx]
  const candidate = resolveSnapshotPeerCandidate(childSnap, peerName)
  if (candidate == null && !propagates) return false
  markVisitedPeerIndex(walk.visited, childDepPath, idx)
  if (candidate != null) recordPeerCandidateInMap(walk.byPeer, peerName, candidate)
  return propagates
}

function markVisitedPeerIndex (visited: Map<DepPath, Set<number>>, depPath: DepPath, idx: number): void {
  let seen = visited.get(depPath)
  if (seen == null) {
    seen = new Set<number>()
    visited.set(depPath, seen)
  }
  seen.add(idx)
}

function recordPeerCandidateInMap (
  byPeer: Map<string, PeerCandidate[]>,
  peerName: string,
  candidate: PeerCandidate
): void {
  let list = byPeer.get(peerName)
  if (list == null) {
    list = []
    byPeer.set(peerName, list)
  }
  pushUniqueCandidate(list, candidate)
}

function * rawSnapshotEntries (parent: PackageSnapshot): IterableIterator<[string, unknown]> {
  yield * entriesOfDeps(parent.dependencies)
  yield * entriesOfDeps(parent.optionalDependencies)
}

function * entriesOfDeps (deps: ResolvedDependencies | undefined): IterableIterator<[string, unknown]> {
  if (deps != null) yield * Object.entries(deps)
}

function resolveSnapshotPeerCandidate (
  snapshot: PackageSnapshot,
  peerName: string
): PeerCandidate | undefined {
  const ref = ownStringProp(snapshot.dependencies, peerName) ??
    ownStringProp(snapshot.optionalDependencies, peerName)
  if (ref == null) return undefined
  if (ref.startsWith('link:')) {
    return { kind: 'link', peerId: `${peerName}@${linkPathToPeerVersion(ref.slice(5))}` }
  }
  const depPath = dp.refToRelative(ref, peerName)
  return depPath != null ? { kind: 'depPath', depPath } : undefined
}

function pushUniqueCandidate (targets: PeerCandidate[], candidate: PeerCandidate): void {
  const exists = targets.some((item) =>
    item.kind === 'link'
      ? (candidate.kind === 'link' && item.peerId === candidate.peerId)
      : (candidate.kind === 'depPath' && item.depPath === candidate.depPath && item.formatted === candidate.formatted)
  )
  if (!exists) targets.push(candidate)
}
