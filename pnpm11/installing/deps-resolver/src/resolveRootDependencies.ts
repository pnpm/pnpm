import { getPreferredVersionsFromLockfileAndManifests } from '@pnpm/lockfile.preferred-versions'
import type { PreferredVersions } from '@pnpm/resolving.resolver-base'

import { getNonDevWantedDependencies } from './getNonDevWantedDependencies.js'
import type { WantedDependency } from './getWantedDependencies.js'
import { getHoistableRootDeps } from './hoistableRootDeps.js'
import { getHoistableOptionalPeers, getLockfileOnlyVersions, type HoistableRootDep, hoistPeers } from './hoistPeers.js'
import { addDirectDepVersion } from './indexResolvedDependencies.js'
import { collectMissingRequiredPeers, filterMissingPeers, mergePkgsDeps } from './missingPeers.js'
import {
  createCandidatePeerRangesLookup,
  getDirectDepVersions,
  peersAcceptProvidedVersions,
} from './optionalPeerCandidates.js'
import type {
  ImporterToResolve,
  MissingPeerInfo,
  MissingPeers,
  ParentPkgAliases,
  PkgAddressesByImportersWithoutPeers,
  ResolutionContext,
  ResolvedRootDependenciesResult,
} from './resolutionTypes.js'
import { resolveDependencies } from './resolveDependencies.js'
import { resolveDependenciesOfImporters } from './resolveDependenciesOfImporters.js'

/**
 * The state of hoisting the missing peers of every importer to that
 * importer, shared by the rounds that add the hoisted peers.
 */
interface PeerHoisting {
  ctx: ResolutionContext
  hoistedPeersUpdateDepths: number[]
  importerResults: PkgAddressesByImportersWithoutPeers[]
  importers: ImporterToResolve[]
  /** See `HoistPeersOptions.lockfileOnlyVersions`. */
  lockfileOnlyVersions?: Map<string, Set<string>>
  publishedBy?: Date
  /** The versions of the workspace root's direct dependencies. */
  rootDepVersions: Map<string, string>
  rootImporterIndex: number
  workspaceRootDeps: HoistableRootDep[]
}

type MissingOptionalPeerRanges = Record<string, string[]>

type CandidatePeerRangesLookup = ReturnType<typeof createCandidatePeerRangesLookup>

export async function resolveRootDependencies (
  ctx: ResolutionContext,
  importers: ImporterToResolve[]
): Promise<ResolvedRootDependenciesResult> {
  if (ctx.autoInstallPeers) {
    ctx.allPreferredVersions = getPreferredVersionsFromLockfileAndManifests(ctx.wantedLockfile.packages, [])
  } else if (ctx.hoistPeers) {
    // Null-prototype: keyed by package names from resolved manifests.
    ctx.allPreferredVersions = Object.create(null) as PreferredVersions
  }
  const { pkgAddressesByImportersWithoutPeers, publishedBy, time } = await resolveDependenciesOfImporters(ctx, importers)
  if (ctx.hoistPeers) {
    const hoisting = await startPeerHoisting(ctx, { importerResults: pkgAddressesByImportersWithoutPeers, importers, publishedBy })
    await hoistPeersToImporters(hoisting)
  }
  return {
    pkgAddressesByImporters: pkgAddressesByImportersWithoutPeers.map(({ pkgAddresses }) => pkgAddresses),
    time,
  }
}

