import path from 'path'
import {
  deprecationLogger,
  progressLogger,
  skippedOptionalDependencyLogger,
} from '@pnpm/core-loggers'
import { PnpmError } from '@pnpm/error'
import {
  type Lockfile,
  type PackageSnapshot,
  type ResolvedDependencies,
} from '@pnpm/lockfile-types'
import {
  nameVerFromPkgSnapshot,
  packageIdFromSnapshot,
  pkgSnapshotToResolution,
} from '@pnpm/lockfile-utils'
import { logger } from '@pnpm/logger'
import { pickRegistryForPackage } from '@pnpm/pick-registry-for-package'
import {
  type DirectoryResolution,
  DIRECT_DEP_SELECTOR_WEIGHT,
  type PreferredVersions,
  type Resolution,
  type WorkspacePackages,
} from '@pnpm/resolver-base'
import {
  type PkgRequestFetchResult,
  type PackageResponse,
  type StoreController,
} from '@pnpm/store-controller-types'
import {
  type AllowedDeprecatedVersions,
  type Dependencies,
  type PackageManifest,
  type PatchFile,
  type PeerDependenciesMeta,
  type ReadPackageHook,
  type Registries,
} from '@pnpm/types'
import * as dp from '@pnpm/dependency-path'
import normalizePath from 'normalize-path'
import exists from 'path-exists'
import pDefer from 'p-defer'
import pShare from 'promise-share'
import pickBy from 'ramda/src/pickBy'
import omit from 'ramda/src/omit'
import zipWith from 'ramda/src/zipWith'
import semver from 'semver'
import { encodePkgId } from './encodePkgId'
import { getNonDevWantedDependencies, type WantedDependency } from './getNonDevWantedDependencies'
import { safeIntersect } from './mergePeers'
import {
  createNodeId,
  nodeIdContainsSequence,
  nodeIdContains,
  splitNodeId,
} from './nodeIdUtils'
import { wantedDepIsLocallyAvailable } from './wantedDepIsLocallyAvailable'
import safePromiseDefer, { type SafePromiseDefer } from 'safe-promise-defer'

const dependencyResolvedLogger = logger('_dependency_resolved')

export function nodeIdToParents (
  nodeId: string,
  resolvedPackagesByDepPath: ResolvedPackagesByDepPath
) {
  return splitNodeId(nodeId).slice(1)
    .map((depPath) => {
      const { id, name, version } = resolvedPackagesByDepPath[depPath]
      return { id, name, version }
    })
}

// child nodeId by child alias name in case of non-linked deps
export interface ChildrenMap {
  [alias: string]: string
}

export type DependenciesTreeNode<T> = {
  children: (() => ChildrenMap) | ChildrenMap
  installable: boolean
} & ({
  resolvedPackage: T & { name: string, version: string }
  depth: number
} | {
  resolvedPackage: { name: string, version: string }
  depth: -1
})

export type DependenciesTree<T> = Map<
// a node ID is the join of the package's keypath with a colon
// E.g., a subdeps node ID which parent is `foo` will be
// registry.npmjs.org/foo/1.0.0:registry.npmjs.org/bar/1.0.0
string,
DependenciesTreeNode<T>
>

export type ResolvedPackagesByDepPath = Record<string, ResolvedPackage>

export interface LinkedDependency {
  isLinkedDependency: true
  optional: boolean
  depPath: string
  dev: boolean
  resolution: DirectoryResolution
  pkgId: string
  version: string
  name: string
  normalizedPref?: string
  alias: string
}

export interface PendingNode {
  alias: string
  nodeId: string
  resolvedPackage: ResolvedPackage
  depth: number
  installable: boolean
}

export interface ChildrenByParentDepPath {
  [depPath: string]: Array<{
    alias: string
    depPath: string
  }>
}

export interface ResolutionContext {
  autoInstallPeers: boolean
  allowBuild?: (pkgName: string) => boolean
  allowedDeprecatedVersions: AllowedDeprecatedVersions
  appliedPatches: Set<string>
  updatedSet: Set<string>
  defaultTag: string
  dryRun: boolean
  forceFullResolution: boolean
  ignoreScripts?: boolean
  resolvedPackagesByDepPath: ResolvedPackagesByDepPath
  outdatedDependencies: { [pkgId: string]: string }
  childrenByParentDepPath: ChildrenByParentDepPath
  patchedDependencies?: Record<string, PatchFile>
  pendingNodes: PendingNode[]
  wantedLockfile: Lockfile
  currentLockfile: Lockfile
  linkWorkspacePackagesDepth: number
  lockfileDir: string
  storeController: StoreController
  // the IDs of packages that are not installable
  skipped: Set<string>
  dependenciesTree: DependenciesTree<ResolvedPackage>
  force: boolean
  preferWorkspacePackages?: boolean
  readPackageHook?: ReadPackageHook
  engineStrict: boolean
  nodeVersion: string
  pnpmVersion: string
  registries: Registries
  resolutionMode?: 'highest' | 'time-based' | 'lowest-direct'
  virtualStoreDir: string
  workspacePackages?: WorkspacePackages
  missingPeersOfChildrenByPkgId: Record<string, { parentImporterId: string, missingPeersOfChildren: MissingPeersOfChildren }>
}

export type MissingPeers = Record<string, string>

export type ResolvedPeers = Record<string, PkgAddress>

interface MissingPeersOfChildren {
  resolve: (missingPeers: MissingPeers) => void
  reject: (err: Error) => void
  get: () => Promise<MissingPeers>
  resolved?: boolean
}

export type PkgAddress = {
  alias: string
  depIsLinked: boolean
  depPath: string
  isNew: boolean
  isLinkedDependency?: false
  nodeId: string
  pkgId: string
  normalizedPref?: string // is returned only for root dependencies
  installable: boolean
  pkg: PackageManifest
  version?: string
  updated: boolean
  rootDir: string
  missingPeers: MissingPeers
  missingPeersOfChildren?: MissingPeersOfChildren
  publishedAt?: string
  optional: boolean
} & ({
  isLinkedDependency: true
  version: string
} | {
  isLinkedDependency: undefined
})

export interface ResolvedPackage {
  id: string
  resolution: Resolution
  prod: boolean
  dev: boolean
  optional: boolean
  fetching: () => Promise<PkgRequestFetchResult>
  filesIndexFile: string
  name: string
  version: string
  peerDependencies: Dependencies
  peerDependenciesMeta?: PeerDependenciesMeta
  optionalDependencies: Set<string>
  hasBin: boolean
  hasBundledDependencies: boolean
  patchFile?: PatchFile
  prepare: boolean
  depPath: string
  requiresBuild: boolean | SafePromiseDefer<boolean>
  additionalInfo: {
    deprecated?: string
    bundleDependencies?: string[]
    bundledDependencies?: string[]
    engines?: {
      node?: string
      npm?: string
    }
    cpu?: string[]
    os?: string[]
    libc?: string[]
  }
  parentImporterIds: Set<string>
}

