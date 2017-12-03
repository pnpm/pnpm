import path = require('path')
import logger from '@pnpm/logger'
import {deprecationLogger} from '../loggers'
import R = require('ramda')
import getNpmTarballUrl from 'get-npm-tarball-url'
import exists = require('path-exists')
import url = require('url')
import {
  FetchedPackage,
  PackageContentInfo,
  Resolution,
} from '@pnpm/package-requester'
import {InstallContext, InstalledPackages} from '../api/install'
import {
  WantedDependency,
} from '../types'
import {
  ReadPackageHook,
  Dependencies,
  PackageManifest,
} from '@pnpm/types'
import memoize from '../memoize'
import logStatus from '../logging/logInstallStatus'
import fs = require('mz/fs')
import * as dp from 'dependency-path'
import {
  Shrinkwrap,
  DependencyShrinkwrap,
  ResolvedDependencies,
  getPkgShortId,
} from 'pnpm-shrinkwrap'
import depsToSpecs from '../depsToSpecs'
import getIsInstallable from './getIsInstallable'
import getPkgInfoFromShr from '../getPkgInfoFromShr'
import semver = require('semver')

export type PkgAddress = {
  alias: string,
  nodeId: string,
  pkgId: string,
  normalizedPref?: string, // is returned only for root dependencies
}

export type InstalledPackage = {
  id: string,
  resolution: Resolution,
  prod: boolean,
  dev: boolean,
  optional: boolean,
  fetchingFiles: Promise<PackageContentInfo>,
  calculatingIntegrity: Promise<void>,
  path: string,
  specRaw: string,
  name: string,
  version: string,
  peerDependencies: Dependencies,
  optionalDependencies: Set<string>,
  hasBundledDependencies: boolean,
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
}

export default async function installMultiple (
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
    ignoreFile?: (filename: string) => boolean,
  }
): Promise<PkgAddress[]> {
  const resolvedDependencies = options.resolvedDependencies || {}
  const preferedDependencies = options.preferedDependencies || {}
  const update = options.update && options.currentDepth <= ctx.depth
  const pkgAddresses = <PkgAddress[]>(
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
            refSatisfies(preferedDependencies[wantedDependency.alias], wantedDependency.pref)) {
            proceed = true
            reference = preferedDependencies[wantedDependency.alias]
          }

          return await install(wantedDependency, ctx, Object.assign({
              keypath: options.keypath,
              parentNodeId: options.parentNodeId,
              currentDepth: options.currentDepth,
              parentIsInstallable: options.parentIsInstallable,
              readPackageHook: options.readPackageHook,
              hasManifestInShrinkwrap: options.hasManifestInShrinkwrap,
              ignoreFile: options.ignoreFile,
              update,
              proceed,
            },
            getInfoFromShrinkwrap(ctx.wantedShrinkwrap, reference, wantedDependency.alias, ctx.registry)))
        })
    )
  )
  .filter(Boolean)

  return pkgAddresses
}

// A reference is not always a version.
// We assume that it does not satisfy the range if it's raw form is not a version
// This logic can be made smarter because
// if the reference is /foo/1.0.0/bar@2.0.0, foo's version if 1.0.0
function refSatisfies (reference: string, range: string) {
  try {
    return semver.satisfies(reference, range, true)
  } catch (err) {
    return false
  }
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

  const dependencyPath = dp.refToRelative(reference, pkgName)

  if (!dependencyPath) {
    return null
  }

  const dependencyShrinkwrap = shrinkwrap.packages && shrinkwrap.packages[dependencyPath]

  if (dependencyShrinkwrap) {
    const absoluteDependencyPath = dp.resolve(shrinkwrap.registry, dependencyPath)
    return {
      dependencyPath,
      absoluteDependencyPath,
      dependencyShrinkwrap,
      pkgId: dependencyShrinkwrap.id || absoluteDependencyPath,
      shrinkwrapResolution: dependencyShrToResolution(dependencyPath, dependencyShrinkwrap, shrinkwrap.registry),
      resolvedDependencies: {
        ...dependencyShrinkwrap.dependencies,
        ...dependencyShrinkwrap.optionalDependencies,
      },
      optionalDependencyNames: R.keys(dependencyShrinkwrap.optionalDependencies),
    }
  } else {
    return {
      dependencyPath,
      pkgId: dp.resolve(shrinkwrap.registry, dependencyPath),
    }
  }
}

