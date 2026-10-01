import type { PkgResolutionId } from '@pnpm/resolving.resolver-base'
import { pickBy } from 'ramda'

import { safeIntersect } from './mergePeers.js'
import type {
  ChildrenByParentId,
  MissingPeers,
  ParentPkgAliases,
  PeerDependencies,
  PeersResolutionResult,
  PkgAddress,
  PostponedPeersResolutionFunction,
  ResolutionContext,
  ResolvedPackage,
  ResolvedPeers,
} from './resolutionTypes.js'

interface PeerGraphPackage {
  children: ChildrenByParentId[PkgResolutionId]
  childAliases: Set<string>
  requiredPeers: MissingPeers
}

interface PeerGraph {
  packages: Map<PkgResolutionId, PeerGraphPackage>
  peerNames: Set<string>
  providedAliases: Set<string>
}

type PeerGraphContext = Pick<ResolutionContext, 'childrenByParentId' | 'autoInstallPeersFromHighestMatch'> & {
  resolvedPkgsById: Record<PkgResolutionId, Pick<ResolvedPackage, 'peerDependencies'>>
}

type PeerGraphRoot = Pick<PkgAddress, 'alias' | 'pkgId'>

// Shared child resolution can omit peers supplied by another importer's ancestors.
// Discover required peers from the graph without waiting on child-resolution promises.
export function collectMissingRequiredPeers (
  ctx: PeerGraphContext,
  roots: PeerGraphRoot[]
): MissingPeers {
  const { packages, peerNames, providedAliases } = indexRequiredPeers(ctx, roots)
  const missingPeers: MissingPeers[] = []
  for (const { requiredPeers } of packages.values()) {
    missingPeers.push(pickBy((_, name) => !providedAliases.has(name), requiredPeers))
  }
  for (const peerName of peerNames) {
    if (!providedAliases.has(peerName)) continue
    missingPeers.push(...findRequestsOfPeerNotProvidedByParent(packages, roots, peerName))
  }
  return mergePkgsDeps(missingPeers, ctx)
}

function indexRequiredPeers (ctx: PeerGraphContext, roots: PeerGraphRoot[]): PeerGraph {
  const rootAliases = new Set(roots.map(({ alias }) => alias))
  const graph: PeerGraph = {
    packages: new Map(),
    peerNames: new Set(),
    providedAliases: new Set(),
  }
  const pending = roots.map(({ pkgId }) => pkgId)
  while (pending.length) {
    const pkgId = pending.pop()!
    if (graph.packages.has(pkgId)) continue
    const pkg = ctx.resolvedPkgsById[pkgId]
    if (!pkg) continue
    const children = ctx.childrenByParentId[pkgId] ?? []
    addPackageToPeerGraph(graph, { children, peerDependencies: pkg.peerDependencies, pkgId }, rootAliases)
    pending.push(...children.map(({ id }) => id))
  }
  return graph
}

function addPackageToPeerGraph (
  graph: PeerGraph,
  pkg: { children: ChildrenByParentId[PkgResolutionId], peerDependencies: PeerDependencies, pkgId: PkgResolutionId },
  rootAliases: Set<string>
): void {
  const requiredPeers: MissingPeers = pickBy(({ optional }, name) => !optional && !rootAliases.has(name), getMissingPeers(pkg.peerDependencies))
  graph.packages.set(pkg.pkgId, { children: pkg.children, childAliases: new Set(pkg.children.map(({ alias }) => alias)), requiredPeers })
  for (const name of Object.keys(requiredPeers)) graph.peerNames.add(name)
  for (const { alias } of pkg.children) graph.providedAliases.add(alias)
}

function findRequestsOfPeerNotProvidedByParent (
  packages: Map<PkgResolutionId, PeerGraphPackage>,
  roots: PeerGraphRoot[],
  peerName: string
): MissingPeers[] {
  const requests: MissingPeers[] = []
  const visited = new Set<PkgResolutionId>()
  const pending = roots.map(({ pkgId }) => pkgId)
  while (pending.length) {
    const pkgId = pending.pop()!
    if (visited.has(pkgId)) continue
    visited.add(pkgId)
    const pkg = packages.get(pkgId)
    if (!pkg) continue
    if (pkg.requiredPeers[peerName]) {
      requests.push({ [peerName]: pkg.requiredPeers[peerName] })
    }
    if (!pkg.childAliases.has(peerName)) {
      pending.push(...pkg.children.map(({ id }) => id))
    }
  }
  return requests
}

