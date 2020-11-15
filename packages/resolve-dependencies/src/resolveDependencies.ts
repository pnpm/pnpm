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
import packageIsInstallable from '@pnpm/package-is-installable'
import pickRegistryForPackage from '@pnpm/pick-registry-for-package'
import pkgIdToFilename from '@pnpm/pkgid-to-filename'
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
  Dependencies,
  DependencyManifest,
  PackageManifest,
  PeerDependenciesMeta,
  ReadPackageHook,
  Registries,
} from '@pnpm/types'
import * as dp from 'dependency-path'
import encodePkgId from './encodePkgId'
import getNonDevWantedDependencies, { WantedDependency } from './getNonDevWantedDependencies'
import {
  createNodeId,
  nodeIdContainsSequence,
  splitNodeId,
} from './nodeIdUtils'
import wantedDepIsLocallyAvailable from './wantedDepIsLocallyAvailable'
import path = require('path')
import exists = require('path-exists')
import R = require('ramda')
import semver = require('semver')

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
  alwaysTryWorkspacePackages?: boolean
  defaultTag: string
  dryRun: boolean
  forceFullResolution: boolean
  resolvedPackagesByDepPath: ResolvedPackagesByDepPath
  outdatedDependencies: {[pkgId: string]: string}
  childrenByParentDepPath: ChildrenByParentDepPath
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
  fetchingBundledManifest?: () => Promise<DependencyManifest>
  filesIndexFile: string
  finishing: () => Promise<void>
  name: string
  version: string
  peerDependencies: Dependencies
  peerDependenciesMeta?: PeerDependenciesMeta
  optionalDependencies: Set<string>
  hasBin: boolean
  hasBundledDependencies: boolean
  prepare: boolean
  depPath: string
  requiresBuild: boolean | undefined // added to fix issue #1201
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
  }
}

type ParentPkg = Pick<PkgAddress, 'nodeId' | 'installable' | 'depPath'>

interface ResolvedDependenciesOptions {
  currentDepth: number
  parentPkg: ParentPkg
  // If the package has been updated, the dependencies
  // which were used by the previous version are passed
  // via this option
  preferredDependencies?: ResolvedDependencies
  proceed: boolean
  resolvedDependencies?: ResolvedDependencies
  updateDepth: number
  workspacePackages?: WorkspacePackages
}

export default async function resolveDependencies (
  ctx: ResolutionContext,
  preferredVersions: PreferredVersions,
  wantedDependencies: Array<WantedDependency & { updateDepth?: number }>,
  options: ResolvedDependenciesOptions
): Promise<Array<PkgAddress | LinkedDependency>> {
  const extendedWantedDeps = getDepsToResolve(wantedDependencies, ctx.wantedLockfile, {
    preferredDependencies: options.preferredDependencies,
    prefix: ctx.prefix,
    proceed: options.proceed || ctx.forceFullResolution,
    registries: ctx.registries,
    resolvedDependencies: options.resolvedDependencies,
  })
  const postponedResolutionsQueue = [] as Array<(preferredVersions: PreferredVersions) => Promise<void>>
  const pkgAddresses = (
    await Promise.all(
      extendedWantedDeps.map((extendedWantedDep) => resolveDependenciesOfDependency(
        postponedResolutionsQueue,
        ctx,
        preferredVersions,
        options,
        extendedWantedDep
      ))
    )
  )
    .filter(Boolean) as PkgAddress[]

  const newPreferredVersions = { ...preferredVersions }
  for (const { depPath } of pkgAddresses) {
    const resolvedPackage = ctx.resolvedPackagesByDepPath[depPath]
    if (!resolvedPackage) continue // This will happen only with linked dependencies
    if (!newPreferredVersions[resolvedPackage.name]) {
      newPreferredVersions[resolvedPackage.name] = {}
    }
    newPreferredVersions[resolvedPackage.name][resolvedPackage.version] = 'version'
  }
  await Promise.all(postponedResolutionsQueue.map((postponedResolution) => postponedResolution(newPreferredVersions)))

  return pkgAddresses
}

interface ExtendedWantedDependency {
  infoFromLockfile?: InfoFromLockfile
  proceed: boolean
  wantedDependency: WantedDependency & { updateDepth?: number }
}