function dependencyShrToResolution (
  dependencyPath: string,
  depShr: DependencyShrinkwrap,
  registry: string
): Resolution {
  if (depShr.resolution['type']) {
    return depShr.resolution as Resolution
  }
  if (!depShr.resolution['tarball']) {
    return Object.assign({}, depShr.resolution, {
      tarball: getTarball(),
      registry: depShr.resolution['registry'] || registry,
    })
  }
  if (depShr.resolution['tarball'].startsWith('file:')) {
    return depShr.resolution as Resolution
  }
  return Object.assign({}, depShr.resolution, {
    tarball: url.resolve(registry, depShr.resolution['tarball'])
  })

  function getTarball () {
    const parsed = dp.parse(dependencyPath)
    if (!parsed['name'] || !parsed['version']) {
      throw new Error(`Couldn't get tarball URL from dependency path ${dependencyPath}`)
    }
    return getNpmTarballUrl(parsed['name'], parsed['version'], {registry})
  }
}

async function install (
  wantedDependency: WantedDependency,
  ctx: InstallContext,
  options: {
    keypath: string[], // TODO: remove. Currently used only for logging
    pkgId?: string,
    absoluteDependencyPath?: string,
    dependencyPath?: string,
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
    ignoreFile?: (filename: string) => boolean,
  }
): Promise<PkgAddress | null> {
  const keypath = options.keypath || []
  const proceed = options.proceed || !options.shrinkwrapResolution || ctx.force || keypath.length <= ctx.depth
  const parentIsInstallable = options.parentIsInstallable === undefined || options.parentIsInstallable

  if (!proceed && options.absoluteDependencyPath &&
    // if package is not in `node_modules/.shrinkwrap.yaml`
    // we can safely assume that it doesn't exist in `node_modules`
    options.dependencyPath && ctx.currentShrinkwrap.packages && ctx.currentShrinkwrap.packages[options.dependencyPath] &&
    await exists(path.join(ctx.nodeModules, `.${options.absoluteDependencyPath}`)) && (
      options.currentDepth > 0 || wantedDependency.alias && await exists(path.join(ctx.nodeModules, wantedDependency.alias))
    )) {

    return null
  }

  const scope = wantedDependency.alias && getScope(wantedDependency.alias)
  const registry = normalizeRegistry(scope && ctx.rawNpmConfig[`${scope}:registry`] || ctx.registry)

  const dependentId = keypath[keypath.length - 1]
  const loggedPkg = {
    rawSpec: wantedDependency.raw,
    name: wantedDependency.alias,
    dependentId,
  }
  logStatus({
    status: 'installing',
    pkg: loggedPkg,
  })

  const fetchedPkg = await ctx.requestPackage(wantedDependency, {
    loggedPkg,
    update: options.update,
    fetchingLocker: ctx.fetchingLocker,
    registry,
    prefix: ctx.prefix,
    storePath: ctx.storePath,
    metaCache: ctx.metaCache,
    shrinkwrapResolution: options.shrinkwrapResolution,
    pkgId: options.pkgId,
    offline: ctx.offline,
    storeIndex: ctx.storeIndex,
    verifyStoreIntegrity: ctx.verifyStoreInegrity,
    downloadPriority: -options.currentDepth,
    ignore: options.ignoreFile,
  })

  if (fetchedPkg.isLocal) {
    const pkg = fetchedPkg.pkg
    if (options.currentDepth > 0) {
      logger.warn(`Ignoring file dependency because it is not a root dependency ${wantedDependency}`)
    } else {
      ctx.localPackages.push({
        alias: wantedDependency.alias || pkg.name,
        id: fetchedPkg.id,
        specRaw: wantedDependency.raw,
        name: pkg.name,
        version: pkg.version,
        dev: wantedDependency.dev,
        optional: wantedDependency.optional,
        resolution: fetchedPkg.resolution,
        normalizedPref: fetchedPkg.normalizedPref,
      })
    }
    logStatus({status: 'downloaded_manifest', pkgId: fetchedPkg.id, pkgVersion: pkg.version})
    return null
  }

  if (options.parentNodeId.indexOf(`:${dependentId}:${fetchedPkg.id}:`) !== -1) {
    return null
  }

  let pkg: PackageManifest
  let useManifestInfoFromShrinkwrap = false
  if (options.hasManifestInShrinkwrap && !options.update && options.dependencyShrinkwrap && options.dependencyPath) {
    useManifestInfoFromShrinkwrap = true
    pkg = Object.assign(
      getPkgInfoFromShr(options.dependencyPath, options.dependencyShrinkwrap),
      options.dependencyShrinkwrap
    )
    if (pkg.peerDependencies) {
      const deps = pkg.dependencies || {}
      R.keys(pkg.peerDependencies).forEach(peer => {
        delete deps[peer]
        if (options.resolvedDependencies) {
          delete options.resolvedDependencies[peer]
        }
      })
    }
  } else {
    try {
      pkg = options.readPackageHook
        ? options.readPackageHook(await fetchedPkg.fetchingPkg)
        : await fetchedPkg.fetchingPkg
    } catch (err) {
      // avoiding unhandled promise rejections
      fetchedPkg.calculatingIntegrity.catch((err: Error) => {})
      fetchedPkg.fetchingFiles.catch((err: Error) => {})
      throw err
    }
  }
  if (options.currentDepth === 0 && fetchedPkg.latest && fetchedPkg.latest !== pkg.version) {
    ctx.outdatedPkgs[fetchedPkg.id] = fetchedPkg.latest
  }
  if (pkg.deprecated) {
    deprecationLogger.warn({
      pkgName: pkg.name,
      pkgVersion: pkg.version,
      pkgId: fetchedPkg.id,
      deprecated: pkg.deprecated,
      depth: options.currentDepth,
    })
  }

  logStatus({status: 'downloaded_manifest', pkgId: fetchedPkg.id, pkgVersion: pkg.version})

  // using colon as it will never be used inside a package ID
  const nodeId = `${options.parentNodeId}${fetchedPkg.id}:`

  const currentIsInstallable = (
      ctx.force ||
      await getIsInstallable(fetchedPkg.id, pkg, fetchedPkg, {
        nodeId,
        installs: ctx.installs,
        optional: wantedDependency.optional,
        engineStrict: ctx.engineStrict,
        nodeVersion: ctx.nodeVersion,
        pnpmVersion: ctx.pnpmVersion,
      })
    )
  const installable = parentIsInstallable && currentIsInstallable

  if (installable) {
    ctx.skipped.delete(fetchedPkg.id)
  }
  if (!ctx.installs[fetchedPkg.id]) {
    if (!installable) {
      // optional dependencies are resolved for consistent shrinkwrap.yaml files
      // but installed only on machines that are supported by the package
      ctx.skipped.add(fetchedPkg.id)
    }

    const peerDependencies = peerDependenciesWithoutOwn(pkg)

    ctx.installs[fetchedPkg.id] = {
      id: fetchedPkg.id,
      resolution: fetchedPkg.resolution,
      optional: wantedDependency.optional,
      name: pkg.name,
      version: pkg.version,
      prod: !wantedDependency.dev && !wantedDependency.optional,
      dev: wantedDependency.dev,
      fetchingFiles: fetchedPkg.fetchingFiles,
      calculatingIntegrity: fetchedPkg.calculatingIntegrity,
      path: fetchedPkg.path,
      specRaw: wantedDependency.raw,
      peerDependencies: peerDependencies || {},
      optionalDependencies: new Set(R.keys(pkg.optionalDependencies)),
      hasBundledDependencies: !!(pkg.bundledDependencies || pkg.bundleDependencies),
      additionalInfo: {
        deprecated: pkg.deprecated,
        peerDependencies,
        bundleDependencies: pkg.bundleDependencies,
        bundledDependencies: pkg.bundledDependencies,
        engines: pkg.engines,
        cpu: pkg.cpu,
        os: pkg.os,
      }
    }
    const children = await installDependencies(
      pkg,
      ctx,
      {
        parentIsInstallable: installable,
        currentDepth: options.currentDepth + 1,
        parentNodeId: nodeId,
        keypath: options.keypath.concat([ fetchedPkg.id ]),
        resolvedDependencies: fetchedPkg.id !== options.pkgId
          ? undefined
          : options.resolvedDependencies,
        preferedDependencies: fetchedPkg.id !== options.pkgId
          ? options.resolvedDependencies
          : undefined,
        optionalDependencyNames: options.optionalDependencyNames,
        update: options.update,
        readPackageHook: options.readPackageHook,
        hasManifestInShrinkwrap: options.hasManifestInShrinkwrap,
        useManifestInfoFromShrinkwrap,
        ignoreFile: options.ignoreFile,
      }
    )
    ctx.childrenByParentId[fetchedPkg.id] = children.map(child => ({
      alias: child.alias,
      pkgId: child.pkgId,
    }))
    ctx.tree[nodeId] = {
      pkg: ctx.installs[fetchedPkg.id],
      children: children.reduce((children, child) => {
        children[child.alias] = child.nodeId
        return children
      }, {}),
      depth: options.currentDepth,
      installable,
    }
  } else {
    ctx.installs[fetchedPkg.id].prod = ctx.installs[fetchedPkg.id].prod || !wantedDependency.dev && !wantedDependency.optional
    ctx.installs[fetchedPkg.id].dev = ctx.installs[fetchedPkg.id].dev || wantedDependency.dev
    ctx.installs[fetchedPkg.id].optional = ctx.installs[fetchedPkg.id].optional && wantedDependency.optional

    ctx.nodesToBuild.push({
      alias: wantedDependency.alias || pkg.name,
      nodeId,
      pkg: ctx.installs[fetchedPkg.id],
      depth: options.currentDepth,
      installable,
    })
  }
  // we need this for saving to package.json
  if (options.currentDepth === 0) {
    ctx.installs[fetchedPkg.id].specRaw = wantedDependency.raw
  }

  logStatus({status: 'dependencies_installed', pkgId: fetchedPkg.id})

  return {
    alias: wantedDependency.alias || pkg.name,
    nodeId,
    pkgId: fetchedPkg.id,
    normalizedPref: options.currentDepth === 0 ? fetchedPkg.normalizedPref : undefined,
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
    R.keys(pkg.dependencies).concat(R.keys(pkg.optionalDependencies))
  )
  const result = {}
  for (let peer of R.keys(pkg.peerDependencies)) {
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

async function installDependencies (
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
    ignoreFile?: (filename: string) => boolean,
  }
): Promise<PkgAddress[]> {

  const bundledDeps = pkg.bundleDependencies || pkg.bundledDependencies || []
  const filterDeps = getNotBundledDeps.bind(null, bundledDeps)
  let deps = depsToSpecs(
    filterDeps({...pkg.optionalDependencies, ...pkg.dependencies}),
    {
      devDependencies: pkg.devDependencies || {},
      optionalDependencies: pkg.optionalDependencies || {},
    }
  )
  if (opts.hasManifestInShrinkwrap && !deps.length && opts.resolvedDependencies && opts.useManifestInfoFromShrinkwrap) {
    const optionalDependencyNames = opts.optionalDependencyNames || []
    deps = R.keys(opts.resolvedDependencies)
      .map(depName => (<WantedDependency>{
        alias: depName,
        optional: optionalDependencyNames.indexOf(depName) !== -1,
      }))
  }

  return await installMultiple(ctx, deps, opts)
}

function getNotBundledDeps (bundledDeps: string[], deps: Dependencies) {
  return Object.keys(deps)
    .filter(depName => bundledDeps.indexOf(depName) === -1)
    .reduce((notBundledDeps, depName) => {
      notBundledDeps[depName] = deps[depName]
      return notBundledDeps
    }, {})
}