export function filterMissingPeersFromPkgAddresses (
  pkgAddresses: PkgAddress[],
  currentParentPkgAliases: ParentPkgAliases,
  resolvedPeers: ResolvedPeers
): PkgAddress[] {
  return pkgAddresses.map((pkgAddress) => ({
    ...pkgAddress,
    missingPeers: pickBy((_, peerName) => {
      if (!currentParentPkgAliases[peerName]) return true
      if (currentParentPkgAliases[peerName] !== true) {
        resolvedPeers[peerName] = currentParentPkgAliases[peerName] as PkgAddress
      }
      return false
    }, pkgAddress.missingPeers ?? {}),
  }))
}

export async function startResolvingPeers (
  {
    childrenResults,
    currentParentPkgAliases,
    parentPkgAliases,
    pkgAddresses,
    postponedPeersResolutionQueue,
    autoInstallPeersFromHighestMatch,
  }: {
    childrenResults: PeersResolutionResult[]
    currentParentPkgAliases: ParentPkgAliases
    parentPkgAliases: ParentPkgAliases
    pkgAddresses: PkgAddress[]
    postponedPeersResolutionQueue: PostponedPeersResolutionFunction[]
    autoInstallPeersFromHighestMatch: boolean
  }
): Promise<PeersResolutionResult> {
  const results = await Promise.all(
    postponedPeersResolutionQueue.map((postponedPeersResolution) => postponedPeersResolution(parentPkgAliases))
  )
  const resolvedPeers = [...childrenResults, ...results].reduce((acc, { resolvedPeers }) => Object.assign(acc, resolvedPeers), {})
  const allMissingPeers = mergePkgsDeps(
    [
      ...filterMissingPeersFromPkgAddresses(pkgAddresses, currentParentPkgAliases, resolvedPeers),
      ...childrenResults,
      ...results,
    ].map(({ missingPeers }) => missingPeers).filter(Boolean),
    { autoInstallPeersFromHighestMatch }
  )
  return {
    missingPeers: allMissingPeers,
    resolvedPeers,
  }
}

export function mergePkgsDeps (pkgsDeps: MissingPeers[], opts: { autoInstallPeersFromHighestMatch: boolean }): MissingPeers {
  const mergedPkgDeps = {} as MissingPeers
  for (const [name, { ranges, optional }] of Object.entries(groupPeerRanges(pkgsDeps))) {
    const intersection = safeIntersect(ranges)
    if (intersection) {
      mergedPkgDeps[name] = { range: intersection, optional }
    } else if (opts.autoInstallPeersFromHighestMatch) {
      mergedPkgDeps[name] = { range: ranges.join(' || '), optional }
    }
  }
  return mergedPkgDeps
}

function groupPeerRanges (pkgsDeps: MissingPeers[]): Record<string, { ranges: string[], optional: boolean }> {
  const groupedRanges: Record<string, { ranges: string[], optional: boolean }> = {}
  for (const deps of pkgsDeps) {
    for (const [name, { range, optional }] of Object.entries(deps)) {
      if (!groupedRanges[name]) {
        groupedRanges[name] = { ranges: [], optional }
      } else {
        groupedRanges[name].optional &&= optional
      }
      groupedRanges[name].ranges.push(range)
    }
  }
  return groupedRanges
}

export function filterMissingPeers (
  { missingPeers, resolvedPeers }: PeersResolutionResult,
  parentPkgAliases: ParentPkgAliases
): PeersResolutionResult {
  const newMissing = {} as MissingPeers
  for (const [peerName, peerVersion] of Object.entries(missingPeers)) {
    if (!parentPkgAliases[peerName]) {
      newMissing[peerName] = peerVersion
    } else if (parentPkgAliases[peerName] !== true) {
      resolvedPeers[peerName] = parentPkgAliases[peerName] as PkgAddress
    }
  }
  return {
    resolvedPeers,
    missingPeers: newMissing,
  }
}

// The materialized peer set is used (not the manifest's raw peerDependencies)
// so that peers implied by a peerDependenciesMeta-only declaration participate
// in missing-peer collection — and therefore in optional-peer hoisting — the
// same way explicitly declared peers do.
export function getMissingPeers (peerDependencies: PeerDependencies): MissingPeers {
  const missingPeers = {} as MissingPeers
  for (const [peerName, peerDep] of Object.entries(peerDependencies)) {
    missingPeers[peerName] = {
      range: peerDep.version,
      optional: peerDep.optional === true,
    }
  }
  return missingPeers
}
