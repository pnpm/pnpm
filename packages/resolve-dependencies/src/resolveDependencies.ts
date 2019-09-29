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
import {
  DirectoryResolution,
  LocalPackages,
  Resolution,
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
import {
  createNodeId,
  getNonDevWantedDependencies,
  nodeIdContainsSequence,
  pickRegistryForPackage,
  splitNodeId,
  WantedDependency,
} from '@pnpm/utils'
import * as dp from 'dependency-path'
import path = require('path')
import exists = require('path-exists')
import R = require('ramda')
import semver = require('semver')
import encodePkgId from './encodePkgId'
import wantedDepIsLocallyAvailable from './wantedDepIsLocallyAvailable'

const dependencyResolvedLogger = logger('_dependency_resolved')

export function nodeIdToParents (
  nodeId: string,
  resolvedPackagesByPackageId: ResolvedPackagesByPackageId,
) {
  const pkgIds = splitNodeId(nodeId).slice(2, -2)
  return pkgIds
    .map((pkgId) => {
      const pkg = resolvedPackagesByPackageId[pkgId]
      return {
        id: pkg.id,
        name: pkg.name,
        version: pkg.version,
      }
    })
}

export interface DependenciesTreeNode {
  children: (() => {[alias: string]: string}) | {[alias: string]: string}, // child nodeId by child alias name
  resolvedPackage: ResolvedPackage,
  depth: number,
  installable: boolean,
}

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
  optional: boolean,
  dev: boolean,
  resolution: DirectoryResolution,
  id: string,
  version: string,
  name: string,
  specRaw: string,
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
  [parentId: string]: Array<{alias: string, pkgId: string}>,
}

export interface ResolutionContext {
  defaultTag: string,
  dryRun: boolean,
  resolvedPackagesByPackageId: ResolvedPackagesByPackageId,
  outdatedDependencies: {[pkgId: string]: string},
  linkedDependencies: LinkedDependency[],
  childrenByParentId: ChildrenByParentId,
  pendingNodes: PendingNode[],
  wantedLockfile: Lockfile,
  updateLockfile: boolean,
  currentLockfile: Lockfile,
  lockfileDirectory: string,
  sideEffectsCache: boolean,
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
  resolutionStrategy: 'fast' | 'fewer-dependencies',
}

type PreferredVersions = {
  [packageName: string]: {
    type: 'version' | 'range' | 'tag',
    selector: string,
  },
}

const ENGINE_NAME = `${process.platform}-${process.arch}-node-${process.version.split('.')[0]}`

export interface PkgAddress {
  alias: string,
  depIsLinked: boolean,
  isNew: boolean,
  nodeId: string,
  pkgId: string,
  normalizedPref?: string, // is returned only for root dependencies
  installable: boolean,
  pkg: PackageManifest,
  updated: boolean,
  useManifestInfoFromLockfile: boolean,
  specRaw: string,
}

export interface ResolvedPackage {
  id: string,
  resolution: Resolution,
  prod: boolean,
  dev: boolean,
  optional: boolean,
  fetchingFiles: () => Promise<PackageFilesResponse>,
  fetchingBundledManifest?: () => Promise<DependencyManifest>,
  finishing: () => Promise<void>,
  path: string,
  name: string,
  version: string,
  peerDependencies: Dependencies,
  optionalDependencies: Set<string>,
  hasBin: boolean,
  hasBundledDependencies: boolean,
  independent: boolean,
  prepare: boolean,
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
  engineCache?: string,
}

