import * as dp from '@pnpm/deps.path'
import { createCyclicPeerPredicate, type PeerGraphEntry } from '@pnpm/installing.deps-resolver'
import type { PackageSnapshot, PackageSnapshots } from '@pnpm/lockfile.fs'
import { nameVerFromPkgSnapshot } from '@pnpm/lockfile.utils'
import type { DepPath } from '@pnpm/types'

import type {
  CyclicPredicate,
  PeerCandidate,
  PeerCheckContext,
  PeerSlot,
} from './verifyLockfileExoticSubdepsPeers.js'
import {
  collectSnapshotPeerNames,
  snapshotPeerNameVer,
} from './verifyLockfileExoticSubdepsPeers.js'
import { lookupPackageSnapshot, ownStringProp } from './verifyLockfileExoticSubdepsScope.js'

const MAX_COMBINATIONS = 4096
const MAX_CYCLE_VISITS = 65_536

interface FormattedCandidate {
  provider: string | undefined
  segment: string
}

type SlotMapper = (peerName: string, candidate: PeerCandidate) => FormattedCandidate | undefined

export function recoverHashedPeerProviders (
  peerCtx: PeerCheckContext,
  target: { parentDepPath: DepPath, expectedHash: string },
  slots: PeerSlot[]
): Set<string> | null {
  const { parentDepPath, expectedHash } = target
  const mappers = buildCandidateMappers(peerCtx, parentDepPath)
  for (const mapCandidate of mappers) {
    const matched = searchCombinations(slots, expectedHash, mapCandidate)
    if (matched != null) return matched
  }
  if (peerCtx.dedupePeers) return null
  const mixedSlots = expandMixedCycleCutSlots(peerCtx, parentDepPath, slots)
  return mixedSlots != null
    ? searchCombinations(mixedSlots, expectedHash, mapDefaultCandidate)
    : null
}

export function isCyclicLockfilePeer (
  peerCtx: PeerCheckContext,
  edge: { parentDepPath: DepPath, alias: string, peerDepPath: DepPath }
): boolean {
  const isCyclic = getOrBuildCyclicPredicate(peerCtx)
  return isCyclic != null && isCyclic(edge.parentDepPath, edge.alias, edge.peerDepPath)
}

function mapDefaultCandidate (_peerName: string, candidate: PeerCandidate): FormattedCandidate {
  if (candidate.kind === 'link') {
    return { provider: undefined, segment: candidate.peerId }
  }
  if (candidate.formatted != null) {
    return { provider: dp.removePeersSuffix(candidate.depPath), segment: candidate.formatted }
  }
  return { provider: candidate.depPath, segment: candidate.depPath }
}

function mapBareCandidate (_peerName: string, candidate: PeerCandidate): FormattedCandidate {
  if (candidate.kind === 'link') {
    return { provider: undefined, segment: candidate.peerId }
  }
  const bare = dp.removePeersSuffix(candidate.depPath)
  return { provider: bare, segment: bare }
}

function createNameVerMapper (packages: PackageSnapshots): SlotMapper {
  return (_peerName, candidate) => {
    if (candidate.kind === 'link') {
      return { provider: undefined, segment: candidate.peerId }
    }
    const seg = snapshotPeerNameVer(candidate.depPath, lookupPackageSnapshot(packages, candidate.depPath))
    return seg != null ? { provider: dp.removePeersSuffix(candidate.depPath), segment: seg } : undefined
  }
}

function createPerPeerCyclicMapper (
  packages: PackageSnapshots,
  parentDepPath: DepPath,
  isCyclic: CyclicPredicate
): SlotMapper {
  return (peerName, candidate) => {
    if (candidate.kind === 'link') {
      return { provider: undefined, segment: candidate.peerId }
    }
    if (!isCyclic(parentDepPath, peerName, candidate.depPath)) {
      return { provider: candidate.depPath, segment: candidate.depPath }
    }
    const seg = snapshotPeerNameVer(candidate.depPath, lookupPackageSnapshot(packages, candidate.depPath))
    return seg != null ? { provider: dp.removePeersSuffix(candidate.depPath), segment: seg } : undefined
  }
}

function buildCandidateMappers (
  peerCtx: PeerCheckContext,
  parentDepPath: DepPath
): SlotMapper[] {
  const mappers: SlotMapper[] = [mapDefaultCandidate, mapBareCandidate]
  if (peerCtx.dedupePeers) {
    mappers.push(createNameVerMapper(peerCtx.packages))
    return mappers
  }
  const isCyclic = getOrBuildCyclicPredicate(peerCtx)
  if (isCyclic != null) {
    mappers.push(createPerPeerCyclicMapper(peerCtx.packages, parentDepPath, isCyclic))
  }
  return mappers
}

function getOrBuildCyclicPredicate (peerCtx: PeerCheckContext): CyclicPredicate | undefined {
  if (peerCtx.cyclicPredicate !== undefined) return peerCtx.cyclicPredicate ?? undefined
  const entries = buildPeerGraphEntries(peerCtx.packages)
  const built = createCyclicPeerPredicate(entries)
  peerCtx.cyclicPredicate = built ?? null
  return built
}

