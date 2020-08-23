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
  resolvedPackagesByPackageId: ResolvedPackagesByPackageId
) {
  return splitNodeId(nodeId).slice(1)
    .map((pkgId) => {
      const { id, name, version } = resolvedPackagesByPackageId[pkgId]
      return { id, name, version }
    })
}

// child nodeId by child alias name in case of non-linked deps
export interface ChildrenMap {
  [alias: string]: string,
}

export type DependenciesTreeNode = {
  children: (() => ChildrenMap) | ChildrenMap,
  installable: boolean,
} & ({
  resolvedPackage: ResolvedPackage,
  depth: number,
} | {
  resolvedPackage: { version: string },
  depth: -1,
})

export interface DependenciesTree {
  // a node ID is the join of the package's keypath with a colon
  // E.g., a subdeps node ID which parent is `foo` will be
  // registry.npmjs.org/foo/1.0.0:registry.npmjs.org/bar/1.0.0
  [nodeId: string]: DependenciesTreeNode,
}

export interface ResolvedPackagesByPackageId {
  [packageId: string]: ResolvedPackage,
}

export interface LinkedDependency {
  isLinkedDependency: true,
  optional: boolean,
  dev: boolean,
  resolution: DirectoryResolution,
  pkgId: string,
  version: string,
  name: string,
  normalizedPref?: string,
  alias: string,
}

export interface PendingNode {
  alias: string,
  nodeId: string,
  resolvedPackage: ResolvedPackage,
  depth: number,
  installable: boolean,
}

export interface ChildrenByParentId {
  [parentId: string]: Array<{
    alias: string,
    pkgId: string,
  }>,
}

export interface ResolutionContext {
  alwaysTryWorkspacePackages?: boolean,
  defaultTag: string,
  dryRun: boolean,
  forceFullResolution: boolean,
  resolvedPackagesByPackageId: ResolvedPackagesByPackageId,
  outdatedDependencies: {[pkgId: string]: string},
  childrenByParentId: ChildrenByParentId,
  pendingNodes: PendingNode[],
  wantedLockfile: Lockfile,
  currentLockfile: Lockfile,
  linkWorkspacePackagesDepth: number,
  lockfileDir: string,
  storeController: StoreController,
  // the IDs of packages that are not installable
  skipped: Set<string>,
  dependenciesTree: DependenciesTree,
  force: boolean,
  prefix: string,
  readPackageHook?: ReadPackageHook,
  engineStrict: boolean,
  modulesDir: string,
  nodeVersion: string,
  pnpmVersion: string,
  registries: Registries,
  virtualStoreDir: string,
  updateMatching?: (pkgName: string) => boolean,
}

export type PkgAddress = {
  alias: string,
  depIsLinked: boolean,
  isNew: boolean,
  isLinkedDependency?: false,
  nodeId: string,
  pkgId: string,
  normalizedPref?: string, // is returned only for root dependencies
  installable: boolean,
  pkg: PackageManifest,
  version?: string,
  updated: boolean,
  useManifestInfoFromLockfile: boolean,
} & ({
  isLinkedDependency: true,
  version: string,
} | {
  isLinkedDependency: undefined,
})

export interface ResolvedPackage {
  id: string,
  resolution: Resolution,
  prod: boolean,
  dev: boolean,
  optional: boolean,
  fetchingFiles: () => Promise<PackageFilesResponse>,
  fetchingBundledManifest?: () => Promise<DependencyManifest>,
  filesIndexFile: string,
  finishing: () => Promise<void>,
  name: string,
  version: string,
  peerDependencies: Dependencies,
  optionalDependencies: Set<string>,
  hasBin: boolean,
  hasBundledDependencies: boolean,
  prepare: boolean,
  depPath: string,
  requiresBuild: boolean | undefined, // added to fix issue #1201
  additionalInfo: {
    deprecated?: string,
    peerDependencies?: Dependencies,
    peerDependenciesMeta?: PeerDependenciesMeta,
    bundleDependencies?: string[],
    bundledDependencies?: string[],
    engines?: {
      node?: string,
      npm?: string,
    },
    cpu?: string[],
    os?: string[],
  },
}

type ParentPkg = Pick<PkgAddress, 'nodeId' | 'installable' | 'pkgId'>