type ParentPkg = Pick<PkgAddress, 'nodeId' | 'installable' | 'depPath' | 'rootDir' | 'optional'>

export type ParentPkgAliases = Record<string, PkgAddress | true>

export type UpdateMatchingFunction = (pkgName: string) => boolean

interface ResolvedDependenciesOptions {
  currentDepth: number
  parentPkg: ParentPkg
  parentPkgAliases: ParentPkgAliases
  // If the package has been updated, the dependencies
  // which were used by the previous version are passed
  // via this option
  preferredDependencies?: ResolvedDependencies
  proceed: boolean
  publishedBy?: Date
  pickLowestVersion?: boolean
  resolvedDependencies?: ResolvedDependencies
  updateMatching?: UpdateMatchingFunction
  updateDepth: number
  prefix: string
}

interface PostponedResolutionOpts {
  preferredVersions: PreferredVersions
  parentPkgAliases: ParentPkgAliases
  publishedBy?: Date
}

interface PeersResolutionResult {
  missingPeers: MissingPeers
  resolvedPeers: ResolvedPeers
}

type PostponedResolutionFunction = (opts: PostponedResolutionOpts) => Promise<PeersResolutionResult>
type PostponedPeersResolutionFunction = (parentPkgAliases: ParentPkgAliases) => Promise<PeersResolutionResult>

interface ResolvedRootDependenciesResult {
  pkgAddressesByImporters: Array<Array<PkgAddress | LinkedDependency>>
  time?: Record<string, string>
}

export async function resolveRootDependencies (
  ctx: ResolutionContext,
  importers: ImporterToResolve[]
): Promise<ResolvedRootDependenciesResult> {
  const { pkgAddressesByImportersWithoutPeers, publishedBy, time } = await resolveDependenciesOfImporters(ctx, importers)
  const pkgAddressesByImporters = await Promise.all(zipWith(async (importerResolutionResult, { parentPkgAliases, preferredVersions, options }) => {
    const pkgAddresses = importerResolutionResult.pkgAddresses
    if (!ctx.autoInstallPeers) return pkgAddresses
    while (true) {
      for (const pkgAddress of importerResolutionResult.pkgAddresses) {
        parentPkgAliases[pkgAddress.alias] = true
      }
      for (const missingPeerName of Object.keys(importerResolutionResult.missingPeers ?? {})) {
        parentPkgAliases[missingPeerName] = true
      }
      // All the missing peers should get installed in the root.
      // Otherwise, pending nodes will not work.
      // even those peers should be hoisted that are not autoinstalled
      for (const [resolvedPeerName, resolvedPeerAddress] of Object.entries(importerResolutionResult.resolvedPeers ?? {})) {
        if (!parentPkgAliases[resolvedPeerName]) {
          pkgAddresses.push(resolvedPeerAddress)
        }
      }
      if (!Object.keys(importerResolutionResult.missingPeers).length) break
      const wantedDependencies = getNonDevWantedDependencies({ dependencies: importerResolutionResult.missingPeers })

      // eslint-disable-next-line no-await-in-loop
      const resolveDependenciesResult = await resolveDependencies(ctx, preferredVersions, wantedDependencies, {
        ...options,
        parentPkgAliases,
        publishedBy,
      })
      importerResolutionResult = {
        pkgAddresses: resolveDependenciesResult.pkgAddresses,
        // eslint-disable-next-line no-await-in-loop
        ...filterMissingPeers(await resolveDependenciesResult.resolvingPeers, parentPkgAliases),
      }
      pkgAddresses.push(...importerResolutionResult.pkgAddresses)
    }
    return pkgAddresses
  }, pkgAddressesByImportersWithoutPeers, importers))
  return { pkgAddressesByImporters, time }
}

interface ResolvedDependenciesResult {
  pkgAddresses: Array<PkgAddress | LinkedDependency>
  resolvingPeers: Promise<PeersResolutionResult>
}

interface PkgAddressesByImportersWithoutPeers extends PeersResolutionResult {
  pkgAddresses: Array<PkgAddress | LinkedDependency>
}

export interface ImporterToResolve {
  updatePackageManifest: boolean
  preferredVersions: PreferredVersions
  parentPkgAliases: ParentPkgAliases
  wantedDependencies: Array<WantedDependency & { updateDepth?: number }>
  options: Omit<ResolvedDependenciesOptions, 'parentPkgAliases' | 'publishedBy'>
}

interface ResolveDependenciesOfImportersResult {
  pkgAddressesByImportersWithoutPeers: PkgAddressesByImportersWithoutPeers[]
  publishedBy?: Date
  time?: Record<string, string>
}