function buildPeerGraphEntries (packages: PackageSnapshots): Array<PeerGraphEntry<DepPath>> {
  const entries: Array<PeerGraphEntry<DepPath>> = []
  for (const [rawDepPath, snapshot] of Object.entries(packages)) {
    const depPath = rawDepPath as DepPath
    if (snapshot == null || dp.parsePeerBindings(depPath) == null) continue
    const packageName = nameVerFromPkgSnapshot(depPath, snapshot).name
    if (!packageName) continue
    entries.push({
      node: depPath,
      packageName,
      peers: collectSnapshotPeerGraphEdges(snapshot),
    })
  }
  return entries
}

function collectSnapshotPeerGraphEdges (
  snapshot: PackageSnapshot
): Array<readonly [string, DepPath | undefined]> {
  const edges: Array<readonly [string, DepPath | undefined]> = []
  const transitiveSet = new Set(snapshot.transitivePeerDependencies ?? [])
  for (const alias of collectSnapshotPeerNames(snapshot)) {
    const ref = ownStringProp(snapshot.dependencies, alias) ??
      ownStringProp(snapshot.optionalDependencies, alias)
    if (ref != null) {
      const target = ref.startsWith('link:') ? undefined : (dp.refToRelative(ref, alias) ?? undefined)
      edges.push([alias, target])
    } else if (transitiveSet.has(alias)) {
      edges.push([alias, undefined])
    }
  }
  return edges
}

function expandMixedCycleCutSlots (
  peerCtx: PeerCheckContext,
  parentDepPath: DepPath,
  slots: PeerSlot[]
): PeerSlot[] | null {
  const probe = createCycleProbe(peerCtx.packages, parentDepPath)
  let changed = false
  const expanded = slots.map((slot) => {
    const variants = [...slot.targets]
    const seen = new Set<string>(variants.map((item) => mapDefaultCandidate(slot.peerName, item).segment))
    for (const candidate of slot.targets) {
      if (candidate.kind !== 'depPath') continue
      if (appendCycleCutVariants(peerCtx, { parentDepPath, peerName: slot.peerName, depPath: candidate.depPath, variants, seen, probe })) {
        changed = true
      }
    }
    return { peerName: slot.peerName, targets: variants }
  })
  return changed ? expanded : null
}

function appendCycleCutVariants (
  peerCtx: PeerCheckContext,
  opts: {
    parentDepPath: DepPath
    peerName: string
    depPath: DepPath
    variants: PeerCandidate[]
    seen: Set<string>
    probe: CycleProbe
  }
): boolean {
  const { parentDepPath, peerName, depPath, variants, seen, probe } = opts
  const bare = dp.removePeersSuffix(depPath)
  const nameVer = snapshotPeerNameVer(depPath, lookupPackageSnapshot(peerCtx.packages, depPath))
  if (bare === depPath && (nameVer == null || nameVer === depPath)) return false
  const mayCut = isCyclicLockfilePeer(peerCtx, { parentDepPath, alias: peerName, peerDepPath: depPath }) ||
    mayReachPackageCycle(probe, depPath)
  if (!mayCut) return false
  return addDistinctCutForms(depPath, [nameVer, bare], { variants, seen })
}

function addDistinctCutForms (
  depPath: DepPath,
  forms: Array<string | undefined>,
  dest: { variants: PeerCandidate[], seen: Set<string> }
): boolean {
  let added = false
  for (const cutFormatted of forms) {
    if (cutFormatted != null && !dest.seen.has(cutFormatted)) {
      dest.seen.add(cutFormatted)
      dest.variants.push({ kind: 'depPath', depPath, formatted: cutFormatted })
      added = true
    }
  }
  return added
}

interface CycleProbe {
  packages: PackageSnapshots
  consumerBare: string
  remainingVisits: number
}

function createCycleProbe (packages: PackageSnapshots, consumerDepPath: DepPath): CycleProbe {
  return {
    packages,
    consumerBare: dp.removePeersSuffix(consumerDepPath),
    remainingVisits: MAX_CYCLE_VISITS,
  }
}

function mayReachPackageCycle (probe: CycleProbe, providerDepPath: DepPath): boolean {
  const providerBare = dp.removePeersSuffix(providerDepPath)
  const rootSnap = lookupPackageSnapshot(probe.packages, providerDepPath)
  if (rootSnap == null) return true
  const pending = collectDirectDepPaths(rootSnap)
  const visited = new Set<DepPath>()
  while (pending.length > 0) {
    const step = advanceCycleProbeStep(probe, pending.pop()!, { providerBare, visited, pending })
    if (step != null) return step
  }
  return false
}