export default async function resolveDependencies (
  ctx: ResolutionContext,
  preferredVersions: PreferredVersions,
  wantedDependencies: Array<WantedDependency & { updateDepth?: number }>,
  options: {
    currentDepth: number,
    parentPkg: ParentPkg,
    // If the package has been updated, the dependencies
    // which were used by the previous version are passed
    // via this option
    preferredDependencies?: ResolvedDependencies,
    proceed: boolean,
    resolvedDependencies?: ResolvedDependencies,
    updateDepth: number,
    workspacePackages?: WorkspacePackages,
  }
): Promise<Array<PkgAddress | LinkedDependency>> {
  const extendedWantedDeps = getDepsToResolve(wantedDependencies, ctx.wantedLockfile, {
    preferredDependencies: options.preferredDependencies,
    prefix: ctx.prefix,
    proceed: options.proceed || ctx.forceFullResolution,
    registries: ctx.registries,
    resolvedDependencies: options.resolvedDependencies,
  })
  const resolveDepOpts = {
    currentDepth: options.currentDepth,
    parentPkg: options.parentPkg,
    preferredVersions,
    workspacePackages: options.workspacePackages,
  }
  const postponedResolutionsQueue = [] as Array<(preferredVersions: PreferredVersions) => Promise<void>>
  const resDeps = resolveDependencies.bind(null, ctx)
  const pkgAddresses = (
    await Promise.all(
      extendedWantedDeps
        .map(async (extendedWantedDep) => {
          const updateDepth = typeof extendedWantedDep.wantedDependency.updateDepth === 'number'
            ? extendedWantedDep.wantedDependency.updateDepth : options.updateDepth
          const updateShouldContinue = options.currentDepth <= updateDepth
          const update = updateShouldContinue && (
            !ctx.updateMatching ||
            !extendedWantedDep.infoFromLockfile?.dependencyLockfile ||
            ctx.updateMatching(extendedWantedDep.infoFromLockfile.dependencyLockfile.name ?? extendedWantedDep.wantedDependency.alias)
          )
          const resolveDependencyOpts: ResolveDependencyOptions = {
            ...resolveDepOpts,
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
                version: resolveDependencyResult.version,
              },
            }
            return resolveDependencyResult
          }
          if (!resolveDependencyResult.isNew) return resolveDependencyResult

          const resolveChildren = async function (preferredVersions: PreferredVersions) {
            const resolvedPackage = ctx.resolvedPackagesByPackageId[resolveDependencyResult.pkgId]
            const currentResolvedDependencies = extendedWantedDep.infoFromLockfile?.dependencyLockfile ? {
              ...extendedWantedDep.infoFromLockfile.dependencyLockfile.dependencies,
              ...extendedWantedDep.infoFromLockfile.dependencyLockfile.optionalDependencies,
            } : undefined
            const resolvedDependencies = resolveDependencyResult.updated
              ? undefined
              : currentResolvedDependencies
            const optionalDependencyNames = Object.keys(extendedWantedDep.infoFromLockfile?.dependencyLockfile?.optionalDependencies ?? {})
            const workspacePackages = options.workspacePackages && ctx.linkWorkspacePackagesDepth > options.currentDepth
              ? options.workspacePackages : undefined
            const parentDependsOnPeer = Boolean(
              Object.keys(resolveDependencyOpts.currentPkg?.dependencyLockfile?.peerDependencies ?? resolveDependencyResult.pkg.peerDependencies ?? {}).length
            )
            const children = await resDeps(preferredVersions,
              getWantedDependencies(resolveDependencyResult.pkg, {
                optionalDependencyNames,
                resolvedDependencies,
                useManifestInfoFromLockfile: resolveDependencyResult.useManifestInfoFromLockfile,
              }),
              {
                currentDepth: options.currentDepth + 1,
                parentPkg: resolveDependencyResult,
                preferredDependencies: resolveDependencyResult.updated
                  ? currentResolvedDependencies
                  : undefined,
                // If the package is not linked, we should also gather information about its dependencies.
                // After linking the package we'll need to symlink its dependencies.
                proceed: !resolveDependencyResult.depIsLinked || parentDependsOnPeer,
                resolvedDependencies,
                updateDepth,
                workspacePackages,
              }
            ) as PkgAddress[]
            ctx.childrenByParentId[resolveDependencyResult.pkgId] = children.map((child) => ({
              alias: child.alias,
              pkgId: child.pkgId,
            }))
            ctx.dependenciesTree[resolveDependencyResult.nodeId] = {
              children: children.reduce((chn, child) => {
                chn[child.alias] = child.nodeId ?? child.pkgId
                return chn
              }, {}),
              depth: options.currentDepth,
              installable: resolveDependencyResult.installable,
              resolvedPackage,
            }
          }

          postponedResolutionsQueue.push(resolveChildren)

          return resolveDependencyResult
        })
    )
  )
    .filter(Boolean) as PkgAddress[]

  const newPreferredVersions = { ...preferredVersions }
  for (const { pkgId } of pkgAddresses) {
    const resolvedPackage = ctx.resolvedPackagesByPackageId[pkgId]
    if (!resolvedPackage) continue // This will happen only with linked dependencies
    if (!newPreferredVersions[resolvedPackage.name]) {
      newPreferredVersions[resolvedPackage.name] = {}
    }
    newPreferredVersions[resolvedPackage.name][resolvedPackage.version] = 'version'
  }
  await Promise.all(postponedResolutionsQueue.map((postponedResolution) => postponedResolution(newPreferredVersions)))

  return pkgAddresses
}

