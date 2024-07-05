import path from 'path'
import { matchCatalogResolveResult, type CatalogResolver } from '@pnpm/catalogs.resolver'
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
  type PkgResolutionId,
} from '@pnpm/resolver-base'
import {
  type PkgRequestFetchResult,
  type PackageResponse,
  type StoreController,
} from '@pnpm/store-controller-types'
import {
  type DepPath,
  type SupportedArchitectures,
  type AllowedDeprecatedVersions,
  type PackageManifest,
  type PatchFile,
  type ReadPackageHook,
  type Registries,
  type PkgIdWithPatchHash,
} from '@pnpm/types'
import * as dp from '@pnpm/dependency-path'
import { getPreferredVersionsFromLockfileAndManifests } from '@pnpm/lockfile.preferred-versions'
import normalizePath from 'normalize-path'
import exists from 'path-exists'
import pDefer from 'p-defer'
import pShare from 'promise-share'
import partition from 'ramda/src/partition'
import pickBy from 'ramda/src/pickBy'
import omit from 'ramda/src/omit'
import zipWith from 'ramda/src/zipWith'
import semver from 'semver'
import { getNonDevWantedDependencies, type WantedDependency } from './getNonDevWantedDependencies'
import { safeIntersect } from './mergePeers'
import { type NodeId, nextNodeId } from './nextNodeId'
import { parentIdsContainSequence } from './parentIdsContainSequence'
import { hoistPeers, getHoistableOptionalPeers } from './hoistPeers'
import { wantedDepIsLocallyAvailable } from './wantedDepIsLocallyAvailable'
import { type CatalogLookupMetadata } from './resolveDependencyTree'
import { replaceVersionInPref } from './replaceVersionInPref'

const dependencyResolvedLogger = logger('_dependency_resolved')

const omitDepsFields = omit(['dependencies', 'optionalDependencies', 'peerDependencies', 'peerDependenciesMeta'])

export function getPkgsInfoFromIds (
  ids: PkgResolutionId[],
  resolvedPkgsById: ResolvedPkgsById
): Array<{ id: PkgResolutionId, name: string, version: string }> {
  return ids
    .slice(1)
    .map((id) => {
      const { name, version } = resolvedPkgsById[id]
      return { id, name, version }
    })
}

// child nodeId by child alias name in case of non-linked deps
export interface ChildrenMap {
  [alias: string]: NodeId
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
NodeId,
DependenciesTreeNode<T>
>

export type ResolvedPkgsById = Record<PkgResolutionId, ResolvedPackage>

export interface LinkedDependency {
  isLinkedDependency: true
  optional: boolean
  dev: boolean
  resolution: DirectoryResolution
  pkgId: PkgResolutionId
  version: string
  name: string
  normalizedPref?: string
  alias: string
  catalogLookup?: CatalogLookupMetadata
}

export interface PendingNode {
  alias: string
  nodeId: NodeId
  resolvedPackage: ResolvedPackage
  depth: number
  installable: boolean
  parentIds: PkgResolutionId[]
}

export interface ChildrenByParentId {
  [id: PkgResolutionId]: Array<{
    alias: string
    id: PkgResolutionId
  }>
}

export interface ResolutionContext {
  allPeerDepNames: Set<string>
  autoInstallPeers: boolean
  autoInstallPeersFromHighestMatch: boolean
  allowBuild?: (pkgName: string) => boolean
  allowedDeprecatedVersions: AllowedDeprecatedVersions
  allPreferredVersions?: PreferredVersions
  appliedPatches: Set<string>
  updatedSet: Set<string>
  catalogResolver: CatalogResolver
  defaultTag: string
  dryRun: boolean
  forceFullResolution: boolean
  ignoreScripts?: boolean
  resolvedPkgsById: ResolvedPkgsById
  outdatedDependencies: Record<PkgResolutionId, string>
  childrenByParentId: ChildrenByParentId
  patchedDependencies?: Record<string, PatchFile>
  pendingNodes: PendingNode[]
  wantedLockfile: Lockfile
  currentLockfile: Lockfile
  linkWorkspacePackagesDepth: number
  lockfileDir: string
  storeController: StoreController
  // the IDs of packages that are not installable
  skipped: Set<PkgResolutionId>
  dependenciesTree: DependenciesTree<ResolvedPackage>
  force: boolean
  preferWorkspacePackages?: boolean
  readPackageHook?: ReadPackageHook
  engineStrict: boolean
  nodeVersion?: string
  pnpmVersion: string
  registries: Registries
  resolutionMode?: 'highest' | 'time-based' | 'lowest-direct'
  virtualStoreDir: string
  virtualStoreDirMaxLength: number
  workspacePackages?: WorkspacePackages
  missingPeersOfChildrenByPkgId: Record<PkgResolutionId, { depth: number, missingPeersOfChildren: MissingPeersOfChildren }>
  hoistPeers?: boolean
}

export type MissingPeers = Record<string, { range: string, optional: boolean }>

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
  isNew: boolean
  isLinkedDependency?: false
  nodeId: NodeId
  pkgId: PkgResolutionId
  normalizedPref?: string // is returned only for root dependencies
  installable: boolean
  pkg: PackageManifest
  version?: string
  updated: boolean
  rootDir: string
  missingPeers: MissingPeers
  missingPeersOfChildren?: MissingPeersOfChildren
  publishedAt?: string
  catalogLookup?: CatalogLookupMetadata
  optional: boolean
} & ({
  isLinkedDependency: true
  version: string
} | {
  isLinkedDependency: undefined
})