async function startPeerHoisting (
  ctx: ResolutionContext,
  { importerResults, importers, publishedBy }: Pick<PeerHoisting, 'importerResults' | 'importers' | 'publishedBy'>
): Promise<PeerHoisting> {
  const hoisting: PeerHoisting = {
    ctx,
    hoistedPeersUpdateDepths: [],
    importerResults,
    importers,
    lockfileOnlyVersions: ctx.autoInstallPeers && ctx.allPreferredVersions != null
      ? getLockfileOnlyVersions(ctx.allPreferredVersions)
      : undefined,
    publishedBy,
    rootDepVersions: new Map<string, string>(),
    rootImporterIndex: -1,
    workspaceRootDeps: [],
  }
  if (ctx.resolvePeersFromWorkspaceRoot) {
    hoisting.rootImporterIndex = importers.findIndex(({ options }) => options.parentIds[0] === '.')
    const rootPkgAddresses = importerResults[hoisting.rootImporterIndex]?.pkgAddresses ?? []
    hoisting.workspaceRootDeps = await getHoistableRootDeps(importers[hoisting.rootImporterIndex], rootPkgAddresses)
    hoisting.rootDepVersions = getDirectDepVersions(ctx.resolvedPkgsById, rootPkgAddresses)
  }
  hoisting.hoistedPeersUpdateDepths = importers.map(getHoistedPeersUpdateDepth)
  return hoisting
}

async function hoistPeersToImporters (hoisting: PeerHoisting): Promise<void> {
  /* eslint-disable no-await-in-loop -- each round hoists the peers that the previous round found missing */
  while (true) {
    const allMissingOptionalPeersByImporters = await Promise.all(
      hoisting.importerResults.map(async (_, index) => hoistRequiredPeersOfImporter(hoisting, index))
    )
    const hasNewMissingPeers = await hoistOptionalPeersOfImporters(hoisting, allMissingOptionalPeersByImporters)
    if (!hasNewMissingPeers) break
  }
  /* eslint-enable no-await-in-loop */
}

/**
 * Hoists the missing required peers of the importer's dependencies until
 * none is left that can be hoisted. Returns the ranges of the missing
 * optional peers.
 */
async function hoistRequiredPeersOfImporter (hoisting: PeerHoisting, index: number): Promise<MissingOptionalPeerRanges> {
  const { ctx } = hoisting
  const importerResult = hoisting.importerResults[index]
  const { parentPkgAliases } = hoisting.importers[index]
  const hoistPeersToImporter = createImporterPeersHoister(hoisting, index)
  const allMissingOptionalPeers: MissingOptionalPeerRanges = {}
  while (true) {
    const missingRequiredPeers = collectMissingPeersOfImporter(ctx, { allMissingOptionalPeers, importerResult, parentPkgAliases })
    if (!missingRequiredPeers.length) break
    const dependencies = hoistPeersToImporter(missingRequiredPeers)
    if (!Object.keys(dependencies).length) break
    const wantedDependencies = getNonDevWantedDependencies({ dependencies })
      .map((wantedDependency) => ({ ...wantedDependency, updateDepth: hoisting.hoistedPeersUpdateDepths[index] }))

    // eslint-disable-next-line no-await-in-loop -- the next round depends on the peers this round resolves
    await resolveHoistedDependencies(hoisting, index, wantedDependencies)
  }
  return allMissingOptionalPeers
}

function createImporterPeersHoister (hoisting: PeerHoisting, index: number): (missingRequiredPeers: Array<[string, MissingPeerInfo]>) => Record<string, string> {
  const { ctx } = hoisting
  const { options } = hoisting.importers[index]
  // The importer is the manifest the hoisted peer is added to, so a local
  // override's `link:`/`file:` target is made relative to its directory,
  // exactly as it would be for a dependency the importer declares.
  return hoistPeers.bind(null, {
    autoInstallPeers: ctx.autoInstallPeers,
    allPreferredVersions: ctx.allPreferredVersions,
    isUpdateTarget: hoisting.hoistedPeersUpdateDepths[index] < 0 ? undefined : options.updateMatching,
    lockfileOnlyVersions: hoisting.lockfileOnlyVersions,
    workspaceRootDeps: hoisting.workspaceRootDeps,
    overrideBareSpecifier: ctx.overrideBareSpecifier == null
      ? undefined
      : (name, range) => ctx.overrideBareSpecifier!(name, range, options.prefix),
  })
}

/**
 * Collects the peers that the importer's dependencies miss. Records the
 * missing optional peers in `allMissingOptionalPeers` and returns the missing
 * required ones.
 */