function getDepsToResolve (
  wantedDependencies: Array<WantedDependency & { updateDepth?: number }>,
  wantedLockfile: Lockfile,
  options: {
    preferredDependencies?: ResolvedDependencies,
    prefix: string,
    proceed: boolean,
    registries: Registries,
    resolvedDependencies?: ResolvedDependencies,
  }
) {
  const resolvedDependencies = options.resolvedDependencies ?? {}
  const preferredDependencies = options.preferredDependencies ?? {}
  const extendedWantedDeps = []
  // The only reason we resolve children in case the package depends on peers
  // is to get information about the existing dependencies, so that they can
  // be merged with the resolved peers.
  const proceedAll = options.proceed
  const allPeers = new Set<string>()
  for (const wantedDependency of wantedDependencies) {
    let reference = wantedDependency.alias && resolvedDependencies[wantedDependency.alias]
    let proceed = proceedAll

    // If dependencies that were used by the previous version of the package
    // satisfy the newer version's requirements, then pnpm tries to keep
    // the previous dependency.
    // So for example, if foo@1.0.0 had bar@1.0.0 as a dependency
    // and foo was updated to 1.1.0 which depends on bar ^1.0.0
    // then bar@1.0.0 can be reused for foo@1.1.0
    if (!reference && wantedDependency.alias && semver.validRange(wantedDependency.pref) !== null && // eslint-disable-line
      preferredDependencies[wantedDependency.alias] &&
      preferedSatisfiesWanted(
        preferredDependencies[wantedDependency.alias],
        wantedDependency as {alias: string, pref: string},
        wantedLockfile,
        {
          prefix: options.prefix,
        }
      )
    ) {
      proceed = true
      reference = preferredDependencies[wantedDependency.alias]
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

function preferedSatisfiesWanted (
  preferredRef: string,
  wantedDep: {alias: string, pref: string},
  lockfile: Lockfile,
  opts: {
    prefix: string,
  }
) {
  const depPath = dp.refToRelative(preferredRef, wantedDep.alias)
  if (depPath === null) return false
  const pkgSnapshot = lockfile.packages?.[depPath]
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

function getInfoFromLockfile (
  lockfile: Lockfile,
  registries: Registries,
  reference: string | undefined,
  alias: string | undefined
) {
  if (!reference || !alias) {
    return null
  }

  const depPath = dp.refToRelative(reference, alias)

  if (!depPath) {
    return null
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
      pkgId: dp.tryGetPackageId(registries, depPath) || depPath, // Does it make sense to set pkgId when we're not sure?
    }
  }
}

interface ResolveDependencyOptions {
  currentDepth: number,
  currentPkg?: {
    depPath?: string,
    pkgId?: string,
    resolution?: Resolution,
    dependencyLockfile?: PackageSnapshot,
  },
  parentPkg: ParentPkg,
  preferredVersions: PreferredVersions,
  proceed: boolean,
  update: boolean,
  updateDepth: number,
  workspacePackages?: WorkspacePackages,
}

async function resolveDependency (
  wantedDependency: WantedDependency,
  ctx: ResolutionContext,
  options: ResolveDependencyOptions
): Promise<PkgAddress | LinkedDependency | null> {
  const update = Boolean(
    options.update ||
    options.workspacePackages &&
    wantedDepIsLocallyAvailable(options.workspacePackages, wantedDependency, { defaultTag: ctx.defaultTag, registry: ctx.registries.default }))
  const currentPkg = options.currentPkg ?? {}
  const proceed = update || options.proceed || !currentPkg.resolution
  const parentIsInstallable = options.parentPkg.installable === undefined || options.parentPkg.installable

  const currentLockfileContainsTheDep = currentPkg.depPath
    ? Boolean(ctx.currentLockfile.packages?.[currentPkg.depPath]) : undefined
  const depIsLinked = Boolean(
    // if package is not in `node_modules/.pnpm-lock.yaml`
    // we can safely assume that it doesn't exist in `node_modules`
    currentLockfileContainsTheDep &&
    currentPkg.depPath && currentPkg.dependencyLockfile &&
    await exists(path.join(ctx.virtualStoreDir, `${pkgIdToFilename(currentPkg.depPath, ctx.prefix)}/node_modules/${nameVerFromPkgSnapshot(currentPkg.depPath, currentPkg.dependencyLockfile).name}/package.json`)) &&
    (options.currentDepth > 0 || wantedDependency.alias && await exists(path.join(ctx.modulesDir, wantedDependency.alias))))

  if (!proceed && depIsLinked) {
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
      projectDir: options.currentDepth > 0 ? ctx.lockfileDir : ctx.prefix,
      registry: wantedDependency.alias && pickRegistryForPackage(ctx.registries, wantedDependency.alias) || ctx.registries.default,
      // Unfortunately, even when run with --lockfile-only, we need the *real* package.json
      // so fetching of the tarball cannot be ever avoided. Related issue: https://github.com/pnpm/pnpm/issues/1176
      skipFetch: false,
      update,
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
        parents: nodeIdToParents(options.parentPkg.nodeId, ctx.resolvedPackagesByPackageId),
        prefix: ctx.prefix,
        reason: 'resolution_failure',
      })
      return null
    }
    err.pkgsStack = nodeIdToParents(options.parentPkg.nodeId, ctx.resolvedPackagesByPackageId)
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

  pkgResponse.body.id = encodePkgId(pkgResponse.body.id)

  if (
    !pkgResponse.body.updated &&
    options.currentDepth === Math.max(0, options.updateDepth) &&
    depIsLinked && !ctx.force && !options.proceed
  ) {
    return null
  }

  if (pkgResponse.body.isLocal) {
    const manifest = pkgResponse.body.manifest || await pkgResponse.bundledManifest!() // eslint-disable-line @typescript-eslint/dot-notation
    return {
      alias: wantedDependency.alias || manifest.name,
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

  if (
    nodeIdContainsSequence(
      options.parentPkg.nodeId,
      options.parentPkg.pkgId,
      pkgResponse.body.id
    )
  ) {
    return null
  }

  let pkg: PackageManifest
  let useManifestInfoFromLockfile = false
  let prepare!: boolean
  let hasBin!: boolean
  pkg = ctx.readPackageHook
    ? ctx.readPackageHook(pkgResponse.body.manifest || await pkgResponse.bundledManifest!())
    : pkgResponse.body.manifest || await pkgResponse.bundledManifest!()

  if (
    !options.update && currentPkg.dependencyLockfile && currentPkg.depPath &&
    !pkgResponse.body.updated &&
    // peerDependencies field is also used for transitive peer dependencies which should not be linked
    // That's why we cannot omit reading package.json of such dependencies.
    // This can be removed if we implement something like peerDependenciesMeta.transitive: true
    !currentPkg.dependencyLockfile.peerDependencies
  ) {
    useManifestInfoFromLockfile = true
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
    hasBin = Boolean(pkg.bin && !R.isEmpty(pkg.bin) || pkg.directories?.bin)
    /* eslint-enable @typescript-eslint/dot-notation */
  }
  if (!pkg.name) { // TODO: don't fail on optional dependencies
    throw new PnpmError('MISSING_PACKAGE_NAME', `Can't install ${wantedDependency.pref}: Missing package name`)
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

  // using colon as it will never be used inside a package ID
  const nodeId = createNodeId(options.parentPkg.nodeId, pkgResponse.body.id)

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
  const installable = parentIsInstallable && currentIsInstallable !== false
  const isNew = !ctx.resolvedPackagesByPackageId[pkgResponse.body.id]

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

    ctx.resolvedPackagesByPackageId[pkgResponse.body.id] = getResolvedPackage({
      dependencyLockfile: currentPkg.dependencyLockfile,
      depPath: dp.relative(ctx.registries, pkg.name, pkgResponse.body.id),
      force: ctx.force,
      hasBin,
      pkg,
      pkgResponse,
      prepare,
      wantedDependency,
    })
  } else {
    ctx.resolvedPackagesByPackageId[pkgResponse.body.id].prod = ctx.resolvedPackagesByPackageId[pkgResponse.body.id].prod || !wantedDependency.dev && !wantedDependency.optional
    ctx.resolvedPackagesByPackageId[pkgResponse.body.id].dev = ctx.resolvedPackagesByPackageId[pkgResponse.body.id].dev || wantedDependency.dev
    ctx.resolvedPackagesByPackageId[pkgResponse.body.id].optional = ctx.resolvedPackagesByPackageId[pkgResponse.body.id].optional && wantedDependency.optional

    ctx.pendingNodes.push({
      alias: wantedDependency.alias || pkg.name,
      depth: options.currentDepth,
      installable,
      nodeId,
      resolvedPackage: ctx.resolvedPackagesByPackageId[pkgResponse.body.id],
    })
  }

  return {
    alias: wantedDependency.alias || pkg.name,
    depIsLinked,
    isNew,
    nodeId,
    normalizedPref: options.currentDepth === 0 ? pkgResponse.body.normalizedPref : undefined,
    pkgId: pkgResponse.body.id,

    // Next fields are actually only needed when isNew = true
    installable,
    isLinkedDependency: undefined,
    pkg,
    updated: pkgResponse.body.updated,
    useManifestInfoFromLockfile,
  }
}

function getResolvedPackage (
  options: {
    dependencyLockfile?: PackageSnapshot,
    depPath: string,
    force: boolean,
    hasBin: boolean,
    pkg: PackageManifest,
    pkgResponse: PackageResponse,
    prepare: boolean,
    wantedDependency: WantedDependency,
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
      peerDependencies,
      peerDependenciesMeta: options.pkg.peerDependenciesMeta,
    },
    depPath: options.depPath,
    dev: options.wantedDependency.dev,
    fetchingBundledManifest: options.pkgResponse.bundledManifest,
    fetchingFiles: options.pkgResponse.files!,
    filesIndexFile: options.pkgResponse.filesIndexFile!,
    finishing: options.pkgResponse.finishing!,
    hasBin: options.hasBin,
    hasBundledDependencies: !!(options.pkg.bundledDependencies || options.pkg.bundleDependencies),
    id: options.pkgResponse.body.id,
    name: options.pkg.name,
    optional: options.wantedDependency.optional,
    optionalDependencies: new Set(R.keys(options.pkg.optionalDependencies)),
    peerDependencies: peerDependencies ?? {},
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

function getWantedDependencies (
  pkg: PackageManifest,
  opts: {
    resolvedDependencies?: ResolvedDependencies,
    optionalDependencyNames?: string[],
    useManifestInfoFromLockfile: boolean,
  }
) {
  let deps = getNonDevWantedDependencies(pkg)
  if (!deps.length && opts.resolvedDependencies && opts.useManifestInfoFromLockfile) {
    const optionalDependencyNames = opts.optionalDependencyNames ?? []
    deps = Object.keys(opts.resolvedDependencies)
      .map((depName) => ({
        alias: depName,
        optional: optionalDependencyNames.includes(depName),
      } as WantedDependency))
  }
  return deps
}
