import path = require('path')
import logger, {deprecationLogger} from 'pnpm-logger'
import R = require('ramda')
import getNpmTarballUrl from 'get-npm-tarball-url'
import exists = require('path-exists')
import url = require('url')
import {
  Got,
  fetch,
  FetchedPackage,
  PackageContentInfo,
  Resolution,
  PackageSpec,
  PackageMeta,
} from 'package-store'
import {InstallContext, InstalledPackages} from '../api/install'
import {
  Dependencies,
  ReadPackageHook,
  Package,
} from '../types'
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
  nodeId: string,
  pkgId: string,
}

export type InstalledPackage = {
  id: string,
  resolution: Resolution,
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
  pkg: Package,
}

export default async function installMultiple (
  ctx: InstallContext,
  specs: PackageSpec[],
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
  }
): Promise<PkgAddress[]> {
  const resolvedDependencies = options.resolvedDependencies || {}
  const preferedDependencies = options.preferedDependencies || {}
  const update = options.update && options.currentDepth <= ctx.depth
  const pkgAddresses = <PkgAddress[]>(
    await Promise.all(
      specs
        .map(async (spec: PackageSpec) => {
          let reference = resolvedDependencies[spec.name]
          let proceed = false

          // If dependencies that were used by the previous version of the package
          // satisfy the newer version's requirements, then pnpm tries to keep
          // the previous dependency.
          // So for example, if foo@1.0.0 had bar@1.0.0 as a dependency
          // and foo was updated to 1.1.0 which depends on bar ^1.0.0
          // then bar@1.0.0 can be reused for foo@1.1.0
          if (!reference && spec.type === 'range' && preferedDependencies[spec.name] &&
            refSatisfies(preferedDependencies[spec.name], spec.fetchSpec)) {
            proceed = true
            reference = preferedDependencies[spec.name]
          }

          return await install(spec, ctx, Object.assign({
              keypath: options.keypath,
              parentNodeId: options.parentNodeId,
              currentDepth: options.currentDepth,
              parentIsInstallable: options.parentIsInstallable,
              readPackageHook: options.readPackageHook,
              update,
              proceed,
            },
            getInfoFromShrinkwrap(ctx.shrinkwrap, reference, spec.name, ctx.registry)))
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
  reference: string,
  pkgName: string,
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
      resolvedDependencies: <ResolvedDependencies>Object.assign({},
        dependencyShrinkwrap.dependencies, dependencyShrinkwrap.optionalDependencies),
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
  spec: PackageSpec,
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
  }
): Promise<PkgAddress | null> {
  const keypath = options.keypath || []
  const proceed = options.proceed || !options.shrinkwrapResolution || ctx.force || keypath.length <= ctx.depth
  const parentIsInstallable = options.parentIsInstallable === undefined || options.parentIsInstallable

  if (!proceed && options.absoluteDependencyPath &&
    // if package is not in `node_modules/.shrinkwrap.yaml`
    // we can safely assume that it doesn't exist in `node_modules`
    options.dependencyPath && ctx.privateShrinkwrap.packages && ctx.privateShrinkwrap.packages[options.dependencyPath] &&
    await exists(path.join(ctx.nodeModules, `.${options.absoluteDependencyPath}`)) && (
      options.currentDepth > 0 || await exists(path.join(ctx.nodeModules, spec.name))
    )) {

    return null
  }

  const registry = normalizeRegistry(spec.scope && ctx.rawNpmConfig[`${spec.scope}:registry`] || ctx.registry)

  const dependentId = keypath[keypath.length - 1]
  const loggedPkg = {
    rawSpec: spec.rawSpec,
    name: spec.name,
    dependentId,
  }
  logStatus({
    status: 'installing',
    pkg: loggedPkg,
  })

  const fetchedPkg = await fetch(spec, {
    loggedPkg,
    update: options.update,
    fetchingLocker: ctx.fetchingLocker,
    registry,
    prefix: ctx.prefix,
    storePath: ctx.storePath,
    metaCache: ctx.metaCache,
    got: ctx.got,
    shrinkwrapResolution: options.shrinkwrapResolution,
    pkgId: options.pkgId,
    offline: ctx.offline,
    storeIndex: ctx.storeIndex,
    verifyStoreIntegrity: ctx.verifyStoreInegrity,
    downloadPriority: -options.currentDepth,
  })

  if (fetchedPkg.isLocal) {
    const pkg = fetchedPkg.pkg
    if (options.currentDepth > 0) {
      logger.warn(`Ignoring file dependency because it is not a root dependency ${spec}`)
    } else {
      ctx.localPackages.push({
        id: fetchedPkg.id,
        specRaw: spec.raw,
        name: pkg.name,
        version: pkg.version,
        dev: spec.dev,
        optional: spec.optional,
        resolution: fetchedPkg.resolution,
      })
    }
    logStatus({status: 'downloaded_manifest', pkgId: fetchedPkg.id, pkgVersion: pkg.version})
    return null
  }

  if (options.parentNodeId.indexOf(`:${dependentId}:${fetchedPkg.id}:`) !== -1) {
    return null
  }

  let pkg: Package
  if (!options.update && options.dependencyShrinkwrap && options.dependencyPath) {
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
      fetchedPkg.calculatingIntegrity.catch(err => {})
      fetchedPkg.fetchingFiles.catch(err => {})
      throw err
    }
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

  const currentIsInstallable = (
      ctx.force ||
      await getIsInstallable(fetchedPkg.id, pkg, fetchedPkg, {
        optional: spec.optional,
        engineStrict: ctx.engineStrict,
        nodeVersion: ctx.nodeVersion,
        pnpmVersion: ctx.pnpmVersion,
      })
    )
  const installable = parentIsInstallable && currentIsInstallable

  // using colon as it will never be used inside a package ID
  const nodeId = `${options.parentNodeId}${fetchedPkg.id}:`

  if (installable) {
    ctx.skipped.delete(fetchedPkg.id)
  }
  if (!ctx.installs[fetchedPkg.id]) {
    if (!installable) {
      // optional dependencies are resolved for consistent shrinkwrap.yaml files
      // but installed only on machines that are supported by the package
      ctx.skipped.add(fetchedPkg.id)
    }

    ctx.installs[fetchedPkg.id] = {
      id: fetchedPkg.id,
      resolution: fetchedPkg.resolution,
      optional: spec.optional,
      name: pkg.name,
      version: pkg.version,
      dev: spec.dev,
      fetchingFiles: fetchedPkg.fetchingFiles,
      calculatingIntegrity: fetchedPkg.calculatingIntegrity,
      path: fetchedPkg.path,
      specRaw: spec.raw,
      peerDependencies: pkg.peerDependencies || {},
      optionalDependencies: new Set(R.keys(pkg.optionalDependencies)),
      hasBundledDependencies: !!(pkg.bundledDependencies || pkg.bundleDependencies),
      pkg,
    }
    const children = await installDependencies(
      pkg,
      spec,
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
      }
    )
    ctx.childrenIdsByParentId[fetchedPkg.id] = children.map(child => child.pkgId)
    ctx.tree[nodeId] = {
      nodeId,
      pkg: ctx.installs[fetchedPkg.id],
      children: children.map(child => child.nodeId),
      depth: options.currentDepth,
      installable,
    }
  } else {
    ctx.installs[fetchedPkg.id].dev = ctx.installs[fetchedPkg.id].dev && spec.dev
    ctx.installs[fetchedPkg.id].optional = ctx.installs[fetchedPkg.id].optional && spec.optional

    ctx.nodesToBuild.push({
      nodeId,
      pkg: ctx.installs[fetchedPkg.id],
      depth: options.currentDepth,
      installable,
    })
  }
  // we need this for saving to package.json
  if (options.currentDepth === 0) {
    ctx.installs[fetchedPkg.id].specRaw = spec.raw
  }

  logStatus({status: 'dependencies_installed', pkgId: fetchedPkg.id})

  return {
    nodeId,
    pkgId: fetchedPkg.id,
  }
}

function normalizeRegistry (registry: string) {
  if (registry.endsWith('/')) return registry
  return `${registry}/`
}

async function installDependencies (
  pkg: Package,
  parentSpec: PackageSpec,
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
  }
): Promise<PkgAddress[]> {

  const bundledDeps = pkg.bundleDependencies || pkg.bundledDependencies || []
  const filterDeps = getNotBundledDeps.bind(null, bundledDeps)
  let deps = depsToSpecs(
    filterDeps(Object.assign({}, pkg.optionalDependencies, pkg.dependencies)),
    {
      where: ctx.prefix,
      devDependencies: pkg.devDependencies || {},
      optionalDependencies: pkg.optionalDependencies || {},
    }
  )
  if (!deps.length && opts.resolvedDependencies) {
    const optionalDependencyNames = opts.optionalDependencyNames || []
    deps = R.keys(opts.resolvedDependencies)
      .map(depName => (<PackageSpec>{
        name: depName,
        scope: depName[0] === '@' ? depName.split('/')[0] : null,
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