export default async function resolveDependencies (
  ctx: ResolutionContext,
  wantedDependencies: Array<WantedDependency & { updateDepth?: number }>,
  options: {
    dependentId?: string,
    parentDependsOnPeers: boolean,
    parentNodeId: string,
    proceed: boolean,
    updateDepth: number,
    currentDepth: number,
    resolvedDependencies?: ResolvedDependencies,
    // If the package has been updated, the dependencies
    // which were used by the previous version are passed
    // via this option
    preferedDependencies?: ResolvedDependencies,
    preferredVersions: PreferredVersions,
    parentIsInstallable?: boolean,
    readPackageHook?: ReadPackageHook,
    localPackages?: LocalPackages,
  },
): Promise<PkgAddress[]> {
  const extendedWantedDeps = getDepsToResolve(wantedDependencies, ctx.wantedLockfile, {
    parentDependsOnPeers: options.parentDependsOnPeers,
    preferedDependencies: options.preferedDependencies,
    prefix: ctx.prefix,
    proceed: options.proceed,
    registries: ctx.registries,
    resolvedDependencies: options.resolvedDependencies,
    updateLockfile: ctx.updateLockfile,
  })
  const resolveDepOpts = {
    currentDepth: options.currentDepth,
    dependentId: options.dependentId,
    localPackages: options.localPackages,
    parentDependsOnPeer: options.parentDependsOnPeers,
    parentIsInstallable: options.parentIsInstallable,
    parentNodeId: options.parentNodeId,
    preferredVersions: options.preferredVersions,
    readPackageHook: options.readPackageHook,
  }
  const postponedResolutionsQueue = ctx.resolutionStrategy === 'fewer-dependencies'
    ? [] as Array<(preferredVersions: PreferredVersions) => Promise<void>> : undefined
  const pkgAddresses = (
    await Promise.all(
      extendedWantedDeps
        .map(async (extendedWantedDep) => {
          const updateDepth = typeof extendedWantedDep.wantedDependency.updateDepth === 'number'
            ? extendedWantedDep.wantedDependency.updateDepth : options.updateDepth
          const resolveDependencyOpts: ResolveDependencyOptions = {
            ...resolveDepOpts,
            ...extendedWantedDep.infoFromLockfile,
            proceed: extendedWantedDep.proceed,
            update: options.currentDepth <= updateDepth,
            updateDepth,
          }
          const resolveDependencyResult = await resolveDependency(extendedWantedDep.wantedDependency, ctx, resolveDependencyOpts)

          if (!resolveDependencyResult) return null
          if (!resolveDependencyResult.isNew) return resolveDependencyResult

          const resolveChildren = async function (preferredVersions: PreferredVersions) {
            const resolvedPackage = ctx.resolvedPackagesByPackageId[resolveDependencyResult.pkgId]
            const resolvedDependencies = resolveDependencyResult.updated
              ? undefined
              : extendedWantedDep.infoFromLockfile && extendedWantedDep.infoFromLockfile.resolvedDependencies || undefined
            const optionalDependencyNames = extendedWantedDep.infoFromLockfile && extendedWantedDep.infoFromLockfile.optionalDependencyNames || undefined
            const children = await resolveDependencies(
              ctx,
              getWantedDependencies(resolveDependencyResult.pkg, {
                optionalDependencyNames,
                resolvedDependencies,
                useManifestInfoFromLockfile: resolveDependencyResult.useManifestInfoFromLockfile,
              }),
              {
                currentDepth: options.currentDepth + 1,
                dependentId: resolveDependencyResult.pkgId,
                parentDependsOnPeers: Boolean(
                  Object.keys(resolveDependencyOpts.dependencyLockfile && resolveDependencyOpts.dependencyLockfile.peerDependencies || resolveDependencyResult.pkg.peerDependencies || {}).length,
                ),
                parentIsInstallable: resolveDependencyResult.installable,
                parentNodeId: resolveDependencyResult.nodeId,
                preferedDependencies: resolveDependencyResult.updated
                  ? extendedWantedDep.infoFromLockfile && extendedWantedDep.infoFromLockfile.resolvedDependencies || undefined
                  : undefined,
                preferredVersions,
                // If the package is not linked, we should also gather information about its dependencies.
                // After linking the package we'll need to symlink its dependencies.
                proceed: !resolveDependencyResult.depIsLinked,
                resolvedDependencies,
                updateDepth,
              },
            )
            ctx.childrenByParentId[resolveDependencyResult.pkgId] = children.map((child) => ({
              alias: child.alias,
              pkgId: child.pkgId,
            }))
            ctx.dependenciesTree[resolveDependencyResult.nodeId] = {
              children: children.reduce((chn, child) => {
                chn[child.alias] = child.nodeId
                return chn
              }, {}),
              depth: options.currentDepth,
              installable: resolveDependencyResult.installable,
              resolvedPackage,
            }
          }

          if (postponedResolutionsQueue) {
            postponedResolutionsQueue.push(resolveChildren)
          }

          await resolveChildren(options.preferredVersions)

          return resolveDependencyResult
        }),
    )
  )
  .filter(Boolean) as PkgAddress[]

  if (postponedResolutionsQueue) {
    const newPreferredVersions = {
      ...options.preferredVersions,
    }
    for (const { pkgId } of pkgAddresses) {
      const resolvedPackage = ctx.resolvedPackagesByPackageId[pkgId]
      if (newPreferredVersions[resolvedPackage.name] && newPreferredVersions[resolvedPackage.name].type !== 'tag') {
        newPreferredVersions[resolvedPackage.name] = {
          selector: `${newPreferredVersions[resolvedPackage.name].selector} || ${resolvedPackage.version}`,
          type: 'range',
        }
      } else {
        newPreferredVersions[resolvedPackage.name] = {
          selector: resolvedPackage.version,
          type: 'version',
        }
      }
    }
    await Promise.all(postponedResolutionsQueue.map((postponedResolution) => postponedResolution(newPreferredVersions)))
  }

  return pkgAddresses
}