async function resolveDependenciesOfImporters (
  ctx: ResolutionContext,
  importers: ImporterToResolve[]
): Promise<ResolveDependenciesOfImportersResult> {
  const extendedWantedDepsByImporters = importers.map(({ wantedDependencies, options }) => getDepsToResolve(wantedDependencies, ctx.wantedLockfile, {
    preferredDependencies: options.preferredDependencies,
    prefix: options.prefix,
    proceed: options.proceed || ctx.forceFullResolution,
    registries: ctx.registries,
    resolvedDependencies: options.resolvedDependencies,
  }))
  const pickLowestVersion = ctx.resolutionMode === 'time-based' || ctx.resolutionMode === 'lowest-direct'
  const resolveResults = await Promise.all(
    zipWith(async (extendedWantedDeps, importer) => {
      const postponedResolutionsQueue: PostponedResolutionFunction[] = []
      const postponedPeersResolutionQueue: PostponedPeersResolutionFunction[] = []
      const pkgAddresses: PkgAddress[] = []
      ;(await Promise.all(
        extendedWantedDeps.map((extendedWantedDep) => resolveDependenciesOfDependency(
          ctx,
          importer.preferredVersions,
          {
            ...importer.options,
            parentPkgAliases: importer.parentPkgAliases,
            pickLowestVersion: pickLowestVersion && !importer.updatePackageManifest,
          },
          extendedWantedDep
        ))
      )).forEach(({ resolveDependencyResult, postponedPeersResolution, postponedResolution }) => {
        if (resolveDependencyResult) {
          pkgAddresses.push(resolveDependencyResult as PkgAddress)
        }
        if (postponedResolution) {
          postponedResolutionsQueue.push(postponedResolution)
        }
        if (postponedPeersResolution) {
          postponedPeersResolutionQueue.push(postponedPeersResolution)
        }
      })
      return { pkgAddresses, postponedResolutionsQueue, postponedPeersResolutionQueue }
    }, extendedWantedDepsByImporters, importers)
  )
  let publishedBy: Date | undefined
  let time: Record<string, string> | undefined
  if (ctx.resolutionMode === 'time-based') {
    const result = getPublishedByDate(resolveResults.map(({ pkgAddresses }) => pkgAddresses).flat(), ctx.wantedLockfile.time)
    if (result.publishedBy) {
      publishedBy = new Date(result.publishedBy.getTime() + 60 * 60 * 1000) // adding 1 hour delta
      time = result.newTime
    }
  }
  const pkgAddressesByImportersWithoutPeers = await Promise.all(zipWith(async (importer, { pkgAddresses, postponedResolutionsQueue, postponedPeersResolutionQueue }) => {
    const newPreferredVersions = Object.create(importer.preferredVersions) as PreferredVersions
    const currentParentPkgAliases: Record<string, PkgAddress | true> = {}
    for (const pkgAddress of pkgAddresses) {
      if (currentParentPkgAliases[pkgAddress.alias] !== true) {
        currentParentPkgAliases[pkgAddress.alias] = pkgAddress
      }
      if (pkgAddress.updated) {
        ctx.updatedSet.add(pkgAddress.alias)
      }
      const resolvedPackage = ctx.resolvedPackagesByDepPath[pkgAddress.depPath]
      if (!resolvedPackage) continue // This will happen only with linked dependencies
      if (!Object.prototype.hasOwnProperty.call(newPreferredVersions, resolvedPackage.name)) {
        newPreferredVersions[resolvedPackage.name] = { ...importer.preferredVersions[resolvedPackage.name] }
      }
      if (!newPreferredVersions[resolvedPackage.name][resolvedPackage.version]) {
        newPreferredVersions[resolvedPackage.name][resolvedPackage.version] = {
          selectorType: 'version',
          weight: DIRECT_DEP_SELECTOR_WEIGHT,
        }
      }
    }
    const newParentPkgAliases = { ...importer.parentPkgAliases, ...currentParentPkgAliases }
    const postponedResolutionOpts = {
      preferredVersions: newPreferredVersions,
      parentPkgAliases: newParentPkgAliases,
      publishedBy,
    }
    const childrenResults = await Promise.all(
      postponedResolutionsQueue.map((postponedResolution) => postponedResolution(postponedResolutionOpts))
    )
    if (!ctx.autoInstallPeers) {
      return {
        missingPeers: {},
        pkgAddresses,
        resolvedPeers: {},
      }
    }
    const postponedPeersResolution = await Promise.all(
      postponedPeersResolutionQueue.map((postponedMissingPeers) => postponedMissingPeers(postponedResolutionOpts.parentPkgAliases))
    )
    const resolvedPeers = [...childrenResults, ...postponedPeersResolution].reduce((acc, { resolvedPeers }) => Object.assign(acc, resolvedPeers), {})
    const allMissingPeers = mergePkgsDeps(
      [
        ...filterMissingPeersFromPkgAddresses(pkgAddresses, currentParentPkgAliases, resolvedPeers),
        ...childrenResults,
        ...postponedPeersResolution,
      ].map(({ missingPeers }) => missingPeers).filter(Boolean)
    )
    return {
      missingPeers: allMissingPeers,
      pkgAddresses,
      resolvedPeers,
    }
  }, importers, resolveResults))
  return {
    pkgAddressesByImportersWithoutPeers,
    publishedBy,
    time,
  }
}

function filterMissingPeersFromPkgAddresses (
  pkgAddresses: PkgAddress[],
  currentParentPkgAliases: ParentPkgAliases,
  resolvedPeers: ResolvedPeers
): PkgAddress[] {
  return pkgAddresses.map((pkgAddress) => ({
    ...pkgAddress,
    missingPeers: pickBy((peer, peerName) => {
      if (!currentParentPkgAliases[peerName]) return true
      if (currentParentPkgAliases[peerName] !== true) {
        resolvedPeers[peerName] = currentParentPkgAliases[peerName] as PkgAddress
      }
      return false
    }, pkgAddress.missingPeers ?? {}),
  }))
}

function getPublishedByDate (pkgAddresses: PkgAddress[], timeFromLockfile: Record<string, string> = {}): { publishedBy: Date, newTime: Record<string, string> } {
  const newTime: Record<string, string> = {}
  for (const pkgAddress of pkgAddresses) {
    if (pkgAddress.publishedAt) {
      newTime[pkgAddress.depPath] = pkgAddress.publishedAt
    } else if (timeFromLockfile[pkgAddress.depPath]) {
      newTime[pkgAddress.depPath] = timeFromLockfile[pkgAddress.depPath]
    }
  }
  const sortedDates = Object.values(newTime)
    .map((publishedAt: string) => new Date(publishedAt))
    .sort((d1, d2) => d1.getTime() - d2.getTime())
  return { publishedBy: sortedDates[sortedDates.length - 1], newTime }
}

export async function resolveDependencies (
  ctx: ResolutionContext,
  preferredVersions: PreferredVersions,
  wantedDependencies: Array<WantedDependency & { updateDepth?: number }>,
  options: ResolvedDependenciesOptions
): Promise<ResolvedDependenciesResult> {
  const extendedWantedDeps = getDepsToResolve(wantedDependencies, ctx.wantedLockfile, {
    preferredDependencies: options.preferredDependencies,
    prefix: options.prefix,
    proceed: options.proceed || ctx.forceFullResolution,
    registries: ctx.registries,
    resolvedDependencies: options.resolvedDependencies,
  })
  const postponedResolutionsQueue: PostponedResolutionFunction[] = []
  const postponedPeersResolutionQueue: PostponedPeersResolutionFunction[] = []
  const pkgAddresses: PkgAddress[] = []
  ;(await Promise.all(
    extendedWantedDeps.map((extendedWantedDep) => resolveDependenciesOfDependency(
      ctx,
      preferredVersions,
      options,
      extendedWantedDep
    ))
  )).forEach(({ resolveDependencyResult, postponedResolution, postponedPeersResolution }) => {
    if (resolveDependencyResult) {
      pkgAddresses.push(resolveDependencyResult as PkgAddress)
    }
    if (postponedResolution) {
      postponedResolutionsQueue.push(postponedResolution)
    }
    if (postponedPeersResolution) {
      postponedPeersResolutionQueue.push(postponedPeersResolution)
    }
  })
  const newPreferredVersions = Object.create(preferredVersions) as PreferredVersions
  const currentParentPkgAliases: Record<string, PkgAddress | true> = {}
  for (const pkgAddress of pkgAddresses) {
    if (currentParentPkgAliases[pkgAddress.alias] !== true) {
      currentParentPkgAliases[pkgAddress.alias] = pkgAddress
    }
    if (pkgAddress.updated) {
      ctx.updatedSet.add(pkgAddress.alias)
    }
    const resolvedPackage = ctx.resolvedPackagesByDepPath[pkgAddress.depPath]
    if (!resolvedPackage) continue // This will happen only with linked dependencies
    if (!Object.prototype.hasOwnProperty.call(newPreferredVersions, resolvedPackage.name)) {
      newPreferredVersions[resolvedPackage.name] = { ...preferredVersions[resolvedPackage.name] }
    }
    if (!newPreferredVersions[resolvedPackage.name][resolvedPackage.version]) {
      newPreferredVersions[resolvedPackage.name][resolvedPackage.version] = 'version'
    }
  }
  const newParentPkgAliases = {
    ...options.parentPkgAliases,
    ...currentParentPkgAliases,
  }
  const postponedResolutionOpts = {
    preferredVersions: newPreferredVersions,
    parentPkgAliases: newParentPkgAliases,
    publishedBy: options.publishedBy,
  }
  const childrenResults = await Promise.all(
    postponedResolutionsQueue.map((postponedResolution) => postponedResolution(postponedResolutionOpts))
  )
  if (!ctx.autoInstallPeers) {
    return {
      resolvingPeers: Promise.resolve({
        missingPeers: {},
        resolvedPeers: {},
      }),
      pkgAddresses,
    }
  }
  return {
    pkgAddresses,
    resolvingPeers: startResolvingPeers({
      childrenResults,
      pkgAddresses,
      parentPkgAliases: options.parentPkgAliases,
      currentParentPkgAliases,
      postponedPeersResolutionQueue,
    }),
  }
}

