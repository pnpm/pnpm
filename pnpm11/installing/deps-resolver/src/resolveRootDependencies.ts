import { getPreferredVersionsFromLockfileAndManifests } from '@pnpm/lockfile.preferred-versions'
import type { PreferredVersions } from '@pnpm/resolving.resolver-base'
import type { ProjectId } from '@pnpm/types'

import { getNonDevWantedDependencies } from './getNonDevWantedDependencies.js'
import type { WantedDependency } from './getWantedDependencies.js'
import { getHoistableRootDeps } from './hoistableRootDeps.js'
import { getHoistableOptionalPeers, getLockfileOnlyVersions, getOptionalPeerSpecifier, type HoistableRootDep, hoistPeers } from './hoistPeers.js'
import { addDirectDepVersion } from './indexResolvedDependencies.js'
import { getImporterLockedPeerVersions } from './lockedPeerVersions.js'
import { collectMissingRequiredPeers, filterMissingPeers, mergePkgsDeps } from './missingPeers.js'
import {
  acceptsOptionalPeerCandidate,
  type CandidatePeers,
  createCandidatePeersLookup,
  createRangePeerResolutions,
  getDirectDepVersions,
  getManifestPeers,
  pickHoistedVersion,
  pickVersionsInClosure,
  type RangePeerResolutions,
} from './optionalPeerCandidates.js'
import type {
  ImporterToResolve,
  MissingPeerInfo,
  MissingPeers,
  ParentPkgAliases,
  PkgAddressesByImportersWithoutPeers,
  ResolutionContext,
  ResolvedDependenciesOptions,
  ResolvedRootDependenciesResult,
} from './resolutionTypes.js'
import { resolveDependencies, resolveDependencyManifestOnly } from './resolveDependencies.js'
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
  /** Each importer's `getImporterLockedPeerVersions`, read when its optional peers are first hoisted, keyed by importer index. */
  lockedPeerVersionsByImporter: Map<number, Map<string, Set<string>>>
  publishedBy?: Date
  /** The versions of the workspace root's direct dependencies. */
  rootDepVersions: Map<string, string>
  rootImporterIndex: number
  workspaceRootDeps: HoistableRootDep[]
}

type MissingOptionalPeerRanges = Record<string, string[]>

type CandidatePeersLookup = ReturnType<typeof createCandidatePeersLookup>

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
    lockedPeerVersionsByImporter: new Map(),
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
    // eslint-disable-next-line no-await-in-loop -- the next round depends on the peers this round resolves
    await resolveHoistedDependencies(hoisting, index, getHoistedRequiredPeers(hoisting, index, dependencies))
  }
  return allMissingOptionalPeers
}