function collectMissingPeersOfImporter (
  ctx: ResolutionContext,
  { allMissingOptionalPeers, importerResult, parentPkgAliases }: {
    allMissingOptionalPeers: MissingOptionalPeerRanges
    importerResult: PkgAddressesByImportersWithoutPeers
    parentPkgAliases: ParentPkgAliases
  }
): Array<[string, MissingPeerInfo]> {
  for (const pkgAddress of importerResult.pkgAddresses) {
    parentPkgAliases[pkgAddress.alias] = true
  }
  if (ctx.autoInstallPeers) {
    importerResult.missingPeers = mergePkgsDeps([
      importerResult.missingPeers,
      collectMissingRequiredPeers(ctx, importerResult.pkgAddresses),
    ], ctx)
  }
  const { missingOptionalPeers, missingRequiredPeers } = partitionMissingPeers(importerResult.missingPeers, parentPkgAliases)
  if (ctx.autoInstallPeers) {
    addResolvedPeerProviders(importerResult, parentPkgAliases)
  }
  for (const [missingOptionalPeerName, { range: missingOptionalPeerRange }] of missingOptionalPeers) {
    if (!allMissingOptionalPeers[missingOptionalPeerName]) {
      allMissingOptionalPeers[missingOptionalPeerName] = [missingOptionalPeerRange]
    } else if (!allMissingOptionalPeers[missingOptionalPeerName].includes(missingOptionalPeerRange)) {
      allMissingOptionalPeers[missingOptionalPeerName].push(missingOptionalPeerRange)
    }
  }
  return missingRequiredPeers
}

/**
 * Splits the missing peers into optional and required ones. The required
 * ones are about to be hoisted, so they are marked as provided by the
 * importer.
 */
function partitionMissingPeers (
  missingPeers: MissingPeers | undefined,
  parentPkgAliases: ParentPkgAliases
): { missingOptionalPeers: Array<[string, MissingPeerInfo]>, missingRequiredPeers: Array<[string, MissingPeerInfo]> } {
  const missingOptionalPeers: Array<[string, MissingPeerInfo]> = []
  const missingRequiredPeers: Array<[string, MissingPeerInfo]> = []
  for (const [peerName, peerInfo] of Object.entries(missingPeers ?? {})) {
    if (peerInfo.optional) {
      missingOptionalPeers.push([peerName, peerInfo])
      continue
    }
    missingRequiredPeers.push([peerName, peerInfo])
    parentPkgAliases[peerName] = true
  }
  return { missingOptionalPeers, missingRequiredPeers }
}

function addResolvedPeerProviders (importerResult: PkgAddressesByImportersWithoutPeers, parentPkgAliases: ParentPkgAliases): void {
  // All the missing peers should get installed in the root.
  // Otherwise, pending nodes will not work.
  // even those peers should be hoisted that are not autoinstalled
  for (const [resolvedPeerName, resolvedPeerAddress] of Object.entries(importerResult.resolvedPeers ?? {})) {
    if (!parentPkgAliases[resolvedPeerName]) {
      importerResult.pkgAddresses.push({
        ...resolvedPeerAddress,
        hoistedPeerProvider: true,
      })
    }
  }
}

/**
 * Resolves dependencies hoisted to the importer and adds them, and the peers
 * they still miss, to the importer's result.
 */
async function resolveHoistedDependencies (
  hoisting: PeerHoisting,
  index: number,
  wantedDependencies: Array<WantedDependency & { updateDepth?: number }>
): Promise<void> {
  const { ctx, publishedBy } = hoisting
  const importerResult = hoisting.importerResults[index]
  const { preferredVersions, parentPkgAliases, options } = hoisting.importers[index]
  const resolveDependenciesResult = await resolveDependencies(ctx, preferredVersions, wantedDependencies, {
    ...options,
    parentPkgAliases,
    publishedBy,
    updateToLatest: false,
  })
  importerResult.pkgAddresses.push(...resolveDependenciesResult.pkgAddresses)
  if (options.directDepVersions) {
    for (const pkgAddress of resolveDependenciesResult.pkgAddresses) {
      const resolvedPackage = ctx.resolvedPkgsById[pkgAddress.pkgId]
      if (!resolvedPackage) continue
      addDirectDepVersion(options.directDepVersions, resolvedPackage)
    }
  }
  Object.assign(importerResult,
    filterMissingPeers(await resolveDependenciesResult.resolvingPeers, parentPkgAliases)
  )
}