async function startResolvingPeers (
  {
    childrenResults,
    currentParentPkgAliases,
    parentPkgAliases,
    pkgAddresses,
    postponedPeersResolutionQueue,
  }: {
    childrenResults: PeersResolutionResult[]
    currentParentPkgAliases: ParentPkgAliases
    parentPkgAliases: ParentPkgAliases
    pkgAddresses: PkgAddress[]
    postponedPeersResolutionQueue: PostponedPeersResolutionFunction[]
  }
) {
  const results = await Promise.all(
    postponedPeersResolutionQueue.map((postponedPeersResolution) => postponedPeersResolution(parentPkgAliases))
  )
  const resolvedPeers = [...childrenResults, ...results].reduce((acc, { resolvedPeers }) => Object.assign(acc, resolvedPeers), {})
  const allMissingPeers = mergePkgsDeps(
    [
      ...filterMissingPeersFromPkgAddresses(pkgAddresses, currentParentPkgAliases, resolvedPeers),
      ...childrenResults,
      ...results,
    ].map(({ missingPeers }) => missingPeers).filter(Boolean)
  )
  return {
    missingPeers: allMissingPeers,
    resolvedPeers,
  }
}

function mergePkgsDeps (pkgsDeps: Array<Record<string, string>>): Record<string, string> {
  const groupedRanges: Record<string, string[]> = {}
  for (const deps of pkgsDeps) {
    for (const [name, range] of Object.entries(deps)) {
      if (!groupedRanges[name]) {
        groupedRanges[name] = []
      }
      groupedRanges[name].push(range)
    }
  }
  const mergedPkgDeps = {} as Record<string, string>
  for (const [name, ranges] of Object.entries(groupedRanges)) {
    const intersection = safeIntersect(ranges)
    if (intersection) {
      mergedPkgDeps[name] = intersection
    }
  }
  return mergedPkgDeps
}

interface ExtendedWantedDependency {
  infoFromLockfile?: InfoFromLockfile
  proceed: boolean
  wantedDependency: WantedDependency & { updateDepth?: number }
}

interface ResolveDependenciesOfDependency {
  postponedResolution?: PostponedResolutionFunction
  postponedPeersResolution?: PostponedPeersResolutionFunction
  resolveDependencyResult: ResolveDependencyResult
}

async function resolveDependenciesOfDependency (
  ctx: ResolutionContext,
  preferredVersions: PreferredVersions,
  options: ResolvedDependenciesOptions,
  extendedWantedDep: ExtendedWantedDependency
): Promise<ResolveDependenciesOfDependency> {
  const updateDepth = typeof extendedWantedDep.wantedDependency.updateDepth === 'number'
    ? extendedWantedDep.wantedDependency.updateDepth
    : options.updateDepth
  const updateShouldContinue = options.currentDepth <= updateDepth
  const update = ((extendedWantedDep.infoFromLockfile?.dependencyLockfile) == null) ||
  (
    updateShouldContinue && (
      (options.updateMatching == null) ||
      options.updateMatching(extendedWantedDep.infoFromLockfile.name!)
    )
  ) || Boolean(
    (ctx.workspacePackages != null) &&
    ctx.linkWorkspacePackagesDepth !== -1 &&
    wantedDepIsLocallyAvailable(
      ctx.workspacePackages,
      extendedWantedDep.wantedDependency,
      { defaultTag: ctx.defaultTag, registry: ctx.registries.default }
    )
  ) || ctx.updatedSet.has(extendedWantedDep.infoFromLockfile.name!)

  const resolveDependencyOpts: ResolveDependencyOptions = {
    currentDepth: options.currentDepth,
    parentPkg: options.parentPkg,
    parentPkgAliases: options.parentPkgAliases,
    preferredVersions,
    currentPkg: extendedWantedDep.infoFromLockfile ?? undefined,
    pickLowestVersion: options.pickLowestVersion,
    prefix: options.prefix,
    proceed: extendedWantedDep.proceed || updateShouldContinue || ctx.updatedSet.size > 0,
    publishedBy: options.publishedBy,
    update,
    updateDepth,
    updateMatching: options.updateMatching,
  }
  const resolveDependencyResult = await resolveDependency(extendedWantedDep.wantedDependency, ctx, resolveDependencyOpts)

  if (resolveDependencyResult == null) return { resolveDependencyResult: null }
  if (resolveDependencyResult.isLinkedDependency) {
    ctx.dependenciesTree.set(createNodeIdForLinkedLocalPkg(ctx.lockfileDir, resolveDependencyResult.resolution.directory), {
      children: {},
      depth: -1,
      installable: true,
      resolvedPackage: {
        name: resolveDependencyResult.name,
        version: resolveDependencyResult.version,
      },
    })
    return { resolveDependencyResult }
  }
  if (!resolveDependencyResult.isNew) {
    return {
      resolveDependencyResult,
      postponedPeersResolution: resolveDependencyResult.missingPeersOfChildren != null
        ? async (parentPkgAliases) => {
          const missingPeers = await resolveDependencyResult.missingPeersOfChildren!.get()
          return filterMissingPeers({ missingPeers, resolvedPeers: {} }, parentPkgAliases)
        }
        : undefined,
    }
  }

  const postponedResolution = resolveChildren.bind(null, ctx, {
    parentPkg: resolveDependencyResult,
    dependencyLockfile: extendedWantedDep.infoFromLockfile?.dependencyLockfile,
    parentDepth: options.currentDepth,
    updateDepth,
    prefix: options.prefix,
    updateMatching: options.updateMatching,
  })
  return {
    resolveDependencyResult,
    postponedResolution: async (postponedResolutionOpts) => {
      const { missingPeers, resolvedPeers } = await postponedResolution(postponedResolutionOpts)
      if (resolveDependencyResult.missingPeersOfChildren) {
        resolveDependencyResult.missingPeersOfChildren.resolved = true
        resolveDependencyResult.missingPeersOfChildren.resolve(missingPeers)
      }
      return filterMissingPeers({ missingPeers, resolvedPeers }, postponedResolutionOpts.parentPkgAliases)
    },
  }
}

export function createNodeIdForLinkedLocalPkg (lockfileDir: string, pkgDir: string) {
  return `link:${normalizePath(path.relative(lockfileDir, pkgDir))}`
}