export interface PeerDependency {
  version: string
  optional?: boolean
}

export type PeerDependencies = Record<string, PeerDependency>

export interface ResolvedPackage {
  id: PkgResolutionId
  resolution: Resolution
  prod: boolean
  dev: boolean
  optional: boolean
  fetching: () => Promise<PkgRequestFetchResult>
  filesIndexFile: string
  name: string
  version: string
  peerDependencies: PeerDependencies
  optionalDependencies: Set<string>
  hasBin: boolean
  hasBundledDependencies: boolean
  patchFile?: PatchFile
  prepare: boolean
  pkgIdWithPatchHash: PkgIdWithPatchHash
  requiresBuild?: boolean
  additionalInfo: {
    deprecated?: string
    bundleDependencies?: string[] | boolean
    bundledDependencies?: string[] | boolean
    engines?: {
      node?: string
      npm?: string
    }
    cpu?: string[]
    os?: string[]
    libc?: string[]
  }
}

type ParentPkg = Pick<PkgAddress, 'nodeId' | 'installable' | 'rootDir' | 'optional' | 'pkgId'>

export type ParentPkgAliases = Record<string, PkgAddress | true>

export type UpdateMatchingFunction = (pkgName: string) => boolean

interface ResolvedDependenciesOptions {
  currentDepth: number
  parentIds: PkgResolutionId[]
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
  supportedArchitectures?: SupportedArchitectures
  updateToLatest?: boolean
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
  if (ctx.autoInstallPeers) {
    ctx.allPreferredVersions = getPreferredVersionsFromLockfileAndManifests(ctx.wantedLockfile.packages, [])
  } else if (ctx.hoistPeers) {
    ctx.allPreferredVersions = {}
  }
  const { pkgAddressesByImportersWithoutPeers, publishedBy, time } = await resolveDependenciesOfImporters(ctx, importers)
  const pkgAddressesByImporters = await Promise.all(zipWith(async (importerResolutionResult, { parentPkgAliases, preferredVersions, options }) => {
    const pkgAddresses = importerResolutionResult.pkgAddresses
    if (!ctx.hoistPeers) return pkgAddresses
    const allMissingOptionalPeers: Record<string, string[]> = {}
    while (true) {
      for (const pkgAddress of importerResolutionResult.pkgAddresses) {
        parentPkgAliases[pkgAddress.alias] = true
      }
      const [missingOptionalPeers, missingRequiredPeers] = partition(([, { optional }]) => optional, Object.entries(importerResolutionResult.missingPeers ?? {}))
      for (const missingPeerName of Object.keys(missingRequiredPeers)) {
        parentPkgAliases[missingPeerName] = true
      }
      if (ctx.autoInstallPeers) {
        // All the missing peers should get installed in the root.
        // Otherwise, pending nodes will not work.
        // even those peers should be hoisted that are not autoinstalled
        for (const [resolvedPeerName, resolvedPeerAddress] of Object.entries(importerResolutionResult.resolvedPeers ?? {})) {
          if (!parentPkgAliases[resolvedPeerName]) {
            pkgAddresses.push(resolvedPeerAddress)
          }
        }
      }
      for (const [missingOptionalPeerName, { range: missingOptionalPeerRange }] of missingOptionalPeers) {
        if (!allMissingOptionalPeers[missingOptionalPeerName]) {
          allMissingOptionalPeers[missingOptionalPeerName] = [missingOptionalPeerRange]
        } else if (!allMissingOptionalPeers[missingOptionalPeerName].includes(missingOptionalPeerRange)) {
          allMissingOptionalPeers[missingOptionalPeerName].push(missingOptionalPeerRange)
        }
      }
      if (!missingRequiredPeers.length) break
      const dependencies = hoistPeers(missingRequiredPeers, ctx)
      if (!Object.keys(dependencies).length) break
      const wantedDependencies = getNonDevWantedDependencies({ dependencies })

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
    if (Object.keys(allMissingOptionalPeers).length && ctx.allPreferredVersions) {
      const optionalDependencies = getHoistableOptionalPeers(allMissingOptionalPeers, ctx.allPreferredVersions)
      if (Object.keys(optionalDependencies).length) {
        const wantedDependencies = getNonDevWantedDependencies({ optionalDependencies })
        const resolveDependenciesResult = await resolveDependencies(ctx, preferredVersions, wantedDependencies, {
          ...options,
          parentPkgAliases,
          publishedBy,
        })
        importerResolutionResult = {
          pkgAddresses: resolveDependenciesResult.pkgAddresses,
          ...filterMissingPeers(await resolveDependenciesResult.resolvingPeers, parentPkgAliases),
        }
        pkgAddresses.push(...importerResolutionResult.pkgAddresses)
      }
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

export type ImporterToResolveOptions = Omit<ResolvedDependenciesOptions, 'parentPkgAliases' | 'publishedBy'>

export interface ImporterToResolve {
  updatePackageManifest: boolean
  preferredVersions: PreferredVersions
  parentPkgAliases: ParentPkgAliases
  wantedDependencies: Array<WantedDependency & { updateDepth?: number }>
  options: ImporterToResolveOptions
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
  const pickLowestVersion = ctx.resolutionMode === 'time-based' || ctx.resolutionMode === 'lowest-direct'
  const resolveResults = await Promise.all(
    importers.map(async (importer) => {
      const extendedWantedDeps = getDepsToResolve(importer.wantedDependencies, ctx.wantedLockfile, {
        preferredDependencies: importer.options.preferredDependencies,
        prefix: importer.options.prefix,
        proceed: importer.options.proceed || ctx.forceFullResolution,
        registries: ctx.registries,
        resolvedDependencies: importer.options.resolvedDependencies,
      })
      const postponedResolutionsQueue: PostponedResolutionFunction[] = []
      const postponedPeersResolutionQueue: PostponedPeersResolutionFunction[] = []
      const pkgAddresses: PkgAddress[] = []

      const resolveDependenciesOfImporterWantedDep = resolveDependenciesOfImporterDependency.bind(null, {
        ctx,
        importer,
        pickLowestVersion,
      })
      const resolvedDependenciesOfImporter = await Promise.all(extendedWantedDeps.map(resolveDependenciesOfImporterWantedDep))

      for (const { resolveDependencyResult, postponedPeersResolution, postponedResolution } of resolvedDependenciesOfImporter) {
        if (resolveDependencyResult) {
          pkgAddresses.push(resolveDependencyResult as PkgAddress)
        }
        if (postponedResolution) {
          postponedResolutionsQueue.push(postponedResolution)
        }
        if (postponedPeersResolution) {
          postponedPeersResolutionQueue.push(postponedPeersResolution)
        }
      }

      return { pkgAddresses, postponedResolutionsQueue, postponedPeersResolutionQueue }
    })
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
      const resolvedPackage = ctx.resolvedPkgsById[pkgAddress.pkgId]
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
    const postponedResolutionOpts: PostponedResolutionOpts = {
      preferredVersions: newPreferredVersions,
      parentPkgAliases: newParentPkgAliases,
      publishedBy,
    }
    const childrenResults = await Promise.all(
      postponedResolutionsQueue.map((postponedResolution) => postponedResolution(postponedResolutionOpts))
    )
    if (!ctx.hoistPeers) {
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
      ].map(({ missingPeers }) => missingPeers).filter(Boolean),
      {
        autoInstallPeersFromHighestMatch: ctx.autoInstallPeersFromHighestMatch,
      }
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

export interface ResolveDependenciesOfImporterDependencyOpts {
  readonly ctx: ResolutionContext
  readonly importer: ImporterToResolve
  readonly pickLowestVersion: boolean
}

async function resolveDependenciesOfImporterDependency (
  { ctx, importer, pickLowestVersion }: ResolveDependenciesOfImporterDependencyOpts,
  extendedWantedDep: ExtendedWantedDependency
): Promise<ResolveDependenciesOfDependency> {
  // The catalog protocol is only usable in importers (i.e. packages in the
  // workspace. Replacing catalog protocol while resolving importers here before
  // resolving dependencies of packages outside of the workspace/monorepo.
  const catalogLookup = matchCatalogResolveResult(ctx.catalogResolver(extendedWantedDep.wantedDependency), {
    found: (result) => result.resolution,
    unused: () => undefined,
    misconfiguration: (result) => {
      throw result.error
    },
  })

  if (catalogLookup != null) {
    // The lockfile from a previous installation may have already resolved this
    // cataloged dependency. Reuse the exact version in the lockfile catalog
    // snapshot to ensure all projects using the same cataloged dependency get
    // the same version.
    const existingCatalogResolution = ctx.wantedLockfile.catalogs
      ?.[catalogLookup.catalogName]
      ?.[extendedWantedDep.wantedDependency.alias]
    const replacementPref = existingCatalogResolution?.specifier === catalogLookup.specifier
      ? replaceVersionInPref(catalogLookup.specifier, existingCatalogResolution.version)
      : catalogLookup.specifier

    extendedWantedDep.wantedDependency.pref = replacementPref
  }

  const result = await resolveDependenciesOfDependency(
    ctx,
    importer.preferredVersions,
    {
      ...importer.options,
      parentPkgAliases: importer.parentPkgAliases,
      pickLowestVersion: pickLowestVersion && !importer.updatePackageManifest,
    },
    extendedWantedDep
  )

  // If the catalog protocol was used, store metadata about the catalog
  // lookup to use in the lockfile.
  if (result.resolveDependencyResult != null && catalogLookup != null) {
    result.resolveDependencyResult.catalogLookup = catalogLookup
  }

  return result
}

function filterMissingPeersFromPkgAddresses (
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

function getPublishedByDate (pkgAddresses: PkgAddress[], timeFromLockfile: Record<string, string> = {}): { publishedBy: Date, newTime: Record<string, string> } {
  const newTime: Record<string, string> = {}
  for (const pkgAddress of pkgAddresses) {
    if (pkgAddress.publishedAt) {
      newTime[pkgAddress.pkgId] = pkgAddress.publishedAt
    } else if (timeFromLockfile[pkgAddress.pkgId]) {
      newTime[pkgAddress.pkgId] = timeFromLockfile[pkgAddress.pkgId]
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
    extendedWantedDeps.map(async (extendedWantedDep) => {
      const {
        resolveDependencyResult,
        postponedResolution,
        postponedPeersResolution,
      } = await resolveDependenciesOfDependency(
        ctx,
        preferredVersions,
        options,
        extendedWantedDep
      )
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
  ))
  const newPreferredVersions = Object.create(preferredVersions) as PreferredVersions
  const currentParentPkgAliases: Record<string, PkgAddress | true> = {}
  for (const pkgAddress of pkgAddresses) {
    if (currentParentPkgAliases[pkgAddress.alias] !== true) {
      currentParentPkgAliases[pkgAddress.alias] = pkgAddress
    }
    if (pkgAddress.updated) {
      ctx.updatedSet.add(pkgAddress.alias)
    }
    const resolvedPackage = ctx.resolvedPkgsById[pkgAddress.pkgId]
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
  const postponedResolutionOpts: PostponedResolutionOpts = {
    preferredVersions: newPreferredVersions,
    parentPkgAliases: newParentPkgAliases,
    publishedBy: options.publishedBy,
  }
  const childrenResults = await Promise.all(
    postponedResolutionsQueue.map((postponedResolution) => postponedResolution(postponedResolutionOpts))
  )
  if (!ctx.hoistPeers) {
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
      autoInstallPeersFromHighestMatch: ctx.autoInstallPeersFromHighestMatch,
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

function mergePkgsDeps (pkgsDeps: MissingPeers[], opts: { autoInstallPeersFromHighestMatch: boolean }): MissingPeers {
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
  const mergedPkgDeps = {} as MissingPeers
  for (const [name, { ranges, optional }] of Object.entries(groupedRanges)) {
    const intersection = safeIntersect(ranges)
    if (intersection) {
      mergedPkgDeps[name] = { range: intersection, optional }
    } else if (opts.autoInstallPeersFromHighestMatch) {
      mergedPkgDeps[name] = { range: ranges.join(' || '), optional }
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
    supportedArchitectures: options.supportedArchitectures,
    updateToLatest: options.updateToLatest,
    parentIds: options.parentIds,
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
    parentIds: [...options.parentIds, resolveDependencyResult.pkgId],
    updateDepth,
    prefix: options.prefix,
    updateMatching: options.updateMatching,
    supportedArchitectures: options.supportedArchitectures,
    updateToLatest: options.updateToLatest,
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

export function createNodeIdForLinkedLocalPkg (lockfileDir: string, pkgDir: string): NodeId {
  return `link:${normalizePath(path.relative(lockfileDir, pkgDir))}` as NodeId
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
    parentIds,
    dependencyLockfile,
    parentDepth,
    updateDepth,
    updateMatching,
    prefix,
    supportedArchitectures,
  }: {
    parentPkg: PkgAddress
    parentIds: PkgResolutionId[]
    dependencyLockfile: PackageSnapshot | undefined
    parentDepth: number
    updateDepth: number
    prefix: string
    updateMatching?: UpdateMatchingFunction
    supportedArchitectures?: SupportedArchitectures
    updateToLatest?: boolean
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
): Promise<PeersResolutionResult> {
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
      supportedArchitectures,
      parentIds,
    }
  )
  ctx.childrenByParentId[parentPkg.pkgId] = pkgAddresses.map((child) => ({
    alias: child.alias,
    id: child.pkgId,
  }))
  ctx.dependenciesTree.set(parentPkg.nodeId, {
    children: pkgAddresses.reduce((chn, child) => {
      chn[child.alias] = (child as PkgAddress).nodeId ?? (child.pkgId as unknown as NodeId)
      return chn
    }, {} as Record<string, NodeId>),
    depth: parentDepth,
    installable: parentPkg.installable,
    resolvedPackage: ctx.resolvedPkgsById[parentPkg.pkgId],
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
  if (!semver.validRange(wantedDep.pref) && Object.values(opts.lockfile.importers).filter(importer => importer.specifiers[wantedDep.alias] === wantedDep.pref).length) {
    return true
  }
  return semver.satisfies(version, wantedDep.pref, true)
}

type InfoFromLockfile = {
  pkgId: PkgResolutionId
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

    const { name, version, nonSemverVersion } = nameVerFromPkgSnapshot(depPath, dependencyLockfile)
    return {
      name,
      version,
      dependencyLockfile,
      pkgId: nonSemverVersion ?? (`${name}@${version}` as PkgResolutionId),
      // resolution may not exist if lockfile is broken, and an unexpected error will be thrown
      // if resolution does not exist, return undefined so it can be autofixed later
      resolution: dependencyLockfile.resolution && pkgSnapshotToResolution(depPath, dependencyLockfile, registries),
    }
  } else {
    const parsed = dp.parse(depPath)
    return {
      pkgId: parsed.nonSemverVersion ?? (parsed.name && parsed.version ? `${parsed.name}@${parsed.version}` : depPath) as PkgResolutionId, // Does it make sense to set pkgId when we're not sure?
    }
  }
}

interface ResolveDependencyOptions {
  currentDepth: number
  currentPkg?: {
    depPath?: DepPath
    name?: string
    version?: string
    pkgId?: PkgResolutionId
    resolution?: Resolution
    dependencyLockfile?: PackageSnapshot
  }
  parentPkg: ParentPkg
  parentIds: PkgResolutionId[]
  parentPkgAliases: ParentPkgAliases
  preferredVersions: PreferredVersions
  prefix: string
  proceed: boolean
  publishedBy?: Date
  pickLowestVersion?: boolean
  update: boolean
  updateDepth: number
  updateMatching?: UpdateMatchingFunction
  supportedArchitectures?: SupportedArchitectures
  updateToLatest?: boolean
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
    currentPkg.name &&
    await exists(
      path.join(
        ctx.virtualStoreDir,
        dp.depPathToFilename(currentPkg.depPath, ctx.virtualStoreDirMaxLength),
        'node_modules',
        currentPkg.name,
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
    if (!options.update && currentPkg.version && currentPkg.pkgId?.endsWith(`@${currentPkg.version}`)) {
      wantedDependency.pref = replaceVersionInPref(wantedDependency.pref, currentPkg.version)
    }
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
      skipFetch: ctx.dryRun,
      update: options.update,
      workspacePackages: ctx.workspacePackages,
      supportedArchitectures: options.supportedArchitectures,
      onFetchError: (err: any) => { // eslint-disable-line
        err.prefix = options.prefix
        err.pkgsStack = getPkgsInfoFromIds(options.parentIds, ctx.resolvedPkgsById)
        return err
      },
      updateToLatest: options.updateToLatest,
    })
  } catch (err: any) { // eslint-disable-line
    const wantedDependencyDetails = {
      name: wantedDependency.alias,
      pref: wantedDependency.pref,
      version: wantedDependency.alias ? wantedDependency.pref : undefined,
    }
    if (wantedDependency.optional) {
      skippedOptionalDependencyLogger.debug({
        details: err.toString(),
        package: wantedDependencyDetails,
        parents: getPkgsInfoFromIds(options.parentIds, ctx.resolvedPkgsById),
        prefix: options.prefix,
        reason: 'resolution_failure',
      })
      return null
    }
    err.package = wantedDependencyDetails
    err.prefix = options.prefix
    err.pkgsStack = getPkgsInfoFromIds(options.parentIds, ctx.resolvedPkgsById)
    throw err
  }

  dependencyResolvedLogger.debug({
    resolution: pkgResponse.body.id,
    wanted: {
      dependentId: options.parentPkg.pkgId,
      name: wantedDependency.alias,
      rawSpec: wantedDependency.pref,
    },
  })

  if (ctx.allPreferredVersions && pkgResponse.body.manifest?.version) {
    if (!ctx.allPreferredVersions[pkgResponse.body.manifest.name]) {
      ctx.allPreferredVersions[pkgResponse.body.manifest.name] = {}
    }
    ctx.allPreferredVersions[pkgResponse.body.manifest.name][pkgResponse.body.manifest.version] = 'version'
  }

  if (
    !pkgResponse.body.updated &&
    options.currentDepth === Math.max(0, options.updateDepth) &&
    depIsLinked && !ctx.force && !options.proceed
  ) {
    return null
  }

  if (pkgResponse.body.isLocal) {
    if (!pkgResponse.body.manifest) {
      // This should actually never happen because the local-resolver returns a manifest
      // even if no real manifest exists in the filesystem.
      throw new PnpmError('MISSING_PACKAGE_JSON', `Can't install ${wantedDependency.pref}: Missing package.json file`)
    }
    return {
      alias: wantedDependency.alias || pkgResponse.body.manifest.name || path.basename(pkgResponse.body.resolution.directory),
      dev: wantedDependency.dev,
      isLinkedDependency: true,
      name: pkgResponse.body.manifest.name,
      normalizedPref: pkgResponse.body.normalizedPref,
      optional: wantedDependency.optional,
      pkgId: pkgResponse.body.id,
      resolution: pkgResponse.body.resolution,
      version: pkgResponse.body.manifest.version,
    }
  }

  let prepare!: boolean
  let hasBin!: boolean
  let pkg: PackageManifest = getManifestFromResponse(pkgResponse, wantedDependency)
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
  let pkgIdWithPatchHash = (pkgResponse.body.id.startsWith(`${pkg.name}@`) ? pkgResponse.body.id : `${pkg.name}@${pkgResponse.body.id}`) as PkgIdWithPatchHash
  const nameAndVersion = `${pkg.name}@${pkg.version}`
  const patchFile = ctx.patchedDependencies?.[nameAndVersion]
  if (patchFile) {
    ctx.appliedPatches.add(nameAndVersion)
    pkgIdWithPatchHash = `${pkgIdWithPatchHash}(patch_hash=${patchFile.hash})` as PkgIdWithPatchHash
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
    parentIdsContainSequence(
      options.parentIds,
      options.parentPkg.pkgId,
      pkgResponse.body.id
    ) || pkgResponse.body.id === options.parentPkg.pkgId
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
    hasBin = currentPkg.dependencyLockfile.hasBin === true
    pkg = {
      ...nameVerFromPkgSnapshot(currentPkg.depPath, currentPkg.dependencyLockfile),
      ...omitDepsFields(currentPkg.dependencyLockfile),
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
  }
  if (options.currentDepth === 0 && pkgResponse.body.latest && pkgResponse.body.latest !== pkg.version) {
    ctx.outdatedDependencies[pkgResponse.body.id] = pkgResponse.body.latest
  }

  if (pkg.peerDependencies != null) {
    Object.keys(pkg.peerDependencies).forEach((name) => {
      ctx.allPeerDepNames.add(name)
    })
  }
  if (pkg.peerDependenciesMeta != null) {
    Object.keys(pkg.peerDependenciesMeta).forEach((name) => {
      ctx.allPeerDepNames.add(name)
    })
  }
  // In case of leaf dependencies (dependencies that have no prod deps or peer deps),
  // we only ever need to analyze one leaf dep in a graph, so the nodeId can be short and stateless.
  const nodeId = pkgIsLeaf(pkg) ? pkgResponse.body.id as unknown as NodeId : nextNodeId()

  const parentIsInstallable = options.parentPkg.installable === undefined || options.parentPkg.installable
  const installable = parentIsInstallable && pkgResponse.body.isInstallable !== false
  const isNew = !ctx.resolvedPkgsById[pkgResponse.body.id]
  const parentImporterId = options.parentIds[0]
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
    ctx.resolvedPkgsById[pkgResponse.body.id] = getResolvedPackage({
      allowBuild: ctx.allowBuild,
      dependencyLockfile: currentPkg.dependencyLockfile,
      pkgIdWithPatchHash,
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
    ctx.resolvedPkgsById[pkgResponse.body.id].prod = ctx.resolvedPkgsById[pkgResponse.body.id].prod || !wantedDependency.dev && !wantedDependency.optional
    ctx.resolvedPkgsById[pkgResponse.body.id].dev = ctx.resolvedPkgsById[pkgResponse.body.id].dev || wantedDependency.dev
    ctx.resolvedPkgsById[pkgResponse.body.id].optional = ctx.resolvedPkgsById[pkgResponse.body.id].optional && currentIsOptional
    if (ctx.resolvedPkgsById[pkgResponse.body.id].fetching == null && pkgResponse.fetching != null) {
      ctx.resolvedPkgsById[pkgResponse.body.id].fetching = pkgResponse.fetching
      ctx.resolvedPkgsById[pkgResponse.body.id].filesIndexFile = pkgResponse.filesIndexFile!
    }

    if (ctx.dependenciesTree.has(nodeId)) {
      ctx.dependenciesTree.get(nodeId)!.depth = Math.min(ctx.dependenciesTree.get(nodeId)!.depth, options.currentDepth)
    } else {
      ctx.pendingNodes.push({
        alias: wantedDependency.alias || pkg.name,
        depth: options.currentDepth,
        parentIds: options.parentIds,
        installable,
        nodeId,
        resolvedPackage: ctx.resolvedPkgsById[pkgResponse.body.id],
      })
    }
  }

  const rootDir = pkgResponse.body.resolution.type === 'directory'
    ? path.resolve(ctx.lockfileDir, (pkgResponse.body.resolution as DirectoryResolution).directory)
    : options.prefix
  let missingPeersOfChildren!: MissingPeersOfChildren | undefined
  if (ctx.hoistPeers && !options.parentIds.includes(pkgResponse.body.id)) {
    if (ctx.missingPeersOfChildrenByPkgId[pkgResponse.body.id]) {
      // This if condition is used to avoid a dead lock.
      // There might be a better way to hoist peer dependencies during resolution
      // but it would probably require a big rewrite of the resolution algorithm.
      if (
        ctx.missingPeersOfChildrenByPkgId[pkgResponse.body.id].depth >= options.currentDepth ||
        ctx.missingPeersOfChildrenByPkgId[pkgResponse.body.id].missingPeersOfChildren.resolved
      ) {
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
        depth: options.currentDepth,
        missingPeersOfChildren,
      }
    }
  }
  return {
    alias: wantedDependency.alias || pkg.name,
    depIsLinked,
    isNew,
    nodeId,
    normalizedPref: options.currentDepth === 0 ? pkgResponse.body.normalizedPref : undefined,
    missingPeersOfChildren,
    pkgId: pkgResponse.body.id,
    rootDir,
    missingPeers: getMissingPeers(pkg),
    optional: ctx.resolvedPkgsById[pkgResponse.body.id].optional,

    // Next fields are actually only needed when isNew = true
    installable,
    isLinkedDependency: undefined,
    pkg,
    updated: pkgResponse.body.updated,
    publishedAt: pkgResponse.body.publishedAt,
  }
}

function getManifestFromResponse (
  pkgResponse: PackageResponse,
  wantedDependency: WantedDependency
): PackageManifest {
  if (pkgResponse.body.manifest) return pkgResponse.body.manifest
  return {
    name: wantedDependency.pref.split('/').pop()!,
    version: '0.0.0',
  }
}

function getMissingPeers (pkg: PackageManifest): MissingPeers {
  const missingPeers = {} as MissingPeers
  for (const [peerName, peerVersion] of Object.entries(pkg.peerDependencies ?? {})) {
    missingPeers[peerName] = {
      range: peerVersion,
      optional: pkg.peerDependenciesMeta?.[peerName]?.optional === true,
    }
  }
  return missingPeers
}

function pkgIsLeaf (pkg: PackageManifest): boolean {
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
    pkgIdWithPatchHash: PkgIdWithPatchHash
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
    pkgIdWithPatchHash: options.pkgIdWithPatchHash,
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
    peerDependencies,
    prepare: options.prepare,
    prod: !options.wantedDependency.dev && !options.wantedDependency.optional,
    resolution: options.pkgResponse.body.resolution,
    version: options.pkg.version,
  }
}

function peerDependenciesWithoutOwn (pkg: PackageManifest): PeerDependencies {
  if ((pkg.peerDependencies == null) && (pkg.peerDependenciesMeta == null)) return {}
  const ownDeps = new Set([
    ...Object.keys(pkg.dependencies ?? {}),
    ...Object.keys(pkg.optionalDependencies ?? {}),
  ])
  const result: PeerDependencies = {}
  if (pkg.peerDependencies != null) {
    for (const [peerName, peerRange] of Object.entries(pkg.peerDependencies)) {
      if (ownDeps.has(peerName)) continue
      result[peerName] = {
        version: peerRange,
      }
    }
  }
  if (pkg.peerDependenciesMeta != null) {
    for (const [peerName, peerMeta] of Object.entries(pkg.peerDependenciesMeta)) {
      if (ownDeps.has(peerName) || peerMeta.optional !== true) continue
      if (!result[peerName]) result[peerName] = { version: '*' }
      result[peerName].optional = true
    }
  }
  return result
}
