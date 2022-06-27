import path from 'path'
import {
  deprecationLogger,
  progressLogger,
  skippedOptionalDependencyLogger,
} from '@pnpm/core-loggers'
import PnpmError from '@pnpm/error'
import {
  Lockfile,
  PackageSnapshot,
  ResolvedDependencies,
} from '@pnpm/lockfile-types'
import {
  nameVerFromPkgSnapshot,
  packageIdFromSnapshot,
  pkgSnapshotToResolution,
} from '@pnpm/lockfile-utils'
import logger from '@pnpm/logger'
import pickRegistryForPackage from '@pnpm/pick-registry-for-package'
import {
  DirectoryResolution,
  PreferredVersions,
  Resolution,
  WorkspacePackages,
} from '@pnpm/resolver-base'
import {
  PackageFilesResponse,
  PackageResponse,
  StoreController,
} from '@pnpm/store-controller-types'
import {
  AllowedDeprecatedVersions,
  Dependencies,
  DependencyManifest,
  PackageManifest,
  PatchFile,
  PeerDependenciesMeta,
  ReadPackageHook,
  Registries,
} from '@pnpm/types'
import * as dp from 'dependency-path'
import exists from 'path-exists'
import isEmpty from 'ramda/src/isEmpty'
import semver from 'semver'
import encodePkgId from './encodePkgId'
import getNonDevWantedDependencies, { WantedDependency } from './getNonDevWantedDependencies'
import { safeIntersect } from './mergePeers'
import {
  createNodeId,
  nodeIdContainsSequence,
  splitNodeId,
} from './nodeIdUtils'
import wantedDepIsLocallyAvailable from './wantedDepIsLocallyAvailable'
import safePromiseDefer, { SafePromiseDefer } from 'safe-promise-defer'

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

export interface DependenciesTree<T> {
  // a node ID is the join of the package's keypath with a colon
  // E.g., a subdeps node ID which parent is `foo` will be
  // registry.npmjs.org/foo/1.0.0:registry.npmjs.org/bar/1.0.0
  [nodeId: string]: DependenciesTreeNode<T>
}

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
  resolvedPackagesByDepPath: ResolvedPackagesByDepPath
  outdatedDependencies: {[pkgId: string]: string}
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
  prefix: string
  preferWorkspacePackages?: boolean
  readPackageHook?: ReadPackageHook
  engineStrict: boolean
  modulesDir: string
  nodeVersion: string
  pnpmVersion: string
  registries: Registries
  virtualStoreDir: string
  updateMatching?: (pkgName: string) => boolean
}

export type MissingPeers = Record<string, string>

export type ResolvedPeers = Record<string, PkgAddress>

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
  resolvedPeers: ResolvedPeers
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
  fetchingFiles: () => Promise<PackageFilesResponse>
  fetchingBundledManifest?: () => Promise<DependencyManifest | undefined>
  filesIndexFile: string
  finishing: () => Promise<void>
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
}

type ParentPkg = Pick<PkgAddress, 'nodeId' | 'installable' | 'depPath' | 'rootDir'>

type ParentPkgAliases = Record<string, PkgAddress | true>

interface ResolvedDependenciesOptions {
  currentDepth: number
  parentPkg: ParentPkg
  parentPkgAliases: ParentPkgAliases
  // If the package has been updated, the dependencies
  // which were used by the previous version are passed
  // via this option
  preferredDependencies?: ResolvedDependencies
  proceed: boolean
  resolvedDependencies?: ResolvedDependencies
  updateDepth: number
  workspacePackages?: WorkspacePackages
}

type PostponedResolutionFunction = (preferredVersions: PreferredVersions, parentPkgAliases: ParentPkgAliases) => Promise<{
  missingPeers: MissingPeers
  resolvedPeers: ResolvedPeers
}>