function getHoistedRequiredPeers (
  hoisting: PeerHoisting,
  index: number,
  dependencies: Record<string, string>
): Array<WantedDependency & { updateDepth: number }> {
  return getNonDevWantedDependencies({ dependencies })
    .map((wantedDependency) => ({ ...wantedDependency, updateDepth: hoisting.hoistedPeersUpdateDepths[index] }))
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
  const { ctx } = hoisting
  const importerResult = hoisting.importerResults[index]
  const { preferredVersions, parentPkgAliases, options } = hoisting.importers[index]
  const resolveDependenciesResult = await resolveDependencies(ctx, preferredVersions, wantedDependencies, getHoistedDependencyOptions(hoisting, index))
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

function getHoistedDependencyOptions (hoisting: PeerHoisting, index: number): ResolvedDependenciesOptions {
  const { parentPkgAliases, options } = hoisting.importers[index]
  return {
    ...options,
    parentPkgAliases,
    publishedBy: hoisting.publishedBy,
    updateToLatest: false,
  }
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
  const getCandidatePeers = createCandidatePeersLookup(hoisting.ctx)
  const hoistOptionalForImporter = async (index: number): Promise<boolean> => hoistOptionalPeersOfImporter(hoisting, {
    allMissingOptionalPeers: allMissingOptionalPeersByImporters[index],
    getCandidatePeers,
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
  opts: OptionalPeersOfImporter
): Promise<boolean> {
  if (!Object.keys(opts.allMissingOptionalPeers).length || !hoisting.ctx.allPreferredVersions) return false
  const rangePeers = createRangePeerResolutions()
  let optionalDependencies = pickHoistableOptionalPeers(hoisting, opts, rangePeers)
  /* eslint-disable no-await-in-loop -- each pick may meet required peers whose ranges the previous pick did not reach */
  while (rangePeers.pending.size > 0) {
    await resolvePendingRangePeers(hoisting, opts.index, rangePeers)
    optionalDependencies = pickHoistableOptionalPeers(hoisting, opts, rangePeers)
  }
  /* eslint-enable no-await-in-loop */
  if (!Object.keys(optionalDependencies).length) return false
  for (const [name, version] of Object.entries(optionalDependencies)) {
    optionalDependencies[name] = getOptionalPeerSpecifier({
      name,
      version,
      workspaceRootDeps: hoisting.workspaceRootDeps,
      rootDepVersions: hoisting.rootDepVersions,
    })
  }
  await resolveHoistedDependencies(hoisting, opts.index, getNonDevWantedDependencies({ optionalDependencies }))
  return true
}

interface OptionalPeersOfImporter {
  allMissingOptionalPeers: MissingOptionalPeerRanges
  getCandidatePeers: CandidatePeersLookup
  index: number
}

/**
 * The optional peers to hoist to the importer. A candidate whose required
 * peers resolve from a range not in `rangePeers` yet is judged without them,
 * and the range is added to `rangePeers.pending`.
 */
function pickHoistableOptionalPeers (
  hoisting: PeerHoisting,
  { allMissingOptionalPeers, getCandidatePeers, index }: OptionalPeersOfImporter,
  rangePeers: RangePeerResolutions
): Record<string, string> {
  const { ctx } = hoisting
  // A hoisted provider resolves its own peers from the importer's direct
  // dependencies first, then from the workspace root's.
  const providedPeerVersions = new Map([
    ...(index === hoisting.rootImporterIndex ? [] : hoisting.rootDepVersions),
    ...getDirectDepVersions(ctx.resolvedPkgsById, hoisting.importerResults[index].pkgAddresses),
  ])
  const pickRequiredPeerVersions = createRequiredPeerVersionsPicker(hoisting, index, rangePeers)
  const getPeersOfCandidate = (name: string, version: string): CandidatePeers | undefined =>
    getCandidatePeers(name, version) ?? rangePeers.peers.get(`${name}@${version}`)
  return getHoistableOptionalPeers(
    allMissingOptionalPeers,
    ctx.dedupePeerDependents
      ? ctx.allPreferredVersions!
      : pickVersionsInClosure(ctx, {
        extraVersions: providedPeerVersions,
        names: new Set(Object.keys(allMissingOptionalPeers)),
        preferredVersions: ctx.allPreferredVersions!,
        roots: hoisting.importerResults[index].pkgAddresses,
      }),
    hoisting.workspaceRootDeps,
    (name, version) => acceptsOptionalPeerCandidate({
      getCandidatePeers: getPeersOfCandidate,
      pickRequiredPeerVersions,
      providedVersions: providedPeerVersions,
    }, { name, version }),
    {
      lockedVersions: getLockedPeerVersionsOfImporter(hoisting, index),
      lockfileOnlyVersions: hoisting.lockfileOnlyVersions,
    }
  )
}

async function resolvePendingRangePeers (hoisting: PeerHoisting, index: number, rangePeers: RangePeerResolutions): Promise<void> {
  const pending = [...rangePeers.pending]
  rangePeers.pending.clear()
  const { preferredVersions } = hoisting.importers[index]
  const options = getHoistedDependencyOptions(hoisting, index)
  await Promise.all(pending.map(async ([key, { name, range }]) => {
    const [wantedDependency] = getHoistedRequiredPeers(hoisting, index, { [name]: range })
    const manifest = await resolveDependencyManifestOnly(hoisting.ctx, preferredVersions, wantedDependency, options)
    rangePeers.versions.set(key, manifest?.version)
    if (manifest?.version != null) {
      rangePeers.peers.set(`${name}@${manifest.version}`, getManifestPeers(manifest))
    }
  }))
}

/**
 * The versions the missing required peers of a wave of packages would be
 * hoisted to the importer at. A peer that would not be hoisted, or resolves
 * from a range `rangePeers` has no version for, is left out. `undefined` when
 * a peer's ranges do not merge, so the peer would stay unmet. Each pick is
 * cached for the round.
 */
function createRequiredPeerVersionsPicker (
  hoisting: PeerHoisting,
  index: number,
  rangePeers: RangePeerResolutions
): (missingPeersOfWave: MissingPeers[]) => Map<string, string> | undefined {
  const pickVersion = createCachedRequiredPeerVersionPicker(hoisting, index, rangePeers)
  return (missingPeersOfWave) => {
    const merged = mergePkgsDeps(missingPeersOfWave, hoisting.ctx)
    if (dropsAnyPeer(missingPeersOfWave, merged)) return undefined
    const versions = new Map<string, string>()
    for (const [name, { range, optional }] of Object.entries(merged)) {
      if (optional) continue
      const version = pickVersion(name, range)
      if (version != null) versions.set(name, version)
    }
    return versions
  }
}

function createCachedRequiredPeerVersionPicker (
  hoisting: PeerHoisting,
  index: number,
  rangePeers: RangePeerResolutions
): (name: string, range: string) => string | undefined {
  const hoistPeersToImporter = createImporterPeersHoister(hoisting, index)
  const picked = new Map<string, string | undefined>()
  return (name, range) => {
    const key = `${name}@${range}`
    if (!picked.has(key)) {
      const spec = hoistPeersToImporter([[name, { range, optional: false }]])[name]
      picked.set(key, spec == null ? undefined : pickHoistedVersion(rangePeers, name, spec))
    }
    return picked.get(key)
  }
}

function dropsAnyPeer (missingPeersOfWave: MissingPeers[], merged: MissingPeers): boolean {
  return missingPeersOfWave.some((missingPeers) => Object.entries(missingPeers).some(([name, { optional }]) => !optional && merged[name] == null))
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

/**
 * Every importer the wanted lockfile does not list reads the same versions,
 * those of the whole lockfile, so they share one entry.
 */
function getLockedPeerVersionsOfImporter (hoisting: PeerHoisting, index: number): Map<string, Set<string>> {
  const { wantedLockfile } = hoisting.ctx
  const importerId = hoisting.importers[index].options.parentIds[0] as string as ProjectId
  const listed = wantedLockfile.importers != null && Object.hasOwn(wantedLockfile.importers, importerId)
  const cacheKey = listed ? index : UNLISTED_IMPORTER
  let versions = hoisting.lockedPeerVersionsByImporter.get(cacheKey)
  if (versions == null) {
    versions = getImporterLockedPeerVersions(wantedLockfile, importerId)
    hoisting.lockedPeerVersionsByImporter.set(cacheKey, versions)
  }
  return versions
}

const UNLISTED_IMPORTER = -1
