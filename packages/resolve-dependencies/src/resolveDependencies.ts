import {
  deprecationLogger,
  progressLogger,
  skippedOptionalDependencyLogger,
} from '@pnpm/core-loggers'
import logger from '@pnpm/logger'
import packageIsInstallable from '@pnpm/package-is-installable'
import {
  DirectoryResolution,
  LocalPackages,
  Resolution,
} from '@pnpm/resolver-base'
import {
  PackageSnapshot,
  ResolvedDependencies,
  Shrinkwrap,
} from '@pnpm/shrinkwrap-types'
import {
  nameVerFromPkgSnapshot,
  packageIdFromSnapshot,
  pkgSnapshotToResolution,
} from '@pnpm/shrinkwrap-utils'
import {
  PackageFilesResponse,
  PackageResponse,
  StoreController,
} from '@pnpm/store-controller-types'
import {
  Dependencies,
  PackageJson,
  PackageManifest,
  ReadPackageHook,
  Registries,
} from '@pnpm/types'
import {
  createNodeId,
  getNonDevWantedDependencies,
  nodeIdContainsSequence,
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
  wantedShrinkwrap: Shrinkwrap,
  currentShrinkwrap: Shrinkwrap,
  shrinkwrapDirectory: string,
  storeController: StoreController,
  // the IDs of packages that are not installable
  skipped: Set<string>,
  dependenciesTree: DependenciesTree,
  force: boolean,
  prefix: string,
  updateDepth: number,
  engineStrict: boolean,
  modulesDir: string,
  nodeVersion: string,
  pnpmVersion: string,
  registries: Registries,
  virtualStoreDir: string,
  verifyStoreIntegrity: boolean,
  preferredVersions: {
    [packageName: string]: {
      type: 'version' | 'range' | 'tag',
      selector: string,
    },
  },
}

const ENGINE_NAME = `${process.platform}-${process.arch}-node-${process.version.split('.')[0]}`

export interface PkgAddress {
  alias: string,
  nodeId: string,
  pkgId: string,
  normalizedPref?: string, // is returned only for root dependencies
}

export interface ResolvedPackage {
  id: string,
  resolution: Resolution,
  prod: boolean,
  dev: boolean,
  optional: boolean,
  fetchingFiles: Promise<PackageFilesResponse>,
  fetchingRawManifest?: Promise<PackageJson>,
  finishing: Promise<void>,
  path: string,
  specRaw: string,
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
  wantedDependencies: WantedDependency[],
  options: {
    keypath: string[],
    parentDependsOnPeers: boolean,
    parentNodeId: string,
    currentDepth: number,
    resolvedDependencies?: ResolvedDependencies,
    // If the package has been updated, the dependencies
    // which were used by the previous version are passed
    // via this option
    preferedDependencies?: ResolvedDependencies,
    parentIsInstallable?: boolean,
    readPackageHook?: ReadPackageHook,
    hasManifestInShrinkwrap: boolean,
    sideEffectsCache: boolean,
    shamefullyFlatten?: boolean,
    localPackages?: LocalPackages,
  },
): Promise<PkgAddress[]> {
  const resolvedDependencies = options.resolvedDependencies || {}
  const preferedDependencies = options.preferedDependencies || {}
  const update = options.currentDepth <= ctx.updateDepth
  const extendedWantedDeps = []
  let proceedAll = options.parentDependsOnPeers
  for (const wantedDependency of wantedDependencies) {
    let reference = wantedDependency.alias && resolvedDependencies[wantedDependency.alias]
    let proceed = options.parentDependsOnPeers

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
        ctx.wantedShrinkwrap,
        {
          prefix: ctx.prefix,
        },
      )
    ) {
      proceed = true
      reference = preferedDependencies[wantedDependency.alias]
    }
    const infoFromShrinkwrap = getInfoFromShrinkwrap(ctx.wantedShrinkwrap, ctx.registries, reference, wantedDependency.alias)
    if (
      infoFromShrinkwrap &&
      infoFromShrinkwrap.dependencyShrinkwrap &&
      infoFromShrinkwrap.dependencyShrinkwrap.peerDependencies &&
      Object.keys(infoFromShrinkwrap.dependencyShrinkwrap.peerDependencies).length
    ) {
      proceedAll = true
    }
    extendedWantedDeps.push({
      infoFromShrinkwrap,
      proceed,
      reference,
      wantedDependency,
    })
  }
  const resolveDepOpts = {
    currentDepth: options.currentDepth,
    hasManifestInShrinkwrap: options.hasManifestInShrinkwrap,
    keypath: options.keypath,
    localPackages: options.localPackages,
    parentDependsOnPeer: options.parentDependsOnPeers,
    parentIsInstallable: options.parentIsInstallable,
    parentNodeId: options.parentNodeId,
    readPackageHook: options.readPackageHook,
    shamefullyFlatten: options.shamefullyFlatten,
    sideEffectsCache: options.sideEffectsCache,
    update,
  }
  const pkgAddresses = (
    await Promise.all(
      extendedWantedDeps
        .map(async (extendedWantedDep) => {
          return resolveDependency(extendedWantedDep.wantedDependency, ctx, {
            ...resolveDepOpts,
            ...extendedWantedDep.infoFromShrinkwrap,
            proceed: extendedWantedDep.proceed || proceedAll,
          })
        }),
    )
  )
  .filter(Boolean) as PkgAddress[]

  return pkgAddresses
}