export async function resolveRootDependencies (
  ctx: ResolutionContext,
  preferredVersions: PreferredVersions,
  wantedDependencies: Array<WantedDependency & { updateDepth?: number }>,
  options: ResolvedDependenciesOptions
): Promise<Array<PkgAddress | LinkedDependency>> {
  const pkgAddresses: Array<PkgAddress | LinkedDependency> = []
  const parentPkgAliases: ParentPkgAliases = {}
  for (const wantedDep of wantedDependencies) {
    if (wantedDep.alias) {
      parentPkgAliases[wantedDep.alias] = true
    }
  }
  while (true) {
    const result = await resolveDependencies(ctx, preferredVersions, wantedDependencies, {
      ...options,
      parentPkgAliases,
    })
    pkgAddresses.push(...result.pkgAddresses)
    if (!ctx.autoInstallPeers) break
    for (const pkgAddress of result.pkgAddresses) {
      parentPkgAliases[pkgAddress.alias] = true
    }
    for (const missingPeerName of Object.keys(result.missingPeers ?? {})) {
      parentPkgAliases[missingPeerName] = true
    }
    // All the missing peers should get installed in the root.
    // Otherwise, pending nodes will not work.
    // even those peers should be hoisted that are not autoinstalled
    for (const [resolvedPeerName, resolvedPeerAddress] of Object.entries(result.resolvedPeers ?? {})) {
      if (!parentPkgAliases[resolvedPeerName]) {
        pkgAddresses.push(resolvedPeerAddress)
      }
    }
    if (!Object.keys(result.missingPeers).length) break
    wantedDependencies = getNonDevWantedDependencies({ dependencies: result.missingPeers })
  }
  return pkgAddresses
}

interface ResolvedDependenciesResult {
  pkgAddresses: Array<PkgAddress | LinkedDependency>
  missingPeers: MissingPeers
  resolvedPeers: ResolvedPeers
}