function filterMissingPeers (
  { missingPeers, resolvedPeers }: PeersResolutionResult,
  parentPkgAliases: ParentPkgAliases
): PeersResolutionResult {
  const newMissing = {} as MissingPeers
  for (const [peerName, peerVersion] of Object.entries(missingPeers)) {
    if (parentPkgAliases[peerName]) {
      if (parentPkgAliases[peerName] !== true) {
        resolvedPeers[peerName] = parentPkgAliases[peerName] as PkgAddress
      }
    } else {
      newMissing[peerName] = peerVersion
    }
  }
  return {
    resolvedPeers,
    missingPeers: newMissing,
  }
}

async function resolveChildren (
  ctx: ResolutionContext,
  {
    parentPkg,
    dependencyLockfile,
    parentDepth,
    updateDepth,
    updateMatching,
    prefix,
  }: {
    parentPkg: PkgAddress
    dependencyLockfile: PackageSnapshot | undefined
    parentDepth: number
    updateDepth: number
    prefix: string
    updateMatching?: UpdateMatchingFunction
  },
  {
    parentPkgAliases,
    preferredVersions,
    publishedBy,
  }: {
    parentPkgAliases: ParentPkgAliases
    preferredVersions: PreferredVersions
    publishedBy?: Date
  }
) {
  const currentResolvedDependencies = (dependencyLockfile != null)
    ? {
      ...dependencyLockfile.dependencies,
      ...dependencyLockfile.optionalDependencies,
    }
    : undefined
  const resolvedDependencies = parentPkg.updated
    ? undefined
    : currentResolvedDependencies
  const parentDependsOnPeer = Boolean(
    Object.keys(
      dependencyLockfile?.peerDependencies ??
      parentPkg.pkg.peerDependencies ??
      {}
    ).length
  )
  const wantedDependencies = getNonDevWantedDependencies(parentPkg.pkg)
  const {
    pkgAddresses,
    resolvingPeers,
  } = await resolveDependencies(ctx, preferredVersions, wantedDependencies,
    {
      currentDepth: parentDepth + 1,
      parentPkg,
      parentPkgAliases,
      preferredDependencies: currentResolvedDependencies,
      prefix,
      // If the package is not linked, we should also gather information about its dependencies.
      // After linking the package we'll need to symlink its dependencies.
      proceed: !parentPkg.depIsLinked || parentDependsOnPeer,
      publishedBy,
      resolvedDependencies,
      updateDepth,
      updateMatching,
    }
  )
  ctx.childrenByParentDepPath[parentPkg.depPath] = pkgAddresses.map((child) => ({
    alias: child.alias,
    depPath: child.depPath,
  }))
  ctx.dependenciesTree.set(parentPkg.nodeId, {
    children: pkgAddresses.reduce((chn, child) => {
      chn[child.alias] = (child as PkgAddress).nodeId ?? child.pkgId
      return chn
    }, {} as Record<string, string>),
    depth: parentDepth,
    installable: parentPkg.installable,
    resolvedPackage: ctx.resolvedPackagesByDepPath[parentPkg.depPath],
  })
  return resolvingPeers
}

function getDepsToResolve (
  wantedDependencies: Array<WantedDependency & { updateDepth?: number }>,
  wantedLockfile: Lockfile,
  options: {
    preferredDependencies?: ResolvedDependencies
    prefix: string
    proceed: boolean
    registries: Registries
    resolvedDependencies?: ResolvedDependencies
  }
): ExtendedWantedDependency[] {
  const resolvedDependencies = options.resolvedDependencies ?? {}
  const preferredDependencies = options.preferredDependencies ?? {}
  const extendedWantedDeps: ExtendedWantedDependency[] = []
  // The only reason we resolve children in case the package depends on peers
  // is to get information about the existing dependencies, so that they can
  // be merged with the resolved peers.
  let proceedAll = options.proceed
  const satisfiesWanted2Args = referenceSatisfiesWantedSpec.bind(null, {
    lockfile: wantedLockfile,
    prefix: options.prefix,
  })
  for (const wantedDependency of wantedDependencies) {
    let reference = undefined as undefined | string
    let proceed = proceedAll
    if (wantedDependency.alias) {
      const satisfiesWanted = satisfiesWanted2Args.bind(null, wantedDependency)
      if (
        resolvedDependencies[wantedDependency.alias] &&
        (satisfiesWanted(resolvedDependencies[wantedDependency.alias]) || resolvedDependencies[wantedDependency.alias].startsWith('file:'))
      ) {
        reference = resolvedDependencies[wantedDependency.alias]
      } else if (
        // If dependencies that were used by the previous version of the package
        // satisfy the newer version's requirements, then pnpm tries to keep
        // the previous dependency.
        // So for example, if foo@1.0.0 had bar@1.0.0 as a dependency
        // and foo was updated to 1.1.0 which depends on bar ^1.0.0
        // then bar@1.0.0 can be reused for foo@1.1.0
        semver.validRange(wantedDependency.pref) !== null &&
        preferredDependencies[wantedDependency.alias] &&
        satisfiesWanted(preferredDependencies[wantedDependency.alias])
      ) {
        proceed = true
        reference = preferredDependencies[wantedDependency.alias]
      }
    }
    const infoFromLockfile = getInfoFromLockfile(wantedLockfile, options.registries, reference, wantedDependency.alias)
    if (
      !proceedAll &&
      (
        (infoFromLockfile == null) ||
        infoFromLockfile.dependencyLockfile != null && (
          infoFromLockfile.dependencyLockfile.peerDependencies != null ||
          infoFromLockfile.dependencyLockfile.transitivePeerDependencies?.length
        )
      )
    ) {
      proceed = true
      proceedAll = true
      for (const extendedWantedDep of extendedWantedDeps) {
        if (!extendedWantedDep.proceed) {
          extendedWantedDep.proceed = true
        }
      }
    }
    extendedWantedDeps.push({
      infoFromLockfile,
      proceed,
      wantedDependency,
    })
  }
  return extendedWantedDeps
}

function referenceSatisfiesWantedSpec (
  opts: {
    lockfile: Lockfile
    prefix: string
  },
  wantedDep: { alias: string, pref: string },
  preferredRef: string
) {
  const depPath = dp.refToRelative(preferredRef, wantedDep.alias)
  if (depPath === null) return false
  const pkgSnapshot = opts.lockfile.packages?.[depPath]
  if (pkgSnapshot == null) {
    logger.warn({
      message: `Could not find preferred package ${depPath} in lockfile`,
      prefix: opts.prefix,
    })
    return false
  }
  const { version } = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
  return semver.satisfies(version, wantedDep.pref, true)
}

type InfoFromLockfile = {
  depPath: string
  pkgId: string
  dependencyLockfile?: PackageSnapshot
  name?: string
  version?: string
  resolution?: Resolution
} & ({
  dependencyLockfile: PackageSnapshot
  name: string
  version: string
  resolution: Resolution
} | unknown)