function preferedSatisfiesWanted (
  preferredRef: string,
  wantedDep: {alias: string, pref: string},
  shr: Shrinkwrap,
  opts: {
    prefix: string,
  },
) {
  const relDepPath = dp.refToRelative(preferredRef, wantedDep.alias)
  if (relDepPath === null) return false
  const pkgSnapshot = shr.packages && shr.packages[relDepPath]
  if (!pkgSnapshot) {
    logger.warn({
      message: `Could not find preferred package ${relDepPath} in shrinkwrap`,
      prefix: opts.prefix,
    })
    return false
  }
  const nameVer = nameVerFromPkgSnapshot(relDepPath, pkgSnapshot)
  return semver.satisfies(nameVer.version, wantedDep.pref, true)
}

function getInfoFromShrinkwrap (
  shrinkwrap: Shrinkwrap,
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

  const dependencyShrinkwrap = shrinkwrap.packages && shrinkwrap.packages[relDepPath]

  if (dependencyShrinkwrap) {
    const depPath = dp.resolve(registries, relDepPath)
    return {
      dependencyShrinkwrap,
      depPath,
      optionalDependencyNames: R.keys(dependencyShrinkwrap.optionalDependencies),
      pkgId: packageIdFromSnapshot(relDepPath, dependencyShrinkwrap, registries),
      relDepPath,
      resolvedDependencies: {
        ...dependencyShrinkwrap.dependencies,
        ...dependencyShrinkwrap.optionalDependencies,
      },
      shrinkwrapResolution: pkgSnapshotToResolution(relDepPath, dependencyShrinkwrap, registries),
    }
  } else {
    return {
      pkgId: dp.tryGetPackageId(registries, relDepPath) || relDepPath, // Does it make sense to set pkgId when we're not sure?
      relDepPath,
    }
  }
}