/**
 * Hoists the missing optional peers of every importer, the workspace root
 * first, as the other importers' providers resolve their own peers from the
 * root's direct dependencies. Returns whether any optional peer was hoisted.
 */
async function hoistOptionalPeersOfImporters (
  hoisting: PeerHoisting,
  allMissingOptionalPeersByImporters: MissingOptionalPeerRanges[]
): Promise<boolean> {
  const getCandidatePeerRanges = createCandidatePeerRangesLookup(hoisting.ctx)
  const hoistOptionalForImporter = async (index: number): Promise<boolean> => hoistOptionalPeersOfImporter(hoisting, {
    allMissingOptionalPeers: allMissingOptionalPeersByImporters[index],
    getCandidatePeerRanges,
    index,
  })
  let rootHoistedPeers = false
  if (hoisting.rootImporterIndex !== -1) {
    refreshRootDepVersions(hoisting)
    rootHoistedPeers = await hoistOptionalForImporter(hoisting.rootImporterIndex)
    refreshRootDepVersions(hoisting)
  }
  const hoistedPeersByImporter = await Promise.all(
    allMissingOptionalPeersByImporters.map(async (_, index) => {
      if (index === hoisting.rootImporterIndex) return false
      return hoistOptionalForImporter(index)
    })
  )
  return rootHoistedPeers || hoistedPeersByImporter.includes(true)
}

function refreshRootDepVersions (hoisting: PeerHoisting): void {
  hoisting.rootDepVersions = getDirectDepVersions(
    hoisting.ctx.resolvedPkgsById,
    hoisting.importerResults[hoisting.rootImporterIndex].pkgAddresses
  )
}

async function hoistOptionalPeersOfImporter (
  hoisting: PeerHoisting,
  { allMissingOptionalPeers, getCandidatePeerRanges, index }: {
    allMissingOptionalPeers: MissingOptionalPeerRanges
    getCandidatePeerRanges: CandidatePeerRangesLookup
    index: number
  }
): Promise<boolean> {
  const { ctx } = hoisting
  if (!Object.keys(allMissingOptionalPeers).length || !ctx.allPreferredVersions) return false
  // A hoisted provider resolves its own peers from the importer's direct
  // dependencies first, then from the workspace root's.
  const providedPeerVersions = new Map([
    ...(index === hoisting.rootImporterIndex ? [] : hoisting.rootDepVersions),
    ...getDirectDepVersions(ctx.resolvedPkgsById, hoisting.importerResults[index].pkgAddresses),
  ])
  const optionalDependencies = getHoistableOptionalPeers(
    allMissingOptionalPeers,
    ctx.allPreferredVersions,
    hoisting.workspaceRootDeps,
    (name, version) => peersAcceptProvidedVersions(getCandidatePeerRanges(name, version), providedPeerVersions)
  )
  if (!Object.keys(optionalDependencies).length) return false
  await resolveHoistedDependencies(hoisting, index, getNonDevWantedDependencies({ optionalDependencies }))
  return true
}

/**
 * An update that matches packages by name reaches the importer's peers hoisted
 * from its subtree. Any other update names only dependencies the importer
 * declares, and a hoisted peer is not one of them.
 */
function getHoistedPeersUpdateDepth ({ options, wantedDependencies }: ImporterToResolve): number {
  if (options.updateMatching == null || wantedDependencies.length === 0) return options.updateDepth
  return wantedDependencies.reduce(
    (updateDepth, wantedDependency) => Math.min(updateDepth, wantedDependency.updateDepth ?? options.updateDepth),
    Infinity
  )
}