function getInfoFromLockfile (
  lockfile: Lockfile,
  registries: Registries,
  reference: string | undefined,
  alias: string | undefined
): InfoFromLockfile | undefined {
  if (!reference || !alias) {
    return undefined
  }

  const depPath = dp.refToRelative(reference, alias)

  if (!depPath) {
    return undefined
  }

  let dependencyLockfile = lockfile.packages?.[depPath]

  if (dependencyLockfile != null) {
    if ((dependencyLockfile.peerDependencies != null) && (dependencyLockfile.dependencies != null)) {
      // This is done to guarantee that the dependency will be relinked with the
      // up-to-date peer dependencies
      // Covered by test: "peer dependency is grouped with dependency when peer is resolved not from a top dependency"
      const dependencies: Record<string, string> = {}
      for (const [depName, ref] of Object.entries(dependencyLockfile.dependencies ?? {})) {
        if (dependencyLockfile.peerDependencies[depName]) continue
        dependencies[depName] = ref
      }
      dependencyLockfile = {
        ...dependencyLockfile,
        dependencies,
      }
    }

    return {
      ...nameVerFromPkgSnapshot(depPath, dependencyLockfile),
      dependencyLockfile,
      depPath,
      pkgId: packageIdFromSnapshot(depPath, dependencyLockfile, registries),
      // resolution may not exist if lockfile is broken, and an unexpected error will be thrown
      // if resolution does not exist, return undefined so it can be autofixed later
      resolution: dependencyLockfile.resolution && pkgSnapshotToResolution(depPath, dependencyLockfile, registries),
    }
  } else {
    return {
      depPath,
      pkgId: dp.tryGetPackageId(registries, depPath) ?? depPath, // Does it make sense to set pkgId when we're not sure?
    }
  }
}

interface ResolveDependencyOptions {
  currentDepth: number
  currentPkg?: {
    depPath?: string
    name?: string
    version?: string
    pkgId?: string
    resolution?: Resolution
    dependencyLockfile?: PackageSnapshot
  }
  parentPkg: ParentPkg
  parentPkgAliases: ParentPkgAliases
  preferredVersions: PreferredVersions
  prefix: string
  proceed: boolean
  publishedBy?: Date
  pickLowestVersion?: boolean
  update: boolean
  updateDepth: number
  updateMatching?: UpdateMatchingFunction
}

type ResolveDependencyResult = PkgAddress | LinkedDependency | null