async function resolveDependency (
  wantedDependency: WantedDependency,
  ctx: ResolutionContext,
  options: {
    keypath: string[], // TODO: remove. Currently used only for logging
    pkgId?: string,
    depPath?: string,
    relDepPath?: string,
    parentDependsOnPeer: boolean,
    parentNodeId: string,
    currentDepth: number,
    dependencyShrinkwrap?: PackageSnapshot,
    shrinkwrapResolution?: Resolution,
    resolvedDependencies?: ResolvedDependencies,
    optionalDependencyNames?: string[],
    parentIsInstallable?: boolean,
    update: boolean,
    proceed: boolean,
    readPackageHook?: ReadPackageHook,
    hasManifestInShrinkwrap: boolean,
    sideEffectsCache: boolean,
    shamefullyFlatten?: boolean,
    localPackages?: LocalPackages,
  },
): Promise<PkgAddress | null> {
  const keypath = options.keypath || []
  const update = Boolean(
    options.update ||
    options.localPackages &&
    wantedDepIsLocallyAvailable(options.localPackages, wantedDependency, { defaultTag: ctx.defaultTag, registry: ctx.registries.default }))
  const proceed = update || options.proceed || !options.shrinkwrapResolution || ctx.force || keypath.length <= ctx.updateDepth
    || options.dependencyShrinkwrap && options.dependencyShrinkwrap.peerDependencies
  const parentIsInstallable = options.parentIsInstallable === undefined || options.parentIsInstallable

  if (!options.shamefullyFlatten && !proceed && options.depPath &&
    // if package is not in `node_modules/.shrinkwrap.yaml`
    // we can safely assume that it doesn't exist in `node_modules`
    options.relDepPath && ctx.currentShrinkwrap.packages && ctx.currentShrinkwrap.packages[options.relDepPath] &&
    await exists(path.join(ctx.virtualStoreDir, `.${options.depPath}`)) && (
      options.currentDepth > 0 || wantedDependency.alias && await exists(path.join(ctx.modulesDir, wantedDependency.alias))
    )) {

    return null
  }

  const scope = wantedDependency.alias && getScope(wantedDependency.alias)
  const registry = normalizeRegistry(scope && ctx.registries[scope] || ctx.registries.default)

  const dependentId = keypath[keypath.length - 1]
  const loggedPkg = {
    dependentId,
    name: wantedDependency.alias,
    rawSpec: wantedDependency.raw,
  }

  let pkgResponse!: PackageResponse
  try {
    pkgResponse = await ctx.storeController.requestPackage(wantedDependency, {
      currentPkgId: options.pkgId,
      defaultTag: ctx.defaultTag,
      downloadPriority: -options.currentDepth,
      localPackages: options.localPackages,
      loggedPkg,
      preferredVersions: ctx.preferredVersions,
      prefix: ctx.prefix,
      registry,
      shrinkwrapDirectory: ctx.shrinkwrapDirectory,
      shrinkwrapResolution: options.shrinkwrapResolution,
      sideEffectsCache: options.sideEffectsCache,
      // Unfortunately, even when run with --shrinkwrap-only, we need the *real* package.json
      // so fetching of the tarball cannot be ever avoided. Related issue: https://github.com/pnpm/pnpm/issues/1176
      skipFetch: false,
      update,
      verifyStoreIntegrity: ctx.verifyStoreIntegrity,
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

  pkgResponse.body.id = encodePkgId(pkgResponse.body.id)

  if (!options.parentDependsOnPeer && !pkgResponse.body.updated && options.update && options.currentDepth >= ctx.updateDepth && options.relDepPath &&
    ctx.currentShrinkwrap.packages && ctx.currentShrinkwrap.packages[options.relDepPath] && !ctx.force) {
    return null
  }

  if (pkgResponse.body.isLocal) {
    const manifest = pkgResponse.body.manifest || await pkgResponse['fetchingRawManifest'] // tslint:disable-line:no-string-literal
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
  if (dependentId && nodeIdContainsSequence(options.parentNodeId, dependentId, pkgResponse.body.id)) {
    return null
  }

  let pkg: PackageManifest
  let useManifestInfoFromShrinkwrap = false
  let prepare!: boolean
  let hasBin!: boolean
  if (options.hasManifestInShrinkwrap && !options.update && options.dependencyShrinkwrap && options.relDepPath
    && !pkgResponse.body.updated) {
    useManifestInfoFromShrinkwrap = true
    prepare = options.dependencyShrinkwrap.prepare === true
    hasBin = options.dependencyShrinkwrap.hasBin === true
    pkg = Object.assign(
      nameVerFromPkgSnapshot(options.relDepPath, options.dependencyShrinkwrap),
      options.dependencyShrinkwrap,
    )
    if (pkg.peerDependencies) {
      const deps = pkg.dependencies || {}
      R.keys(pkg.peerDependencies).forEach((peer) => {
        delete deps[peer]
        if (options.resolvedDependencies) {
          delete options.resolvedDependencies[peer]
        }
      })
    }
  } else {
    // tslint:disable:no-string-literal
    try {
      pkg = options.readPackageHook
        ? options.readPackageHook(pkgResponse.body['manifest'] || await pkgResponse['fetchingRawManifest'])
        : pkgResponse.body['manifest'] || await pkgResponse['fetchingRawManifest']

      prepare = Boolean(pkgResponse.body['resolvedVia'] === 'git-repository' && pkg['scripts'] && typeof pkg['scripts']['prepare'] === 'string')
      if (options.dependencyShrinkwrap && options.dependencyShrinkwrap.deprecated && !pkgResponse.body.updated && !pkg.deprecated) {
        pkg.deprecated = options.dependencyShrinkwrap.deprecated
      }
      hasBin = Boolean(pkg.bin && !R.isEmpty(pkg.bin) || pkg.directories && pkg.directories.bin)
    } catch (err) {
      // tslint:disable:no-empty
      // avoiding unhandled promise rejections
      if (pkgResponse['finishing']) pkgResponse['finishing'].catch(() => {})
      if (pkgResponse['fetchingFiles']) pkgResponse['fetchingFiles'].catch(() => {})
      // tslint:enable:no-empty
      throw err
    }
    // tslint:enable:no-string-literal
  }
  if (!pkg.name) { // TODO: don't fail on optional dependencies
    const err = new Error(`Can't install ${wantedDependency.raw}: Missing package name`)
    // tslint:disable:no-string-literal
    err['code'] = 'ERR_PNPM_MISSING_PACKAGE_NAME'
    // tslint:enable:no-string-literal
    throw err
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

  if (!ctx.resolvedPackagesByPackageId[pkgResponse.body.id]) {
    progressLogger.debug({
      packageId: pkgResponse.body.id,
      requester: ctx.shrinkwrapDirectory,
      status: 'resolved',
    })
    // tslint:disable:no-string-literal
    if (pkgResponse['fetchingFiles']) {
      pkgResponse['fetchingFiles']
        .then((fetchResult: PackageFilesResponse) => {
          progressLogger.debug({
            packageId: pkgResponse.body.id,
            requester: ctx.shrinkwrapDirectory,
            status: fetchResult.fromStore
              ? 'found_in_store' : 'fetched',
          })
        })
    }
    // tslint:enable:no-string-literal

    const peerDependencies = peerDependenciesWithoutOwn(pkg)

    ctx.resolvedPackagesByPackageId[pkgResponse.body.id] = {
      additionalInfo: {
        bundledDependencies: pkg.bundledDependencies,
        bundleDependencies: pkg.bundleDependencies,
        cpu: pkg.cpu,
        deprecated: pkg.deprecated,
        engines: pkg.engines,
        os: pkg.os,
        peerDependencies,
      },
      dev: wantedDependency.dev,
      engineCache: !ctx.force && pkgResponse.body.cacheByEngine && pkgResponse.body.cacheByEngine[ENGINE_NAME],
      fetchingFiles: pkgResponse['fetchingFiles'], // tslint:disable-line:no-string-literal
      fetchingRawManifest: pkgResponse['fetchingRawManifest'], // tslint:disable-line:no-string-literal
      finishing: pkgResponse['finishing'], // tslint:disable-line:no-string-literal
      hasBin,
      hasBundledDependencies: !!(pkg.bundledDependencies || pkg.bundleDependencies),
      id: pkgResponse.body.id,
      independent: (pkg.dependencies === undefined || R.isEmpty(pkg.dependencies)) &&
        (pkg.optionalDependencies === undefined || R.isEmpty(pkg.optionalDependencies)) &&
        (pkg.peerDependencies === undefined || R.isEmpty(pkg.peerDependencies)),
      name: pkg.name,
      optional: wantedDependency.optional,
      optionalDependencies: new Set(R.keys(pkg.optionalDependencies)),
      path: pkgResponse.body.inStoreLocation,
      peerDependencies: peerDependencies || {},
      prepare,
      prod: !wantedDependency.dev && !wantedDependency.optional,
      requiresBuild: options.dependencyShrinkwrap && Boolean(options.dependencyShrinkwrap.requiresBuild),
      resolution: pkgResponse.body.resolution,
      specRaw: wantedDependency.raw,
      version: pkg.version,
    }
    const children = await resolveDependenciesOfPackage(
      pkg,
      ctx,
      {
        currentDepth: options.currentDepth + 1,
        hasManifestInShrinkwrap: options.hasManifestInShrinkwrap,
        keypath: options.keypath.concat([ pkgResponse.body.id ]),
        optionalDependencyNames: options.optionalDependencyNames,
        parentDependsOnPeers: Boolean(
          Object.keys(options.dependencyShrinkwrap && options.dependencyShrinkwrap.peerDependencies || pkg.peerDependencies || {}).length,
        ),
        parentIsInstallable: installable,
        parentNodeId: nodeId,
        preferedDependencies: pkgResponse.body.updated
          ? options.resolvedDependencies
          : undefined,
        readPackageHook: options.readPackageHook,
        resolvedDependencies: pkgResponse.body.updated
          ? undefined
          : options.resolvedDependencies,
        shamefullyFlatten: options.shamefullyFlatten,
        sideEffectsCache: options.sideEffectsCache,
        useManifestInfoFromShrinkwrap,
      },
    )
    ctx.childrenByParentId[pkgResponse.body.id] = children.map((child) => ({
      alias: child.alias,
      pkgId: child.pkgId,
    }))
    ctx.dependenciesTree[nodeId] = {
      children: children.reduce((chn, child) => {
        chn[child.alias] = child.nodeId
        return chn
      }, {}),
      depth: options.currentDepth,
      installable,
      resolvedPackage: ctx.resolvedPackagesByPackageId[pkgResponse.body.id],
    }
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
  // we need this for saving to package.json
  if (options.currentDepth === 0) {
    ctx.resolvedPackagesByPackageId[pkgResponse.body.id].specRaw = wantedDependency.raw
  }

  return {
    alias: wantedDependency.alias || pkg.name,
    nodeId,
    normalizedPref: options.currentDepth === 0 ? pkgResponse.body.normalizedPref : undefined,
    pkgId: pkgResponse.body.id,
  }
}

function getScope (pkgName: string): string | null {
  if (pkgName[0] === '@') {
    return pkgName.substr(0, pkgName.indexOf('/'))
  }
  return null
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

function normalizeRegistry (registry: string) {
  if (registry.endsWith('/')) return registry
  return `${registry}/`
}

async function resolveDependenciesOfPackage (
  pkg: PackageManifest,
  ctx: ResolutionContext,
  opts: {
    keypath: string[],
    parentNodeId: string,
    currentDepth: number,
    resolvedDependencies?: ResolvedDependencies,
    preferedDependencies?: ResolvedDependencies,
    optionalDependencyNames?: string[],
    parentDependsOnPeers: boolean,
    parentIsInstallable: boolean,
    readPackageHook?: ReadPackageHook,
    hasManifestInShrinkwrap: boolean,
    useManifestInfoFromShrinkwrap: boolean,
    sideEffectsCache: boolean,
    shamefullyFlatten?: boolean,
  },
): Promise<PkgAddress[]> {

  let deps = getNonDevWantedDependencies(pkg)
  if (opts.hasManifestInShrinkwrap && !deps.length && opts.resolvedDependencies && opts.useManifestInfoFromShrinkwrap) {
    const optionalDependencyNames = opts.optionalDependencyNames || []
    deps = Object.keys(opts.resolvedDependencies)
      .map((depName) => ({
        alias: depName,
        optional: optionalDependencyNames.indexOf(depName) !== -1,
      } as WantedDependency))
  }

  return resolveDependencies(ctx, deps, opts)
}