function advanceCycleProbeStep (
  probe: CycleProbe,
  current: DepPath,
  ctx: { providerBare: string, visited: Set<DepPath>, pending: DepPath[] }
): boolean | undefined {
  const currentBare = dp.removePeersSuffix(current)
  if (currentBare === ctx.providerBare || currentBare === probe.consumerBare) return true
  if (ctx.visited.has(current)) return undefined
  if (probe.remainingVisits <= 0) return true
  probe.remainingVisits -= 1
  const snap = lookupPackageSnapshot(probe.packages, current)
  if (snap == null) return true
  ctx.visited.add(current)
  for (const child of collectDirectDepPaths(snap)) ctx.pending.push(child)
  return undefined
}

function collectDirectDepPaths (snapshot: PackageSnapshot): DepPath[] {
  const result: DepPath[] = []
  appendDepPathsFromRecord(result, snapshot.dependencies)
  appendDepPathsFromRecord(result, snapshot.optionalDependencies)
  return result
}

function appendDepPathsFromRecord (
  result: DepPath[],
  deps: Record<string, string> | undefined
): void {
  if (deps == null) return
  for (const [alias, ref] of Object.entries(deps)) {
    if (typeof ref !== 'string') continue
    const depPath = dp.refToRelative(ref, alias)
    if (depPath != null) result.push(depPath)
  }
}

interface SearchFrame {
  nextTarget: number
  omitted: number
  chosenLen: number
}

function searchCombinations (
  slots: PeerSlot[],
  expectedHash: string,
  mapCandidate: SlotMapper
): Set<string> | null {
  const mappedSlots = prepareMappedSlots(slots, mapCandidate)
  if (mappedSlots.length === 0) return null
  const chosen: FormattedCandidate[] = []
  const budget = { remaining: MAX_COMBINATIONS }
  for (let omitted = 0; omitted < mappedSlots.length; omitted++) {
    if (budget.remaining <= 0) break
    const matched = searchWithOmissions(mappedSlots, chosen, { omitted, expectedHash, budget })
    if (matched != null) return matched
  }
  return null
}

function prepareMappedSlots (slots: PeerSlot[], mapCandidate: SlotMapper): FormattedCandidate[][] {
  const mapped: FormattedCandidate[][] = []
  for (const slot of slots) {
    const deduped = deduplicateFormattedSlot(slot, mapCandidate)
    if (deduped.length > 0) mapped.push(deduped)
  }
  return mapped
}

function deduplicateFormattedSlot (slot: PeerSlot, mapCandidate: SlotMapper): FormattedCandidate[] {
  const seen = new Set<string>()
  const result: FormattedCandidate[] = []
  for (const candidate of slot.targets) {
    const formatted = mapCandidate(slot.peerName, candidate)
    if (formatted == null) continue
    const key = `${formatted.provider ?? ''}\0${formatted.segment}`
    if (!seen.has(key)) {
      seen.add(key)
      result.push(formatted)
    }
  }
  return result
}

function searchWithOmissions (
  slots: FormattedCandidate[][],
  chosen: FormattedCandidate[],
  ctx: { omitted: number, expectedHash: string, budget: { remaining: number } }
): Set<string> | null {
  const frames: SearchFrame[] = [{ nextTarget: 0, omitted: ctx.omitted, chosenLen: 0 }]
  while (ctx.budget.remaining > 0 && frames.length > 0) {
    const matched = stepSearchFrame(frames, slots, { chosen, ...ctx })
    if (matched != null) return matched
  }
  return null
}

function stepSearchFrame (
  frames: SearchFrame[],
  slots: FormattedCandidate[][],
  ctx: { chosen: FormattedCandidate[], expectedHash: string, budget: { remaining: number } }
): Set<string> | null {
  const depth = frames.length - 1
  const frame = frames[depth]
  ctx.chosen.length = frame.chosenLen
  if (frame.omitted > slots.length - depth) {
    frames.pop()
    return null
  }
  if (depth === slots.length) {
    frames.pop()
    return matchChosenCandidates(ctx.chosen, ctx.expectedHash, ctx.budget)
  }
  const next = advanceSearchFrame(frame, slots[depth], ctx.chosen)
  if (next != null) frames.push(next)
  else frames.pop()
  return null
}

function advanceSearchFrame (
  frame: SearchFrame,
  targets: FormattedCandidate[],
  chosen: FormattedCandidate[]
): SearchFrame | undefined {
  let omitted = frame.omitted
  if (frame.nextTarget < targets.length) {
    chosen.push(targets[frame.nextTarget])
  } else if (frame.nextTarget === targets.length && frame.omitted > 0) {
    omitted = frame.omitted - 1
  } else {
    return undefined
  }
  frame.nextTarget += 1
  return { nextTarget: 0, omitted, chosenLen: chosen.length }
}

function matchChosenCandidates (
  chosen: FormattedCandidate[],
  expectedHash: string,
  budget: { remaining: number }
): Set<string> | null {
  if (chosen.length === 0) return null
  budget.remaining -= 1
  const segments = chosen.map((entry) => entry.segment)
  if (dp.createPeerDepGraphHash(segments, 0) !== expectedHash) return null
  const providers = new Set<string>()
  for (const entry of chosen) {
    if (entry.provider != null) providers.add(entry.provider)
  }
  return providers
}