async function resolveDependency (
  wantedDependency: WantedDependency,
  ctx: ResolutionContext,
  options: ResolveDependencyOptions
): Promise<ResolveDependencyResult> {
  const currentPkg = options.currentPkg ?? {}

  const currentLockfileContainsTheDep = currentPkg.depPath
    ? Boolean(ctx.currentLockfile.packages?.[currentPkg.depPath])
    : undefined
  const depIsLinked = Boolean(
    // if package is not in `node_modules/.pnpm-lock.yaml`
    // we can safely assume that it doesn't exist in `node_modules`
    currentLockfileContainsTheDep &&
    currentPkg.depPath &&
    currentPkg.dependencyLockfile &&
    await exists(
      path.join(
        ctx.virtualStoreDir,
        dp.depPathToFilename(currentPkg.depPath),
        'node_modules',
        currentPkg.name!,
        'package.json'
      )
    )
  )

  if (!options.update && !options.proceed && (currentPkg.resolution != null) && depIsLinked) {
    return null
  }

  let pkgResponse!: PackageResponse
  if (!options.parentPkg.installable) {
    wantedDependency = {
      ...wantedDependency,
      optional: true,
    }
  }
  try {
    pkgResponse = await ctx.storeController.requestPackage(wantedDependency, {
      alwaysTryWorkspacePackages: ctx.linkWorkspacePackagesDepth >= options.currentDepth,
      currentPkg: currentPkg
        ? {
          id: currentPkg.pkgId,
          resolution: currentPkg.resolution,
        }
        : undefined,
      expectedPkg: currentPkg,
      defaultTag: ctx.defaultTag,
      ignoreScripts: ctx.ignoreScripts,
      publishedBy: options.publishedBy,
      pickLowestVersion: options.pickLowestVersion,
      downloadPriority: -options.currentDepth,
      lockfileDir: ctx.lockfileDir,
      preferredVersions: options.preferredVersions,
      preferWorkspacePackages: ctx.preferWorkspacePackages,
      projectDir: (
        options.currentDepth > 0 &&
        !wantedDependency.pref.startsWith('file:')
      )
        ? ctx.lockfileDir
        : options.parentPkg.rootDir,
      registry: wantedDependency.alias && pickRegistryForPackage(ctx.registries, wantedDependency.alias, wantedDependency.pref) || ctx.registries.default,
      // Unfortunately, even when run with --lockfile-only, we need the *real* package.json
      // so fetching of the tarball cannot be ever avoided. Related issue: https://github.com/pnpm/pnpm/issues/1176
      skipFetch: false,
      update: options.update,
      workspacePackages: ctx.workspacePackages,
    })
  } catch (err: any) { // eslint-disable-line
    if (wantedDependency.optional) {
      skippedOptionalDependencyLogger.debug({
        details: err.toString(),
        package: {
          name: wantedDependency.alias,
          pref: wantedDependency.pref,
          version: wantedDependency.alias ? wantedDependency.pref : undefined,
        },
        parents: nodeIdToParents(options.parentPkg.nodeId, ctx.resolvedPackagesByDepPath),
        prefix: options.prefix,
        reason: 'resolution_failure',
      })
      return null
    }
    err.prefix = options.prefix
    err.pkgsStack = nodeIdToParents(options.parentPkg.nodeId, ctx.resolvedPackagesByDepPath)
    throw err
  }

  dependencyResolvedLogger.debug({
    resolution: pkgResponse.body.id,
    wanted: {
      dependentId: options.parentPkg.depPath,
      name: wantedDependency.alias,
      rawSpec: wantedDependency.pref,
    },
  })

  pkgResponse.body.id = encodePkgId(pkgResponse.body.id)

  if (
    !pkgResponse.body.updated &&
    options.currentDepth === Math.max(0, options.updateDepth) &&
    depIsLinked && !ctx.force && !options.proceed
  ) {
    return null
  }

  if (pkgResponse.body.isLocal) {
    const manifest = pkgResponse.body.manifest ?? (await pkgResponse.fetching!()).bundledManifest
    if (!manifest) {
      // This should actually never happen because the local-resolver returns a manifest
      // even if no real manifest exists in the filesystem.
      throw new PnpmError('MISSING_PACKAGE_JSON', `Can't install ${wantedDependency.pref}: Missing package.json file`)
    }
    return {
      alias: wantedDependency.alias || manifest.name,
      depPath: pkgResponse.body.id,
      dev: wantedDependency.dev,
      isLinkedDependency: true,
      name: manifest.name,
      normalizedPref: pkgResponse.body.normalizedPref,
      optional: wantedDependency.optional,
      pkgId: pkgResponse.body.id,
      resolution: pkgResponse.body.resolution,
      version: manifest.version,
    }
  }

  let prepare!: boolean
  let hasBin!: boolean
  let pkg: PackageManifest = await getManifestFromResponse(pkgResponse, wantedDependency)
  if (!pkg.dependencies) {
    pkg.dependencies = {}
  }
  if (ctx.readPackageHook != null) {
    pkg = await ctx.readPackageHook(pkg)
  }
  if (pkg.peerDependencies && pkg.dependencies) {
    if (ctx.autoInstallPeers) {
      pkg = {
        ...pkg,
        dependencies: omit(Object.keys(pkg.peerDependencies), pkg.dependencies),
      }
    } else {
      pkg = {
        ...pkg,
        dependencies: omit(
          Object.keys(pkg.peerDependencies).filter((peerDep) => options.parentPkgAliases[peerDep]),
          pkg.dependencies
        ),
      }
    }
  }
  if (!pkg.name) { // TODO: don't fail on optional dependencies
    throw new PnpmError('MISSING_PACKAGE_NAME', `Can't install ${wantedDependency.pref}: Missing package name`)
  }
  let depPath = dp.relative(ctx.registries, pkg.name, pkgResponse.body.id)
  const nameAndVersion = `${pkg.name}@${pkg.version}`
  const patchFile = ctx.patchedDependencies?.[nameAndVersion]
  if (patchFile) {
    ctx.appliedPatches.add(nameAndVersion)
    depPath += `(patch_hash=${patchFile.hash})`
  }

  // We are building the dependency tree only until there are new packages
  // or the packages repeat in a unique order.
  // This is needed later during peer dependencies resolution.
  //
  // So we resolve foo > bar > qar > foo
  // But we stop on foo > bar > qar > foo > qar
  // In the second example, there's no reason to walk qar again
  // when qar is included the first time, the dependencies of foo
  // are already resolved and included as parent dependencies of qar.
  // So during peers resolution, qar cannot possibly get any new or different
  // peers resolved, after the first occurrence.
  //
  // However, in the next example we would analyze the second qar as well,
  // because zoo is a new parent package:
  // foo > bar > qar > zoo > qar
  if (
    nodeIdContainsSequence(
      options.parentPkg.nodeId,
      options.parentPkg.depPath,
      depPath
    ) || depPath === options.parentPkg.depPath
  ) {
    return null
  }

  if (
    !options.update && (currentPkg.dependencyLockfile != null) && currentPkg.depPath &&
    !pkgResponse.body.updated &&
    // peerDependencies field is also used for transitive peer dependencies which should not be linked
    // That's why we cannot omit reading package.json of such dependencies.
    // This can be removed if we implement something like peerDependenciesMeta.transitive: true
    (currentPkg.dependencyLockfile.peerDependencies == null)
  ) {
    prepare = currentPkg.dependencyLockfile.prepare === true
    hasBin = currentPkg.dependencyLockfile.hasBin === true
    pkg = {
      ...nameVerFromPkgSnapshot(currentPkg.depPath, currentPkg.dependencyLockfile),
      ...currentPkg.dependencyLockfile,
      ...pkg,
    }
  } else {
    prepare = Boolean(
      pkgResponse.body.resolvedVia === 'git-repository' &&
      typeof pkg.scripts?.prepare === 'string'
    )

    if (
      currentPkg.dependencyLockfile?.deprecated &&
      !pkgResponse.body.updated && !pkg.deprecated
    ) {
      pkg.deprecated = currentPkg.dependencyLockfile.deprecated
    }
    hasBin = Boolean((pkg.bin && !(pkg.bin === '' || Object.keys(pkg.bin).length === 0)) ?? pkg.directories?.bin)
    /* eslint-enable @typescript-eslint/dot-notation */
  }
  if (options.currentDepth === 0 && pkgResponse.body.latest && pkgResponse.body.latest !== pkg.version) {
    ctx.outdatedDependencies[pkgResponse.body.id] = pkgResponse.body.latest
  }

  // In case of leaf dependencies (dependencies that have no prod deps or peer deps),
  // we only ever need to analyze one leaf dep in a graph, so the nodeId can be short and stateless.
  const nodeId = pkgIsLeaf(pkg)
    ? `>${depPath}>`
    : createNodeId(options.parentPkg.nodeId, depPath)

  const parentIsInstallable = options.parentPkg.installable === undefined || options.parentPkg.installable
  const installable = parentIsInstallable && pkgResponse.body.isInstallable !== false
  const isNew = !ctx.resolvedPackagesByDepPath[depPath]
  const parentImporterId = options.parentPkg.nodeId.substring(0, options.parentPkg.nodeId.indexOf('>', 1) + 1)
  let resolveChildren = false
  const currentIsOptional = wantedDependency.optional || options.parentPkg.optional

  if (isNew) {
    if (
      pkg.deprecated &&
      (!ctx.allowedDeprecatedVersions[pkg.name] || !semver.satisfies(pkg.version, ctx.allowedDeprecatedVersions[pkg.name]))
    ) {
      // Report deprecated packages only on first occurrence.
      deprecationLogger.debug({
        deprecated: pkg.deprecated,
        depth: options.currentDepth,
        pkgId: pkgResponse.body.id,
        pkgName: pkg.name,
        pkgVersion: pkg.version,
        prefix: options.prefix,
      })
    }
    if (pkgResponse.body.isInstallable === false || !parentIsInstallable) {
      ctx.skipped.add(pkgResponse.body.id)
    }
    progressLogger.debug({
      packageId: pkgResponse.body.id,
      requester: ctx.lockfileDir,
      status: 'resolved',
    })

    // WARN: It is very important to keep this sync
    // Otherwise, deprecation messages for the same package might get written several times
    ctx.resolvedPackagesByDepPath[depPath] = getResolvedPackage({
      allowBuild: ctx.allowBuild,
      dependencyLockfile: currentPkg.dependencyLockfile,
      depPath,
      force: ctx.force,
      hasBin,
      patchFile,
      pkg,
      pkgResponse,
      prepare,
      wantedDependency,
      parentImporterId,
      optional: currentIsOptional,
    })
  } else {
    ctx.resolvedPackagesByDepPath[depPath].prod = ctx.resolvedPackagesByDepPath[depPath].prod || !wantedDependency.dev && !wantedDependency.optional
    ctx.resolvedPackagesByDepPath[depPath].dev = ctx.resolvedPackagesByDepPath[depPath].dev || wantedDependency.dev
    ctx.resolvedPackagesByDepPath[depPath].optional = ctx.resolvedPackagesByDepPath[depPath].optional && currentIsOptional
    if (ctx.autoInstallPeers) {
      resolveChildren = !ctx.missingPeersOfChildrenByPkgId[pkgResponse.body.id].missingPeersOfChildren.resolved &&
        !ctx.resolvedPackagesByDepPath[depPath].parentImporterIds.has(parentImporterId)
      ctx.resolvedPackagesByDepPath[depPath].parentImporterIds.add(parentImporterId)
    }
    if (ctx.resolvedPackagesByDepPath[depPath].fetching == null && pkgResponse.fetching != null) {
      ctx.resolvedPackagesByDepPath[depPath].fetching = pkgResponse.fetching
      ctx.resolvedPackagesByDepPath[depPath].filesIndexFile = pkgResponse.filesIndexFile!
    }

    if (ctx.dependenciesTree.has(nodeId)) {
      ctx.dependenciesTree.get(nodeId)!.depth = Math.min(ctx.dependenciesTree.get(nodeId)!.depth, options.currentDepth)
    } else {
      ctx.pendingNodes.push({
        alias: wantedDependency.alias || pkg.name,
        depth: options.currentDepth,
        installable,
        nodeId,
        resolvedPackage: ctx.resolvedPackagesByDepPath[depPath],
      })
    }
  }

  const rootDir = pkgResponse.body.resolution.type === 'directory'
    ? path.resolve(ctx.lockfileDir, (pkgResponse.body.resolution as DirectoryResolution).directory)
    : options.prefix
  let missingPeersOfChildren!: MissingPeersOfChildren | undefined
  if (ctx.autoInstallPeers && !nodeIdContains(options.parentPkg.nodeId, depPath)) {
    if (ctx.missingPeersOfChildrenByPkgId[pkgResponse.body.id]) {
      if (!options.parentPkg.nodeId.startsWith(ctx.missingPeersOfChildrenByPkgId[pkgResponse.body.id].parentImporterId)) {
        missingPeersOfChildren = ctx.missingPeersOfChildrenByPkgId[pkgResponse.body.id].missingPeersOfChildren
      }
    } else {
      const p = pDefer<MissingPeers>()
      missingPeersOfChildren = {
        resolve: p.resolve,
        reject: p.reject,
        get: pShare(p.promise),
      }
      ctx.missingPeersOfChildrenByPkgId[pkgResponse.body.id] = {
        parentImporterId,
        missingPeersOfChildren,
      }
    }
  }
  return {
    alias: wantedDependency.alias || pkg.name,
    depIsLinked,
    depPath,
    isNew: isNew || resolveChildren,
    nodeId,
    normalizedPref: options.currentDepth === 0 ? pkgResponse.body.normalizedPref : undefined,
    missingPeersOfChildren,
    pkgId: pkgResponse.body.id,
    rootDir,
    missingPeers: getMissingPeers(pkg),
    optional: ctx.resolvedPackagesByDepPath[depPath].optional,

    // Next fields are actually only needed when isNew = true
    installable,
    isLinkedDependency: undefined,
    pkg,
    updated: pkgResponse.body.updated,
    publishedAt: pkgResponse.body.publishedAt,
  }
}