export async function resolveDependencies (
  ctx: ResolutionContext,
  preferredVersions: PreferredVersions,
  wantedDependencies: Array<WantedDependency & { updateDepth?: number }>,
  options: ResolvedDependenciesOptions
): Promise<ResolvedDependenciesResult> {
  const postponedResolutionsQueue: PostponedResolutionFunction[] = []
  const extendedWantedDeps = getDepsToResolve(wantedDependencies, ctx.wantedLockfile, {
    preferredDependencies: options.preferredDependencies,
    prefix: ctx.prefix,
    proceed: options.proceed || ctx.forceFullResolution,
    registries: ctx.registries,
    resolvedDependencies: options.resolvedDependencies,
  })
  const pkgAddresses = (
    await Promise.all(
      extendedWantedDeps.map(async (extendedWantedDep) => resolveDependenciesOfDependency(
        postponedResolutionsQueue,
        ctx,
        preferredVersions,
        options,
        extendedWantedDep
      ))
    )
  ).filter(Boolean) as PkgAddress[]
  const newPreferredVersions = { ...preferredVersions }
  const newParentPkgAliases = { ...options.parentPkgAliases }
  for (const pkgAddress of pkgAddresses) {
    if (newParentPkgAliases[pkgAddress.alias] !== true) {
      newParentPkgAliases[pkgAddress.alias] = pkgAddress
    }
    if (pkgAddress.updated) {
      ctx.updatedSet.add(pkgAddress.alias)
    }
    const resolvedPackage = ctx.resolvedPackagesByDepPath[pkgAddress.depPath]
    if (!resolvedPackage) continue // This will happen only with linked dependencies
    if (!newPreferredVersions[resolvedPackage.name]) {
      newPreferredVersions[resolvedPackage.name] = {}
    }
    newPreferredVersions[resolvedPackage.name][resolvedPackage.version] = 'version'
  }
  const childrenResults = await Promise.all(postponedResolutionsQueue.map(async (postponedResolution) => postponedResolution(newPreferredVersions, newParentPkgAliases)))
  if (!ctx.autoInstallPeers) {
    return {
      missingPeers: {},
      pkgAddresses,
      resolvedPeers: {},
    }
  }
  const allMissingPeers = mergePkgsDeps(
    [
      ...pkgAddresses,
      ...childrenResults,
    ].map(({ missingPeers }) => missingPeers).filter(Boolean)
  )
  return {
    missingPeers: allMissingPeers,
    pkgAddresses,
    resolvedPeers: [...pkgAddresses, ...childrenResults].reduce((acc, { resolvedPeers }) => Object.assign(acc, resolvedPeers), {}),
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

async function resolveDependenciesOfDependency (
  postponedResolutionsQueue: PostponedResolutionFunction[],
  ctx: ResolutionContext,
  preferredVersions: PreferredVersions,
  options: ResolvedDependenciesOptions,
  extendedWantedDep: ExtendedWantedDependency
) {
  const updateDepth = typeof extendedWantedDep.wantedDependency.updateDepth === 'number'
    ? extendedWantedDep.wantedDependency.updateDepth
    : options.updateDepth
  const updateShouldContinue = options.currentDepth <= updateDepth
  const update = ((extendedWantedDep.infoFromLockfile?.dependencyLockfile) == null) ||
  (
    updateShouldContinue && (
      (ctx.updateMatching == null) ||
      ctx.updateMatching(extendedWantedDep.infoFromLockfile.name!)
    )
  ) || Boolean(
    (options.workspacePackages != null) &&
    ctx.linkWorkspacePackagesDepth !== -1 &&
    wantedDepIsLocallyAvailable(
      options.workspacePackages,
      extendedWantedDep.wantedDependency,
      { defaultTag: ctx.defaultTag, registry: ctx.registries.default }
    )
  ) || ctx.updatedSet.has(extendedWantedDep.infoFromLockfile.name!)

  const resolveDependencyOpts: ResolveDependencyOptions = {
    currentDepth: options.currentDepth,
    parentPkg: options.parentPkg,
    parentPkgAliases: options.parentPkgAliases,
    preferredVersions,
    workspacePackages: options.workspacePackages,
    currentPkg: extendedWantedDep.infoFromLockfile ?? undefined,
    proceed: extendedWantedDep.proceed || updateShouldContinue || ctx.updatedSet.size > 0,
    update,
    updateDepth,
  }
  const resolveDependencyResult = await resolveDependency(extendedWantedDep.wantedDependency, ctx, resolveDependencyOpts)

  if (resolveDependencyResult == null) return null
  if (resolveDependencyResult.isLinkedDependency) {
    ctx.dependenciesTree[resolveDependencyResult.pkgId] = {
      children: {},
      depth: -1,
      installable: true,
      resolvedPackage: {
        name: resolveDependencyResult.name,
        version: resolveDependencyResult.version,
      },
    }
    return resolveDependencyResult
  }
  if (!resolveDependencyResult.isNew) return resolveDependencyResult

  postponedResolutionsQueue.push(async (preferredVersions, parentPkgAliases) =>
    resolveChildren(
      ctx,
      resolveDependencyResult,
      parentPkgAliases,
      extendedWantedDep.infoFromLockfile?.dependencyLockfile,
      options.workspacePackages,
      options.currentDepth,
      updateDepth,
      preferredVersions
    )
  )

  return resolveDependencyResult
}

async function resolveChildren (
  ctx: ResolutionContext,
  parentPkg: PkgAddress,
  parentPkgAliases: ParentPkgAliases,
  dependencyLockfile: PackageSnapshot | undefined,
  workspacePackages: WorkspacePackages | undefined,
  parentDepth: number,
  updateDepth: number,
  preferredVersions: PreferredVersions
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
    missingPeers,
    resolvedPeers,
  } = await resolveDependencies(ctx, preferredVersions, wantedDependencies,
    {
      currentDepth: parentDepth + 1,
      parentPkg,
      parentPkgAliases,
      preferredDependencies: currentResolvedDependencies,
      // If the package is not linked, we should also gather information about its dependencies.
      // After linking the package we'll need to symlink its dependencies.
      proceed: !parentPkg.depIsLinked || parentDependsOnPeer,
      resolvedDependencies,
      updateDepth,
      workspacePackages,
    }
  )
  ctx.childrenByParentDepPath[parentPkg.depPath] = pkgAddresses.map((child) => ({
    alias: child.alias,
    depPath: child.depPath,
  }))
  ctx.dependenciesTree[parentPkg.nodeId] = {
    children: pkgAddresses.reduce((chn, child) => {
      chn[child.alias] = child['nodeId'] ?? child.pkgId
      return chn
    }, {}),
    depth: parentDepth,
    installable: parentPkg.installable,
    resolvedPackage: ctx.resolvedPackagesByDepPath[parentPkg.depPath],
  }
  return {
    missingPeers,
    resolvedPeers,
  }
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
        satisfiesWanted(resolvedDependencies[wantedDependency.alias])
      ) {
        reference = resolvedDependencies[wantedDependency.alias]
      } else if (
        // If dependencies that were used by the previous version of the package
        // satisfy the newer version's requirements, then pnpm tries to keep
        // the previous dependency.
        // So for example, if foo@1.0.0 had bar@1.0.0 as a dependency
        // and foo was updated to 1.1.0 which depends on bar ^1.0.0
        // then bar@1.0.0 can be reused for foo@1.1.0
        semver.validRange(wantedDependency.pref) !== null && // eslint-disable-line
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
  wantedDep: {alias: string, pref: string},
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
} | {})

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
  proceed: boolean
  update: boolean
  updateDepth: number
  workspacePackages?: WorkspacePackages
}

