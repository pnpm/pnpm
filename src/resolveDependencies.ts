import logger from '@pnpm/logger'
import {
  PackageFilesResponse,
  PackageResponse,
} from '@pnpm/package-requester'
import {Resolution} from '@pnpm/resolver-base'
import {
  Dependencies,
  PackageManifest,
  ReadPackageHook,
} from '@pnpm/types'
import * as dp from 'dependency-path'
import fs = require('mz/fs')
import path = require('path')
import exists = require('path-exists')
import {
  DependencyShrinkwrap,
  nameVerFromPkgSnapshot,
  pkgSnapshotToResolution,
  ResolvedDependencies,
  Shrinkwrap,
} from 'pnpm-shrinkwrap'
import R = require('ramda')
import semver = require('semver')
import url = require('url')
import {InstallContext, PkgByPkgId} from './api/install'
import depsToSpecs from './depsToSpecs'
import encodePkgId from './encodePkgId'
import getIsInstallable from './install/getIsInstallable'
import {deprecationLogger} from './loggers'
import logStatus from './logging/logInstallStatus'
import memoize from './memoize'
import {
  createNodeId,
  nodeIdContainsSequence,
} from './nodeIdUtils'
import {
  WantedDependency,
} from './types'

const ENGINE_NAME = `${process.platform}-${process.arch}-node-${process.version.split('.')[0]}`

export interface PkgAddress {
  alias: string,
  nodeId: string,
  pkgId: string,
  normalizedPref?: string, // is returned only for root dependencies
}