async function getManifestFromResponse (
  pkgResponse: PackageResponse,
  wantedDependency: WantedDependency
): Promise<PackageManifest> {
  const pkg = pkgResponse.body.manifest ?? (await pkgResponse.fetching!()).bundledManifest
  if (pkg) return pkg
  return {
    name: wantedDependency.pref.split('/').pop()!,
    version: '0.0.0',
  }
}

function getMissingPeers (pkg: PackageManifest) {
  const missingPeers = {} as MissingPeers
  for (const [peerName, peerVersion] of Object.entries(pkg.peerDependencies ?? {})) {
    if (!pkg.peerDependenciesMeta?.[peerName]?.optional) {
      missingPeers[peerName] = peerVersion
    }
  }
  return missingPeers
}

function pkgIsLeaf (pkg: PackageManifest) {
  return Object.keys(pkg.dependencies ?? {}).length === 0 &&
    Object.keys(pkg.optionalDependencies ?? {}).length === 0 &&
    Object.keys(pkg.peerDependencies ?? {}).length === 0 &&
    // Package manifests can declare peerDependenciesMeta without declaring
    // peerDependencies. peerDependenciesMeta implies the later.
    Object.keys(pkg.peerDependenciesMeta ?? {}).length === 0
}

function getResolvedPackage (
  options: {
    allowBuild?: (pkgName: string) => boolean
    dependencyLockfile?: PackageSnapshot
    depPath: string
    force: boolean
    hasBin: boolean
    parentImporterId: string
    patchFile?: PatchFile
    pkg: PackageManifest
    pkgResponse: PackageResponse
    prepare: boolean
    optional: boolean
    wantedDependency: WantedDependency
  }
): ResolvedPackage {
  const peerDependencies = peerDependenciesWithoutOwn(options.pkg)

  const requiresBuild = (options.allowBuild == null || options.allowBuild(options.pkg.name))
    ? ((options.dependencyLockfile != null) ? Boolean(options.dependencyLockfile.requiresBuild) : safePromiseDefer<boolean>())
    : false

  return {
    additionalInfo: {
      bundledDependencies: options.pkg.bundledDependencies,
      bundleDependencies: options.pkg.bundleDependencies,
      cpu: options.pkg.cpu,
      deprecated: options.pkg.deprecated,
      engines: options.pkg.engines,
      os: options.pkg.os,
      libc: options.pkg.libc,
    },
    parentImporterIds: new Set([options.parentImporterId]),
    depPath: options.depPath,
    dev: options.wantedDependency.dev,
    fetching: options.pkgResponse.fetching!,
    filesIndexFile: options.pkgResponse.filesIndexFile!,
    hasBin: options.hasBin,
    hasBundledDependencies: !((options.pkg.bundledDependencies ?? options.pkg.bundleDependencies) == null),
    id: options.pkgResponse.body.id,
    name: options.pkg.name,
    optional: options.optional,
    optionalDependencies: new Set(Object.keys(options.pkg.optionalDependencies ?? {})),
    patchFile: options.patchFile,
    peerDependencies: peerDependencies ?? {},
    peerDependenciesMeta: options.pkg.peerDependenciesMeta,
    prepare: options.prepare,
    prod: !options.wantedDependency.dev && !options.wantedDependency.optional,
    requiresBuild,
    resolution: options.pkgResponse.body.resolution,
    version: options.pkg.version,
  }
}

function peerDependenciesWithoutOwn (pkg: PackageManifest) {
  if ((pkg.peerDependencies == null) && (pkg.peerDependenciesMeta == null)) return pkg.peerDependencies
  const ownDeps = new Set([
    ...Object.keys(pkg.dependencies ?? {}),
    ...Object.keys(pkg.optionalDependencies ?? {}),
  ])
  const result: Record<string, string> = {}
  if (pkg.peerDependencies != null) {
    for (const [peerName, peerRange] of Object.entries(pkg.peerDependencies)) {
      if (ownDeps.has(peerName)) continue
      result[peerName] = peerRange
    }
  }
  if (pkg.peerDependenciesMeta != null) {
    for (const [peerName, peerMeta] of Object.entries(pkg.peerDependenciesMeta)) {
      if (ownDeps.has(peerName) || result[peerName] || peerMeta.optional !== true) continue
      result[peerName] = '*'
    }
  }
  if (Object.keys(result).length === 0) return undefined
  return result
}