async function resolveDependency (
  wantedDependency: WantedDependency,
  ctx: ResolutionContext,
  options: ResolveDependencyOptions
): Promise<PkgAddress | LinkedDependency | null> {
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
      workspacePackages: options.workspacePackages,
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
        prefix: ctx.prefix,
        reason: 'resolution_failure',
      })
      return null
    }
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
    const manifest = pkgResponse.body.manifest ?? await pkgResponse.bundledManifest!() // eslint-disable-line @typescript-eslint/dot-notation
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
  if (ctx.readPackageHook != null) {
    pkg = await ctx.readPackageHook(pkg)
  }
  if (!pkg.name) { // TODO: don't fail on optional dependencies
    throw new PnpmError('MISSING_PACKAGE_NAME', `Can't install ${wantedDependency.pref}: Missing package name`)
  }
  let depPath = dp.relative(ctx.registries, pkg.name, pkgResponse.body.id)
  const nameAndVersion = `${pkg.name}@${pkg.version}`
  const patchFile = ctx.patchedDependencies?.[nameAndVersion]
  if (patchFile) {
    ctx.appliedPatches.add(nameAndVersion)
    depPath += `_${patchFile.hash}`
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
    /* eslint-disable @typescript-eslint/dot-notation */
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
    hasBin = Boolean((pkg.bin && !isEmpty(pkg.bin)) ?? pkg.directories?.bin)
    /* eslint-enable @typescript-eslint/dot-notation */
  }
  if (options.currentDepth === 0 && pkgResponse.body.latest && pkgResponse.body.latest !== pkg.version) {
    ctx.outdatedDependencies[pkgResponse.body.id] = pkgResponse.body.latest
  }

  // In case of leaf dependencies (dependencies that have no prod deps or peer deps),
  // we only ever need to analyze one leaf dep in a graph, so the nodeId can be short and stateless.
  const nodeId = pkgIsLeaf(pkg)
    ? pkgResponse.body.id
    : createNodeId(options.parentPkg.nodeId, depPath)

  const parentIsInstallable = options.parentPkg.installable === undefined || options.parentPkg.installable
  const installable = parentIsInstallable && pkgResponse.body.isInstallable !== false
  const isNew = !ctx.resolvedPackagesByDepPath[depPath]

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
        prefix: ctx.prefix,
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

    ctx.resolvedPackagesByDepPath[depPath] = await getResolvedPackage({
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
    })
  } else {
    ctx.resolvedPackagesByDepPath[depPath].prod = ctx.resolvedPackagesByDepPath[depPath].prod || !wantedDependency.dev && !wantedDependency.optional
    ctx.resolvedPackagesByDepPath[depPath].dev = ctx.resolvedPackagesByDepPath[depPath].dev || wantedDependency.dev
    ctx.resolvedPackagesByDepPath[depPath].optional = ctx.resolvedPackagesByDepPath[depPath].optional && wantedDependency.optional
    if (ctx.resolvedPackagesByDepPath[depPath].fetchingFiles == null && pkgResponse.files != null) {
      ctx.resolvedPackagesByDepPath[depPath].fetchingFiles = pkgResponse.files
      ctx.resolvedPackagesByDepPath[depPath].filesIndexFile = pkgResponse.filesIndexFile!
      ctx.resolvedPackagesByDepPath[depPath].finishing = pkgResponse.finishing!
      ctx.resolvedPackagesByDepPath[depPath].fetchingBundledManifest = pkgResponse.bundledManifest!
    }

    if (ctx.dependenciesTree[nodeId]) {
      ctx.dependenciesTree[nodeId].depth = Math.min(ctx.dependenciesTree[nodeId].depth, options.currentDepth)
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
    ? path.resolve(ctx.lockfileDir, pkgResponse.body.resolution['directory'])
    : ctx.prefix
  return {
    alias: wantedDependency.alias || pkg.name,
    depIsLinked,
    depPath,
    isNew,
    nodeId,
    normalizedPref: options.currentDepth === 0 ? pkgResponse.body.normalizedPref : undefined,
    pkgId: pkgResponse.body.id,
    rootDir,
    ...getMissingPeers(pkg, options.parentPkgAliases),

    // Next fields are actually only needed when isNew = true
    installable,
    isLinkedDependency: undefined,
    pkg,
    updated: pkgResponse.body.updated,
  }
}

async function getManifestFromResponse (
  pkgResponse: PackageResponse,
  wantedDependency: WantedDependency
): Promise<PackageManifest> {
  const pkg = pkgResponse.body.manifest ?? await pkgResponse.bundledManifest!()
  if (pkg) return pkg
  return {
    name: wantedDependency.pref.split('/').pop()!,
    version: '0.0.0',
  }
}

function getMissingPeers (pkg: PackageManifest, parentPkgAliases: ParentPkgAliases) {
  const missingPeers = {} as MissingPeers
  const resolvedPeers = {} as ResolvedPeers
  for (const [peerName, peerVersion] of Object.entries(pkg.peerDependencies ?? {})) {
    if (parentPkgAliases[peerName]) {
      if (parentPkgAliases[peerName] !== true) {
        resolvedPeers[peerName] = parentPkgAliases[peerName] as PkgAddress
      }
    } else if (!pkg.peerDependenciesMeta?.[peerName]?.optional) {
      missingPeers[peerName] = peerVersion
    }
  }
  return { missingPeers, resolvedPeers }
}

function pkgIsLeaf (pkg: PackageManifest) {
  return isEmpty(pkg.dependencies ?? {}) &&
    isEmpty(pkg.optionalDependencies ?? {}) &&
    isEmpty(pkg.peerDependencies ?? {})
}

async function getResolvedPackage (
  options: {
    allowBuild?: (pkgName: string) => boolean
    dependencyLockfile?: PackageSnapshot
    depPath: string
    force: boolean
    hasBin: boolean
    patchFile?: PatchFile
    pkg: PackageManifest
    pkgResponse: PackageResponse
    prepare: boolean
    wantedDependency: WantedDependency
  }
): Promise<ResolvedPackage> {
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
    depPath: options.depPath,
    dev: options.wantedDependency.dev,
    fetchingBundledManifest: options.pkgResponse.bundledManifest,
    fetchingFiles: options.pkgResponse.files!,
    filesIndexFile: options.pkgResponse.filesIndexFile!,
    finishing: options.pkgResponse.finishing!,
    hasBin: options.hasBin,
    hasBundledDependencies: !((options.pkg.bundledDependencies ?? options.pkg.bundleDependencies) == null),
    id: options.pkgResponse.body.id,
    name: options.pkg.name,
    optional: options.wantedDependency.optional,
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
  const result = {}
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
  if (isEmpty(result)) return undefined
  return result
}