async function resolveDependenciesOfDependency (
  postponedResolutionsQueue: Array<(preferredVersions: PreferredVersions) => Promise<void>>,
  ctx: ResolutionContext,
  preferredVersions: PreferredVersions,
  options: ResolvedDependenciesOptions,
  extendedWantedDep: ExtendedWantedDependency
) {
  const updateDepth = typeof extendedWantedDep.wantedDependency.updateDepth === 'number'
    ? extendedWantedDep.wantedDependency.updateDepth : options.updateDepth
  const updateShouldContinue = options.currentDepth <= updateDepth
  const update = (
    updateShouldContinue && (
      !ctx.updateMatching ||
      !extendedWantedDep.infoFromLockfile?.dependencyLockfile ||
      ctx.updateMatching(extendedWantedDep.infoFromLockfile.dependencyLockfile.name ?? extendedWantedDep.wantedDependency.alias)
    )
  ) || Boolean(
    options.workspacePackages &&
    wantedDepIsLocallyAvailable(
      options.workspacePackages,
      extendedWantedDep.wantedDependency,
      { defaultTag: ctx.defaultTag, registry: ctx.registries.default }
    )
  )
  const resolveDependencyOpts: ResolveDependencyOptions = {
    currentDepth: options.currentDepth,
    parentPkg: options.parentPkg,
    preferredVersions,
    workspacePackages: options.workspacePackages,
    currentPkg: extendedWantedDep.infoFromLockfile ?? undefined,
    proceed: extendedWantedDep.proceed || updateShouldContinue,
    update,
    updateDepth,
  }
  const resolveDependencyResult = await resolveDependency(extendedWantedDep.wantedDependency, ctx, resolveDependencyOpts)

  if (!resolveDependencyResult) return null
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

  postponedResolutionsQueue.push(async (preferredVersions) =>
    resolveChildren(
      ctx,
      resolveDependencyResult,
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
  dependencyLockfile: PackageSnapshot | undefined,
  workspacePackages: WorkspacePackages | undefined,
  parentDepth: number,
  updateDepth: number,
  preferredVersions: PreferredVersions
) {
  const currentResolvedDependencies = dependencyLockfile ? {
    ...dependencyLockfile.dependencies,
    ...dependencyLockfile.optionalDependencies,
  } : undefined
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
  workspacePackages = workspacePackages && ctx.linkWorkspacePackagesDepth > parentDepth
    ? workspacePackages : undefined
  const children = await resolveDependencies(ctx, preferredVersions, wantedDependencies,
    {
      currentDepth: parentDepth + 1,
      parentPkg,
      preferredDependencies: currentResolvedDependencies,
      // If the package is not linked, we should also gather information about its dependencies.
      // After linking the package we'll need to symlink its dependencies.
      proceed: !parentPkg.depIsLinked || parentDependsOnPeer,
      resolvedDependencies,
      updateDepth,
      workspacePackages,
    }
  )
  ctx.childrenByParentDepPath[parentPkg.depPath] = children.map((child) => ({
    alias: child.alias,
    depPath: child.depPath,
  }))
  ctx.dependenciesTree[parentPkg.nodeId] = {
    children: children.reduce((chn, child) => {
      chn[child.alias] = child['nodeId'] ?? child.pkgId
      return chn
    }, {}),
    depth: parentDepth,
    installable: parentPkg.installable,
    resolvedPackage: ctx.resolvedPackagesByDepPath[parentPkg.depPath],
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
  const allPeers = new Set<string>()
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
      infoFromLockfile?.dependencyLockfile?.peerDependencies
    ) {
      proceed = true
      Object.keys(infoFromLockfile.dependencyLockfile.peerDependencies).forEach((peerName) => {
        allPeers.add(peerName)
      })
    }
    if (!infoFromLockfile && !proceedAll) {
      // In this case we don't know if the package depends on peer dependencies, so we proceed all.
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
  if (!proceedAll && allPeers.size) {
    for (const extendedWantedDep of extendedWantedDeps) {
      if (!extendedWantedDep.proceed && allPeers.has(extendedWantedDep.wantedDependency.alias)) {
        extendedWantedDep.proceed = true
      }
    }
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
  if (!pkgSnapshot) {
    logger.warn({
      message: `Could not find preferred package ${depPath} in lockfile`,
      prefix: opts.prefix,
    })
    return false
  }
  const { version } = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
  return semver.satisfies(version, wantedDep.pref, true)
}

interface InfoFromLockfile {
  dependencyLockfile?: PackageSnapshot
  depPath: string
  pkgId: string
  resolution?: Resolution
}

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

  if (dependencyLockfile) {
    if (dependencyLockfile.peerDependencies && dependencyLockfile.dependencies) {
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
      dependencyLockfile,
      depPath,
      pkgId: packageIdFromSnapshot(depPath, dependencyLockfile, registries),
      resolution: pkgSnapshotToResolution(depPath, dependencyLockfile, registries),
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
    pkgId?: string
    resolution?: Resolution
    dependencyLockfile?: PackageSnapshot
  }
  parentPkg: ParentPkg
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
    ? Boolean(ctx.currentLockfile.packages?.[currentPkg.depPath]) : undefined
  const depIsLinked = Boolean(
    // if package is not in `node_modules/.pnpm-lock.yaml`
    // we can safely assume that it doesn't exist in `node_modules`
    currentLockfileContainsTheDep &&
    currentPkg.depPath &&
    currentPkg.dependencyLockfile &&
    await exists(
      path.join(
        ctx.virtualStoreDir,
        pkgIdToFilename(currentPkg.depPath, ctx.prefix),
        'node_modules',
        nameVerFromPkgSnapshot(currentPkg.depPath, currentPkg.dependencyLockfile).name,
        'package.json'
      )
    )
  )

  if (!options.update && !options.proceed && currentPkg.resolution && depIsLinked) {
    return null
  }

  let pkgResponse!: PackageResponse
  try {
    pkgResponse = await ctx.storeController.requestPackage(wantedDependency, {
      alwaysTryWorkspacePackages: ctx.alwaysTryWorkspacePackages,
      currentPackageId: currentPkg.pkgId,
      currentResolution: currentPkg.resolution,
      defaultTag: ctx.defaultTag,
      downloadPriority: -options.currentDepth,
      lockfileDir: ctx.lockfileDir,
      preferredVersions: options.preferredVersions,
      preferWorkspacePackages: ctx.preferWorkspacePackages,
      projectDir: options.currentDepth > 0 ? ctx.lockfileDir : ctx.prefix,
      registry: wantedDependency.alias && pickRegistryForPackage(ctx.registries, wantedDependency.alias) || ctx.registries.default,
      // Unfortunately, even when run with --lockfile-only, we need the *real* package.json
      // so fetching of the tarball cannot be ever avoided. Related issue: https://github.com/pnpm/pnpm/issues/1176
      skipFetch: false,
      update: options.update,
      workspacePackages: options.workspacePackages,
    })
  } catch (err) {
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

  let pkg: PackageManifest
  let prepare!: boolean
  let hasBin!: boolean
  pkg = ctx.readPackageHook
    ? ctx.readPackageHook(pkgResponse.body.manifest ?? await pkgResponse.bundledManifest!())
    : pkgResponse.body.manifest ?? await pkgResponse.bundledManifest!()
  if (!pkg.name) { // TODO: don't fail on optional dependencies
    throw new PnpmError('MISSING_PACKAGE_NAME', `Can't install ${wantedDependency.pref}: Missing package name`)
  }
  const depPath = dp.relative(ctx.registries, pkg.name, pkgResponse.body.id)

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
  // peers resolved, after the first ocurrence.
  //
  // However, in the next example we would analyze the second qar as well,
  // because zoo is a new parent package:
  // foo > bar > qar > zoo > qar
  if (
    nodeIdContainsSequence(
      options.parentPkg.nodeId,
      options.parentPkg.depPath,
      depPath
    )
  ) {
    return null
  }

  if (
    !options.update && currentPkg.dependencyLockfile && currentPkg.depPath &&
    !pkgResponse.body.updated &&
    // peerDependencies field is also used for transitive peer dependencies which should not be linked
    // That's why we cannot omit reading package.json of such dependencies.
    // This can be removed if we implement something like peerDependenciesMeta.transitive: true
    !currentPkg.dependencyLockfile.peerDependencies
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
    hasBin = Boolean((pkg.bin && !R.isEmpty(pkg.bin)) ?? pkg.directories?.bin)
    /* eslint-enable @typescript-eslint/dot-notation */
  }
  if (options.currentDepth === 0 && pkgResponse.body.latest && pkgResponse.body.latest !== pkg.version) {
    ctx.outdatedDependencies[pkgResponse.body.id] = pkgResponse.body.latest
  }
  if (pkg.deprecated) {
    deprecationLogger.debug({
      deprecated: pkg.deprecated,
      depth: options.currentDepth,
      pkgId: pkgResponse.body.id,
      pkgName: pkg.name,
      pkgVersion: pkg.version,
      prefix: ctx.prefix,
    })
  }

  // In case of leaf dependencies (dependencies that have no prod deps or peer deps),
  // we only ever need to analyze one leaf dep in a graph, so the nodeId can be short and stateless.
  const nodeId = pkgIsLeaf(pkg)
    ? pkgResponse.body.id
    : createNodeId(options.parentPkg.nodeId, depPath)

  const currentIsInstallable = (
    ctx.force ||
      packageIsInstallable(pkgResponse.body.id, pkg, {
        engineStrict: ctx.engineStrict,
        lockfileDir: ctx.lockfileDir,
        nodeVersion: ctx.nodeVersion,
        optional: wantedDependency.optional,
        pnpmVersion: ctx.pnpmVersion,
      })
  )
  const parentIsInstallable = options.parentPkg.installable === undefined || options.parentPkg.installable
  const installable = parentIsInstallable && currentIsInstallable !== false
  const isNew = !ctx.resolvedPackagesByDepPath[depPath]

  if (isNew) {
    if (currentIsInstallable !== true || !parentIsInstallable) {
      ctx.skipped.add(pkgResponse.body.id)
    }
    progressLogger.debug({
      packageId: pkgResponse.body.id,
      requester: ctx.lockfileDir,
      status: 'resolved',
    })
    if (pkgResponse.files) {
      pkgResponse.files()
        .then((fetchResult: PackageFilesResponse) => {
          progressLogger.debug({
            packageId: pkgResponse.body.id,
            requester: ctx.lockfileDir,
            status: fetchResult.fromStore
              ? 'found_in_store' : 'fetched',
          })
        })
        .catch(() => {
          // Ignore
        })
    }

    ctx.resolvedPackagesByDepPath[depPath] = getResolvedPackage({
      dependencyLockfile: currentPkg.dependencyLockfile,
      depPath,
      force: ctx.force,
      hasBin,
      pkg,
      pkgResponse,
      prepare,
      wantedDependency,
    })
  } else {
    ctx.resolvedPackagesByDepPath[depPath].prod = ctx.resolvedPackagesByDepPath[depPath].prod || !wantedDependency.dev && !wantedDependency.optional
    ctx.resolvedPackagesByDepPath[depPath].dev = ctx.resolvedPackagesByDepPath[depPath].dev || wantedDependency.dev
    ctx.resolvedPackagesByDepPath[depPath].optional = ctx.resolvedPackagesByDepPath[depPath].optional && wantedDependency.optional

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

  return {
    alias: wantedDependency.alias || pkg.name,
    depIsLinked,
    depPath,
    isNew,
    nodeId,
    normalizedPref: options.currentDepth === 0 ? pkgResponse.body.normalizedPref : undefined,
    pkgId: pkgResponse.body.id,

    // Next fields are actually only needed when isNew = true
    installable,
    isLinkedDependency: undefined,
    pkg,
    updated: pkgResponse.body.updated,
  }
}

function pkgIsLeaf (pkg: PackageManifest) {
  return R.isEmpty(pkg.dependencies ?? {}) &&
    R.isEmpty(pkg.optionalDependencies ?? {}) &&
    R.isEmpty(pkg.peerDependencies ?? {})
}

function getResolvedPackage (
  options: {
    dependencyLockfile?: PackageSnapshot
    depPath: string
    force: boolean
    hasBin: boolean
    pkg: PackageManifest
    pkgResponse: PackageResponse
    prepare: boolean
    wantedDependency: WantedDependency
  }
) {
  const peerDependencies = peerDependenciesWithoutOwn(options.pkg)

  return {
    additionalInfo: {
      bundledDependencies: options.pkg.bundledDependencies,
      bundleDependencies: options.pkg.bundleDependencies,
      cpu: options.pkg.cpu,
      deprecated: options.pkg.deprecated,
      engines: options.pkg.engines,
      os: options.pkg.os,
    },
    depPath: options.depPath,
    dev: options.wantedDependency.dev,
    fetchingBundledManifest: options.pkgResponse.bundledManifest,
    fetchingFiles: options.pkgResponse.files!,
    filesIndexFile: options.pkgResponse.filesIndexFile!,
    finishing: options.pkgResponse.finishing!,
    hasBin: options.hasBin,
    hasBundledDependencies: !!(options.pkg.bundledDependencies ?? options.pkg.bundleDependencies),
    id: options.pkgResponse.body.id,
    name: options.pkg.name,
    optional: options.wantedDependency.optional,
    optionalDependencies: new Set(R.keys(options.pkg.optionalDependencies)),
    peerDependencies: peerDependencies ?? {},
    peerDependenciesMeta: options.pkg.peerDependenciesMeta,
    prepare: options.prepare,
    prod: !options.wantedDependency.dev && !options.wantedDependency.optional,
    requiresBuild: options.dependencyLockfile && Boolean(options.dependencyLockfile.requiresBuild),
    resolution: options.pkgResponse.body.resolution,
    version: options.pkg.version,
  }
}

function peerDependenciesWithoutOwn (pkg: PackageManifest) {
  if (!pkg.peerDependencies && !pkg.peerDependenciesMeta) return pkg.peerDependencies
  const ownDeps = new Set([
    ...Object.keys(pkg.dependencies ?? {}),
    ...Object.keys(pkg.optionalDependencies ?? {}),
  ])
  const result = {}
  if (pkg.peerDependencies) {
    for (const [peerName, peerRange] of Object.entries(pkg.peerDependencies)) {
      if (ownDeps.has(peerName)) continue
      result[peerName] = peerRange
    }
  }
  if (pkg.peerDependenciesMeta) {
    for (const [peerName, peerMeta] of Object.entries(pkg.peerDependenciesMeta)) {
      if (ownDeps.has(peerName) || result[peerName] || peerMeta.optional !== true) continue
      result[peerName] = '*'
    }
  }
  if (R.isEmpty(result)) return undefined
  return result
}