export interface Pkg {
  id: string,
  resolution: Resolution,
  prod: boolean,
  dev: boolean,
  optional: boolean,
  fetchingFiles: Promise<PackageFilesResponse>,
  finishing: Promise<void>,
  path: string,
  specRaw: string,
  name: string,
  version: string,
  peerDependencies: Dependencies,
  optionalDependencies: Set<string>,
  hasBundledDependencies: boolean,
  requiresBuild: boolean,
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
  ctx: InstallContext,
  wantedDependencies: WantedDependency[],
  options: {
    keypath: string[],
    parentNodeId: string,
    currentDepth: number,
    resolvedDependencies?: ResolvedDependencies,
    // If the package has been updated, the dependencies
    // which were used by the previous version are passed
    // via this option
    preferedDependencies?: ResolvedDependencies,
    parentIsInstallable?: boolean,
    update: boolean,
    readPackageHook?: ReadPackageHook,
    hasManifestInShrinkwrap: boolean,
    sideEffectsCache: boolean,
    reinstallForFlatten?: boolean,
    shamefullyFlatten?: boolean,
  },
): Promise<PkgAddress[]> {
  const resolvedDependencies = options.resolvedDependencies || {}
  const preferedDependencies = options.preferedDependencies || {}
  const update = options.update && options.currentDepth <= ctx.depth
  const pkgAddresses = (
    await Promise.all(
      wantedDependencies
        .map(async (wantedDependency: WantedDependency) => {
          let reference = wantedDependency.alias && resolvedDependencies[wantedDependency.alias]
          let proceed = false

          // If dependencies that were used by the previous version of the package
          // satisfy the newer version's requirements, then pnpm tries to keep
          // the previous dependency.
          // So for example, if foo@1.0.0 had bar@1.0.0 as a dependency
          // and foo was updated to 1.1.0 which depends on bar ^1.0.0
          // then bar@1.0.0 can be reused for foo@1.1.0
          if (!reference && wantedDependency.alias && semver.validRange(wantedDependency.pref) !== null &&
            preferedDependencies[wantedDependency.alias] &&
            preferedSatisfiesWanted(preferedDependencies[wantedDependency.alias], wantedDependency as {alias: string, pref: string}, ctx.wantedShrinkwrap)) {
            proceed = true
            reference = preferedDependencies[wantedDependency.alias]
          }

          return await install(wantedDependency, ctx, {
            currentDepth: options.currentDepth,
            hasManifestInShrinkwrap: options.hasManifestInShrinkwrap,
            keypath: options.keypath,
            parentIsInstallable: options.parentIsInstallable,
            parentNodeId: options.parentNodeId,
            proceed,
            readPackageHook: options.readPackageHook,
            reinstallForFlatten: options.reinstallForFlatten,
            shamefullyFlatten: options.shamefullyFlatten,
            sideEffectsCache: options.sideEffectsCache,
            update,
            ...getInfoFromShrinkwrap(ctx.wantedShrinkwrap, reference, wantedDependency.alias, ctx.registry),
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
) {
  const relDepPath = dp.refToRelative(preferredRef, wantedDep.alias)
  const pkgSnapshot = shr.packages && shr.packages[relDepPath]
  if (!pkgSnapshot) {
    logger.warn(`Could not find prefered package ${relDepPath} in shrinkwrap`)
    return false
  }
  const nameVer = nameVerFromPkgSnapshot(relDepPath, pkgSnapshot)
  return semver.satisfies(nameVer.version, wantedDep.pref, true)
}

function getInfoFromShrinkwrap (
  shrinkwrap: Shrinkwrap,
  reference: string | undefined,
  pkgName: string | undefined,
  registry: string,
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
    const depPath = dp.resolve(shrinkwrap.registry, relDepPath)
    return {
      depPath,
      dependencyShrinkwrap,
      optionalDependencyNames: R.keys(dependencyShrinkwrap.optionalDependencies),
      pkgId: dependencyShrinkwrap.id || depPath,
      relDepPath,
      resolvedDependencies: {
        ...dependencyShrinkwrap.dependencies,
        ...dependencyShrinkwrap.optionalDependencies,
      },
      shrinkwrapResolution: pkgSnapshotToResolution(relDepPath, dependencyShrinkwrap, shrinkwrap.registry),
    }
  } else {
    return {
      pkgId: dp.resolve(shrinkwrap.registry, relDepPath),
      relDepPath,
    }
  }
}

async function install (
  wantedDependency: WantedDependency,
  ctx: InstallContext,
  options: {
    keypath: string[], // TODO: remove. Currently used only for logging
    pkgId?: string,
    depPath?: string,
    relDepPath?: string,
    parentNodeId: string,
    currentDepth: number,
    dependencyShrinkwrap?: DependencyShrinkwrap,
    shrinkwrapResolution?: Resolution,
    resolvedDependencies?: ResolvedDependencies,
    optionalDependencyNames?: string[],
    parentIsInstallable?: boolean,
    update: boolean,
    proceed: boolean,
    readPackageHook?: ReadPackageHook,
    hasManifestInShrinkwrap: boolean,
    sideEffectsCache: boolean,
    reinstallForFlatten?: boolean,
    shamefullyFlatten?: boolean,
  },
): Promise<PkgAddress | null> {
  const keypath = options.keypath || []
  const proceed = options.proceed || !options.shrinkwrapResolution || ctx.force || keypath.length <= ctx.depth
    || options.dependencyShrinkwrap && options.dependencyShrinkwrap.peerDependencies
  const parentIsInstallable = options.parentIsInstallable === undefined || options.parentIsInstallable

  if (!options.shamefullyFlatten && !options.reinstallForFlatten && !proceed && options.depPath &&
    // if package is not in `node_modules/.shrinkwrap.yaml`
    // we can safely assume that it doesn't exist in `node_modules`
    options.relDepPath && ctx.currentShrinkwrap.packages && ctx.currentShrinkwrap.packages[options.relDepPath] &&
    await exists(path.join(ctx.nodeModules, `.${options.depPath}`)) && (
      options.currentDepth > 0 || wantedDependency.alias && await exists(path.join(ctx.nodeModules, wantedDependency.alias))
    )) {

    return null
  }

  const scope = wantedDependency.alias && getScope(wantedDependency.alias)
  const registry = normalizeRegistry(scope && ctx.rawNpmConfig[`${scope}:registry`] || ctx.registry)

  const dependentId = keypath[keypath.length - 1]
  const loggedPkg = {
    dependentId,
    name: wantedDependency.alias,
    rawSpec: wantedDependency.raw,
  }
  logStatus({
    pkg: loggedPkg,
    status: 'installing',
  })

  let pkgResponse!: PackageResponse
  try {
    pkgResponse = await ctx.storeController.requestPackage(wantedDependency, {
      currentPkgId: options.pkgId,
      defaultTag: ctx.defaultTag,
      downloadPriority: -options.currentDepth,
      loggedPkg,
      preferredVersions: ctx.preferredVersions,
      prefix: ctx.prefix,
      registry,
      shrinkwrapResolution: options.shrinkwrapResolution,
      sideEffectsCache: options.sideEffectsCache,
      skipFetch: ctx.dryRun,
      update: options.update,
      verifyStoreIntegrity: ctx.verifyStoreInegrity,
    })
  } catch (err) {
    if (wantedDependency.optional) {
      logger.warn({
        err,
        message: `Skipping optional dependency ${wantedDependency.raw}. ${err.toString()}`,
      })
      return null
    }
    throw err
  }

  pkgResponse.body.id = encodePkgId(pkgResponse.body.id)

  if (pkgResponse.body.isLocal) {
    const manifest = pkgResponse.body.manifest || await pkgResponse['fetchingManifest'] // tslint:disable-line:no-string-literal
    if (options.currentDepth > 0) {
      logger.warn(`Ignoring file dependency because it is not a root dependency ${wantedDependency}`)
    } else {
      ctx.localPackages.push({
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
    logStatus({status: 'downloaded_manifest', pkgId: pkgResponse.body.id, pkgVersion: manifest.version})
    return null
  }

  // For the root dependency dependentId will be undefined,
  // that's why checking it
  if (dependentId && nodeIdContainsSequence(options.parentNodeId, dependentId, pkgResponse.body.id)) {
    return null
  }

  let pkg: PackageManifest
  let useManifestInfoFromShrinkwrap = false
  let requiresBuild!: boolean
  if (options.hasManifestInShrinkwrap && !options.update && options.dependencyShrinkwrap && options.relDepPath
    && !pkgResponse.body.updated) {
    useManifestInfoFromShrinkwrap = true
    requiresBuild = options.dependencyShrinkwrap.requiresBuild === true
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
        ? options.readPackageHook(pkgResponse.body['manifest'] || await pkgResponse['fetchingManifest'])
        : pkgResponse.body['manifest'] || await pkgResponse['fetchingManifest']

      // TODO: check the scripts field of the real package.json that is unpacked from the tarball
      requiresBuild = Boolean(pkg['scripts'] && (pkg['scripts']['preinstall'] || pkg['scripts']['install'] || pkg['scripts']['postinstall']))
    } catch (err) {
      // tslint:disable:no-empty
      // avoiding unhandled promise rejections
      if (pkgResponse['finishing']) pkgResponse['finishing'].catch((err: Error) => {})
      if (pkgResponse['fetchingFiles']) pkgResponse['fetchingFiles'].catch((err: Error) => {})
      // tslint:enable:no-empty
      throw err
    }
    // tslint:enable:no-string-literal
  }
  if (options.currentDepth === 0 && pkgResponse.body.latest && pkgResponse.body.latest !== pkg.version) {
    ctx.outdatedPkgs[pkgResponse.body.id] = pkgResponse.body.latest
  }
  if (pkg.deprecated) {
    deprecationLogger.warn({
      deprecated: pkg.deprecated,
      depth: options.currentDepth,
      pkgId: pkgResponse.body.id,
      pkgName: pkg.name,
      pkgVersion: pkg.version,
    })
  }

  logStatus({status: 'downloaded_manifest', pkgId: pkgResponse.body.id, pkgVersion: pkg.version})

  // using colon as it will never be used inside a package ID
  const nodeId = createNodeId(options.parentNodeId, pkgResponse.body.id)

  const currentIsInstallable = (
      ctx.force ||
      await getIsInstallable(pkgResponse.body.id, pkg, {
        engineStrict: ctx.engineStrict,
        nodeId,
        nodeVersion: ctx.nodeVersion,
        optional: wantedDependency.optional,
        pkgByPkgId: ctx.pkgByPkgId,
        pnpmVersion: ctx.pnpmVersion,
      })
    )
  const installable = parentIsInstallable && currentIsInstallable

  if (installable) {
    ctx.skipped.delete(pkgResponse.body.id)
  }
  if (!ctx.pkgByPkgId[pkgResponse.body.id]) {
    if (!installable) {
      // optional dependencies are resolved for consistent shrinkwrap.yaml files
      // but installed only on machines that are supported by the package
      ctx.skipped.add(pkgResponse.body.id)
    }

    const peerDependencies = peerDependenciesWithoutOwn(pkg)

    ctx.pkgByPkgId[pkgResponse.body.id] = {
      additionalInfo: {
        bundleDependencies: pkg.bundleDependencies,
        bundledDependencies: pkg.bundledDependencies,
        cpu: pkg.cpu,
        deprecated: pkg.deprecated,
        engines: pkg.engines,
        os: pkg.os,
        peerDependencies,
      },
      dev: wantedDependency.dev,
      engineCache: !ctx.force && pkgResponse.body.cacheByEngine && pkgResponse.body.cacheByEngine[ENGINE_NAME],
      fetchingFiles: pkgResponse['fetchingFiles'], // tslint:disable-line:no-string-literal
      finishing: pkgResponse['finishing'], // tslint:disable-line:no-string-literal
      hasBundledDependencies: !!(pkg.bundledDependencies || pkg.bundleDependencies),
      id: pkgResponse.body.id,
      name: pkg.name,
      optional: wantedDependency.optional,
      optionalDependencies: new Set(R.keys(pkg.optionalDependencies)),
      path: pkgResponse.body.inStoreLocation,
      peerDependencies: peerDependencies || {},
      prod: !wantedDependency.dev && !wantedDependency.optional,
      requiresBuild,
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
        parentIsInstallable: installable,
        parentNodeId: nodeId,
        preferedDependencies: pkgResponse.body.updated
          ? options.resolvedDependencies
          : undefined,
        readPackageHook: options.readPackageHook,
        reinstallForFlatten: options.reinstallForFlatten,
        resolvedDependencies: pkgResponse.body.updated
          ? undefined
          : options.resolvedDependencies,
        shamefullyFlatten: options.shamefullyFlatten,
        sideEffectsCache: options.sideEffectsCache,
        update: options.update,
        useManifestInfoFromShrinkwrap,
      },
    )
    ctx.childrenByParentId[pkgResponse.body.id] = children.map((child) => ({
      alias: child.alias,
      pkgId: child.pkgId,
    }))
    ctx.pkgGraph[nodeId] = {
      children: children.reduce((chn, child) => {
        chn[child.alias] = child.nodeId
        return chn
      }, {}),
      depth: options.currentDepth,
      installable,
      pkg: ctx.pkgByPkgId[pkgResponse.body.id],
    }
  } else {
    ctx.pkgByPkgId[pkgResponse.body.id].prod = ctx.pkgByPkgId[pkgResponse.body.id].prod || !wantedDependency.dev && !wantedDependency.optional
    ctx.pkgByPkgId[pkgResponse.body.id].dev = ctx.pkgByPkgId[pkgResponse.body.id].dev || wantedDependency.dev
    ctx.pkgByPkgId[pkgResponse.body.id].optional = ctx.pkgByPkgId[pkgResponse.body.id].optional && wantedDependency.optional

    ctx.nodesToBuild.push({
      alias: wantedDependency.alias || pkg.name,
      depth: options.currentDepth,
      installable,
      nodeId,
      pkg: ctx.pkgByPkgId[pkgResponse.body.id],
    })
  }
  // we need this for saving to package.json
  if (options.currentDepth === 0) {
    ctx.pkgByPkgId[pkgResponse.body.id].specRaw = wantedDependency.raw
  }

  logStatus({status: 'dependencies_installed', pkgId: pkgResponse.body.id})

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
  for (const peer of R.keys(pkg.peerDependencies)) {
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
  ctx: InstallContext,
  opts: {
    keypath: string[],
    parentNodeId: string,
    currentDepth: number,
    resolvedDependencies?: ResolvedDependencies,
    preferedDependencies?: ResolvedDependencies,
    optionalDependencyNames?: string[],
    parentIsInstallable: boolean,
    update: boolean,
    readPackageHook?: ReadPackageHook,
    hasManifestInShrinkwrap: boolean,
    useManifestInfoFromShrinkwrap: boolean,
    sideEffectsCache: boolean,
    reinstallForFlatten?: boolean,
    shamefullyFlatten?: boolean,
  },
): Promise<PkgAddress[]> {

  const bundledDeps = pkg.bundleDependencies || pkg.bundledDependencies || []
  const filterDeps = getNotBundledDeps.bind(null, bundledDeps)
  let deps = depsToSpecs(
    filterDeps({...pkg.optionalDependencies, ...pkg.dependencies}),
    {
      devDependencies: {},
      optionalDependencies: pkg.optionalDependencies || {},
    },
  )
  if (opts.hasManifestInShrinkwrap && !deps.length && opts.resolvedDependencies && opts.useManifestInfoFromShrinkwrap) {
    const optionalDependencyNames = opts.optionalDependencyNames || []
    deps = R.keys(opts.resolvedDependencies)
      .map((depName) => ({
        alias: depName,
        optional: optionalDependencyNames.indexOf(depName) !== -1,
      } as WantedDependency))
  }

  return await resolveDependencies(ctx, deps, opts)
}

function getNotBundledDeps (bundledDeps: string[], deps: Dependencies) {
  return Object.keys(deps)
    .filter((depName) => bundledDeps.indexOf(depName) === -1)
    .reduce((notBundledDeps, depName) => {
      notBundledDeps[depName] = deps[depName]
      return notBundledDeps
    }, {})
}