function getDepsToResolve (
  wantedDependencies: Array<WantedDependency & { updateDepth?: number }>,
  wantedLockfile: Lockfile,
  options: {
    parentDependsOnPeers: boolean,
    preferedDependencies?: ResolvedDependencies,
    prefix: string,
    proceed: boolean,
    registries: Registries,
    resolvedDependencies?: ResolvedDependencies,
    updateLockfile: boolean,
  },
) {
  const resolvedDependencies = options.resolvedDependencies || {}
  const preferedDependencies = options.preferedDependencies || {}
  const extendedWantedDeps = []
  // The only reason we resolve children in case the package depends on peers
  // is to get information about the existing dependencies, so that they can
  // be merged with the resolved peers.
  const proceedAll = options.proceed || options.parentDependsOnPeers || options.updateLockfile
  let allPeers = new Set<string>()
  for (const wantedDependency of wantedDependencies) {
    let reference = wantedDependency.alias && resolvedDependencies[wantedDependency.alias]
    let proceed = proceedAll

    // If dependencies that were used by the previous version of the package
    // satisfy the newer version's requirements, then pnpm tries to keep
    // the previous dependency.
    // So for example, if foo@1.0.0 had bar@1.0.0 as a dependency
    // and foo was updated to 1.1.0 which depends on bar ^1.0.0
    // then bar@1.0.0 can be reused for foo@1.1.0
    if (!reference && wantedDependency.alias && semver.validRange(wantedDependency.pref) !== null && // tslint:disable-line
      preferedDependencies[wantedDependency.alias] &&
      preferedSatisfiesWanted(
        preferedDependencies[wantedDependency.alias],
        wantedDependency as {alias: string, pref: string},
        wantedLockfile,
        {
          prefix: options.prefix,
        },
      )
    ) {
      proceed = true
      reference = preferedDependencies[wantedDependency.alias]
    }
    const infoFromLockfile = getInfoFromLockfile(wantedLockfile, options.registries, reference, wantedDependency.alias)
    if (
      !proceedAll &&
      infoFromLockfile &&
      infoFromLockfile.dependencyLockfile &&
      infoFromLockfile.dependencyLockfile.peerDependencies
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
  },
) {
  const relDepPath = dp.refToRelative(preferredRef, wantedDep.alias)
  if (relDepPath === null) return false
  const pkgSnapshot = lockfile.packages && lockfile.packages[relDepPath]
  if (!pkgSnapshot) {
    logger.warn({
      message: `Could not find preferred package ${relDepPath} in lockfile`,
      prefix: opts.prefix,
    })
    return false
  }
  const { version } = nameVerFromPkgSnapshot(relDepPath, pkgSnapshot)
  return semver.satisfies(version, wantedDep.pref, true)
}

function getInfoFromLockfile (
  lockfile: Lockfile,
  registries: Registries,
  reference: string | undefined,
  pkgName: string | undefined,
) {
  if (!reference || !pkgName) {
    return null
  }

  const relDepPath = dp.refToRelative(reference, pkgName)

  if (!relDepPath) {
    return null
  }

  const dependencyLockfile = lockfile.packages && lockfile.packages[relDepPath]

  if (dependencyLockfile) {
    if (dependencyLockfile.peerDependencies && dependencyLockfile.dependencies) {
      // This is done to guarantee that the dependency will be relinked with the
      // up-to-date peer dependencies
      // Covered by test: "peer dependency is grouped with dependency when peer is resolved not from a top dependency"
      R.keys(dependencyLockfile.peerDependencies).forEach((peer) => {
        delete dependencyLockfile.dependencies![peer]
      })
    }

    const depPath = dp.resolve(registries, relDepPath)
    return {
      currentResolution: pkgSnapshotToResolution(relDepPath, dependencyLockfile, registries),
      dependencyLockfile,
      depPath,
      optionalDependencyNames: R.keys(dependencyLockfile.optionalDependencies),
      pkgId: packageIdFromSnapshot(relDepPath, dependencyLockfile, registries),
      relDepPath,
      resolvedDependencies: {
        ...dependencyLockfile.dependencies,
        ...dependencyLockfile.optionalDependencies,
      },
    }
  } else {
    return {
      pkgId: dp.tryGetPackageId(registries, relDepPath) || relDepPath, // Does it make sense to set pkgId when we're not sure?
      relDepPath,
    }
  }
}

type ResolveDependencyOptions = {
  pkgId?: string,
  dependentId?: string,
  depPath?: string,
  relDepPath?: string,
  parentDependsOnPeer: boolean,
  parentNodeId: string,
  currentDepth: number,
  dependencyLockfile?: PackageSnapshot,
  currentResolution?: Resolution,
  parentIsInstallable?: boolean,
  preferredVersions: PreferredVersions,
  update: boolean,
  updateDepth: number,
  proceed: boolean,
  localPackages?: LocalPackages,
}

async function resolveDependency (
  wantedDependency: WantedDependency,
  ctx: ResolutionContext,
  options: ResolveDependencyOptions,
): Promise<PkgAddress | null> {
  const update = Boolean(
    options.update ||
    options.localPackages &&
    wantedDepIsLocallyAvailable(options.localPackages, wantedDependency, { defaultTag: ctx.defaultTag, registry: ctx.registries.default }))
  const proceed = update || options.proceed || !options.currentResolution
  const parentIsInstallable = options.parentIsInstallable === undefined || options.parentIsInstallable

  const currentLockfileContainsTheDep = options.relDepPath ? Boolean(ctx.currentLockfile.packages && ctx.currentLockfile.packages[options.relDepPath]) : undefined
  const depIsLinked = Boolean(options.depPath &&
    // if package is not in `node_modules/.pnpm-lock.yaml`
    // we can safely assume that it doesn't exist in `node_modules`
    currentLockfileContainsTheDep &&
    options.relDepPath && options.dependencyLockfile &&
    await exists(path.join(ctx.virtualStoreDir, `${options.depPath}/node_modules/${nameVerFromPkgSnapshot(options.relDepPath, options.dependencyLockfile).name}/package.json`)) &&
    (options.currentDepth > 0 || wantedDependency.alias && await exists(path.join(ctx.modulesDir, wantedDependency.alias))))

  if (!proceed && depIsLinked) {
    return null
  }

  let pkgResponse!: PackageResponse
  try {
    pkgResponse = await ctx.storeController.requestPackage(wantedDependency, {
      currentPackageId: options.pkgId,
      currentResolution: options.currentResolution,
      defaultTag: ctx.defaultTag,
      downloadPriority: -options.currentDepth,
      localPackages: options.localPackages,
      lockfileDirectory: ctx.lockfileDirectory,
      preferredVersions: options.preferredVersions,
      prefix: ctx.prefix,
      registry: wantedDependency.alias && pickRegistryForPackage(ctx.registries, wantedDependency.alias) || ctx.registries.default,
      sideEffectsCache: ctx.sideEffectsCache,
      // Unfortunately, even when run with --lockfile-only, we need the *real* package.json
      // so fetching of the tarball cannot be ever avoided. Related issue: https://github.com/pnpm/pnpm/issues/1176
      skipFetch: false,
      update,
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
        parents: nodeIdToParents(createNodeId(options.parentNodeId, 'fake-id'), ctx.resolvedPackagesByPackageId),
        prefix: ctx.prefix,
        reason: 'resolution_failure',
      })
      return null
    }
    throw err
  }

  dependencyResolvedLogger.debug({
    resolution: pkgResponse.body.id,
    wanted: {
      dependentId: options.dependentId,
      name: wantedDependency.alias,
      rawSpec: wantedDependency.raw,
    },
  })

  pkgResponse.body.id = encodePkgId(pkgResponse.body.id)

  if (
    !options.parentDependsOnPeer && !pkgResponse.body.updated &&
    options.currentDepth === options.updateDepth &&
    currentLockfileContainsTheDep && !ctx.force
  ) {
    return null
  }

  if (pkgResponse.body.isLocal) {
    const manifest = pkgResponse.body.manifest || await pkgResponse.bundledManifest!() // tslint:disable-line:no-string-literal
    if (options.currentDepth > 0) {
      logger.warn({
        message: `Ignoring file dependency because it is not a root dependency ${wantedDependency}`,
        prefix: ctx.prefix,
      })
    } else {
      ctx.linkedDependencies.push({
        alias: wantedDependency.alias || manifest.name,
        dev: wantedDependency.dev,
        id: pkgResponse.body.id,
        name: manifest.name,
        normalizedPref: pkgResponse.body.normalizedPref,
        optional: wantedDependency.optional,
        resolution: pkgResponse.body.resolution,
        specRaw: wantedDependency.raw,
        version: manifest.version,
      })
    }
    return null
  }

  // For the root dependency dependentId will be undefined,
  // that's why checking it
  if (options.dependentId && nodeIdContainsSequence(options.parentNodeId, options.dependentId, pkgResponse.body.id)) {
    return null
  }

  let pkg: PackageManifest
  let useManifestInfoFromLockfile = false
  let prepare!: boolean
  let hasBin!: boolean
  if (
    !options.update && options.dependencyLockfile && options.relDepPath &&
    !pkgResponse.body.updated &&
    // peerDependencies field is also used for transitive peer dependencies which should not be linked
    // That's why we cannot omit reading package.json of such dependencies.
    // This can be removed if we implement something like peerDependenciesMeta.transitive: true
    !options.dependencyLockfile.peerDependencies
  ) {
    useManifestInfoFromLockfile = true
    prepare = options.dependencyLockfile.prepare === true
    hasBin = options.dependencyLockfile.hasBin === true
    pkg = Object.assign(
      nameVerFromPkgSnapshot(options.relDepPath, options.dependencyLockfile),
      options.dependencyLockfile,
    )
  } else {
    // tslint:disable:no-string-literal
    pkg = ctx.readPackageHook
      ? ctx.readPackageHook(pkgResponse.body.manifest || await pkgResponse.bundledManifest!())
      : pkgResponse.body.manifest || await pkgResponse.bundledManifest!()

    prepare = Boolean(
      pkgResponse.body.resolvedVia === 'git-repository' &&
      pkg.scripts && typeof pkg.scripts.prepare === 'string',
    )

    if (
      options.dependencyLockfile && options.dependencyLockfile.deprecated &&
      !pkgResponse.body.updated && !pkg.deprecated
    ) {
      pkg.deprecated = options.dependencyLockfile.deprecated
    }
    hasBin = Boolean(pkg.bin && !R.isEmpty(pkg.bin) || pkg.directories && pkg.directories.bin)
    // tslint:enable:no-string-literal
  }
  if (!pkg.name) { // TODO: don't fail on optional dependencies
    throw new PnpmError('MISSING_PACKAGE_NAME', `Can't install ${wantedDependency.raw}: Missing package name`)
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
  const nodeId = createNodeId(options.parentNodeId, pkgResponse.body.id)

  const currentIsInstallable = (
      ctx.force ||
      packageIsInstallable(pkgResponse.body.id, pkg, {
        engineStrict: ctx.engineStrict,
        nodeVersion: ctx.nodeVersion,
        optional: wantedDependency.optional,
        pnpmVersion: ctx.pnpmVersion,
        prefix: ctx.prefix,
      })
    )
  if (currentIsInstallable !== true || !parentIsInstallable) {
    ctx.skipped.add(pkgResponse.body.id)
  }
  const installable = parentIsInstallable && currentIsInstallable !== false
  const isNew = !ctx.resolvedPackagesByPackageId[pkgResponse.body.id]

  if (isNew) {
    progressLogger.debug({
      packageId: pkgResponse.body.id,
      requester: ctx.lockfileDirectory,
      status: 'resolved',
    })
    if (pkgResponse.files) {
      pkgResponse.files()
        .then((fetchResult: PackageFilesResponse) => {
          progressLogger.debug({
            packageId: pkgResponse.body.id,
            requester: ctx.lockfileDirectory,
            status: fetchResult.fromStore
              ? 'found_in_store' : 'fetched',
          })
        })
        .catch(() => {
          // Ignore
        })
    }

    ctx.resolvedPackagesByPackageId[pkgResponse.body.id] = getResolvedPackage({
      dependencyLockfile: options.dependencyLockfile,
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
    specRaw: wantedDependency.raw, // we need this for saving to package.json

    // Next fields are actually only needed when isNew = true
    installable,
    pkg,
    updated: pkgResponse.body.updated,
    useManifestInfoFromLockfile,
  }
}

function getResolvedPackage (
  options: {
    dependencyLockfile?: PackageSnapshot,
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
    dev: options.wantedDependency.dev,
    engineCache: !options.force && options.pkgResponse.body.cacheByEngine && options.pkgResponse.body.cacheByEngine[ENGINE_NAME],
    fetchingBundledManifest: options.pkgResponse.bundledManifest,
    fetchingFiles: options.pkgResponse.files!,
    finishing: options.pkgResponse.finishing!,
    hasBin: options.hasBin,
    hasBundledDependencies: !!(options.pkg.bundledDependencies || options.pkg.bundleDependencies),
    id: options.pkgResponse.body.id,
    independent: (options.pkg.dependencies === undefined || R.isEmpty(options.pkg.dependencies)) &&
      (options.pkg.optionalDependencies === undefined || R.isEmpty(options.pkg.optionalDependencies)) &&
      (options.pkg.peerDependencies === undefined || R.isEmpty(options.pkg.peerDependencies)),
    name: options.pkg.name,
    optional: options.wantedDependency.optional,
    optionalDependencies: new Set(R.keys(options.pkg.optionalDependencies)),
    path: options.pkgResponse.body.inStoreLocation!,
    peerDependencies: peerDependencies || {},
    prepare: options.prepare,
    prod: !options.wantedDependency.dev && !options.wantedDependency.optional,
    requiresBuild: options.dependencyLockfile && Boolean(options.dependencyLockfile.requiresBuild),
    resolution: options.pkgResponse.body.resolution,
    version: options.pkg.version,
  }
}

function peerDependenciesWithoutOwn (pkg: PackageManifest) {
  if (!pkg.peerDependencies) return pkg.peerDependencies
  const ownDeps = new Set(
    R.keys(pkg.dependencies).concat(R.keys(pkg.optionalDependencies)),
  )
  const result = {}
  for (const peer of Object.keys(pkg.peerDependencies)) {
    if (ownDeps.has(peer)) continue
    result[peer] = pkg.peerDependencies[peer]
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
  },
) {
  let deps = getNonDevWantedDependencies(pkg)
  if (!deps.length && opts.resolvedDependencies && opts.useManifestInfoFromLockfile) {
    const optionalDependencyNames = opts.optionalDependencyNames || []
    deps = Object.keys(opts.resolvedDependencies)
      .map((depName) => ({
        alias: depName,
        optional: optionalDependencyNames.includes(depName),
      } as WantedDependency))
  }
  return deps
}
