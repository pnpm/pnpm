import path = require('path')
import logger from 'pnpm-logger'
import R = require('ramda')
import getNpmTarballUrl from 'get-npm-tarball-url'
import fetch, {FetchedPackage} from './fetch'
import {InstallContext, InstalledPackages} from '../api/install'
import {Dependencies} from '../types'
import memoize from '../memoize'
import {Package} from '../types'
import installChecks = require('pnpm-install-checks')
import pnpmPkg from '../pnpmPkgJson'
import logStatus from '../logging/logInstallStatus'
import rimraf = require('rimraf-then')
import fs = require('mz/fs')
import getRegistryUrl = require('registry-url')
import {Got} from '../network/got'
import {
  DependencyShrinkwrap,
  ResolvedDependencies,
  getPkgId,
  getPkgShortId,
  pkgIdToRef,
  pkgShortId,
} from '../fs/shrinkwrap'
import {Resolution, PackageSpec, PackageMeta} from '../resolve'
import depsToSpecs from '../depsToSpecs'

const installCheckLogger = logger('install-check')

export type InstalledPackage = {
  id: string,
  // optional dependencies are resolved for consistent shrinkwrap.yaml files
  // but installed only on machines that are supported by the package
  isInstallable: boolean,
  resolution: Resolution,
  pkg: Package,
  srcPath?: string,
  optional: boolean,
  hardlinkedLocation: string,
  modules: string,
  dependencies: string[],
  fetchingFiles: Promise<Boolean>,
  path: string,
}

export default async function installAll (
  ctx: InstallContext,
  specs: PackageSpec[],
  optionalDependencies: string[],
  options: {
    force: boolean,
    root: string,
    storePath: string,
    localRegistry: string,
    registry: string,
    metaCache: Map<string, PackageMeta>,
    got: Got,
    keypath?: string[],
    resolvedDependencies?: ResolvedDependencies,
    depth: number,
    engineStrict: boolean,
    nodeVersion: string,
    baseNodeModules: string,
    offline: boolean,
  }
): Promise<InstalledPackage[]> {
  const keypath = options.keypath || []

  const specGroups = R.partition((spec: PackageSpec) => !!spec.name && R.contains(spec.name, optionalDependencies), specs)
  const optionalDepSpecs = specGroups[0]
  const nonOptionalDepSpecs = specGroups[1]

  const installedPkgs: InstalledPackage[] = Array.prototype.concat.apply([], await Promise.all([
    installMultiple(ctx, nonOptionalDepSpecs, Object.assign({}, options, {optional: false, keypath})),
    installMultiple(ctx, optionalDepSpecs, Object.assign({}, options, {optional: true, keypath})),
  ]))

  return installedPkgs
}

async function installMultiple (
  ctx: InstallContext,
  specs: PackageSpec[],
  options: {
    force: boolean,
    root: string,
    storePath: string,
    localRegistry: string,
    registry: string,
    metaCache: Map<string, PackageMeta>,
    got: Got,
    keypath: string[],
    resolvedDependencies?: ResolvedDependencies,
    optional: boolean,
    depth: number,
    engineStrict: boolean,
    nodeVersion: string,
    baseNodeModules: string,
    offline: boolean,
  }
): Promise<InstalledPackage[]> {
  const installedPkgs: InstalledPackage[] = <InstalledPackage[]>(
    await Promise.all(
      specs
        .map(async (spec: PackageSpec) => {
          const reference = options.resolvedDependencies &&
            options.resolvedDependencies[spec.name]
          const pkgShortId = reference && getPkgShortId(reference, spec.name)
          const dependencyShrinkwrap = pkgShortId && ctx.shrinkwrap.packages[pkgShortId]
          const pkgId = reference && getPkgId(reference, spec.name, ctx.shrinkwrap.registry)
          return await install(spec, ctx, Object.assign({}, options, {
            pkgId,
            resolvedDependencies: dependencyShrinkwrap && dependencyShrinkwrap['dependencies'],
            shrinkwrapResolution: pkgShortId && dependencyShrinkwrap
              ? dependencyShrToResolution(pkgShortId, dependencyShrinkwrap, options.registry)
              : undefined,
          }))
        })
    )
  )
  .filter(pkg => pkg)

  return installedPkgs
}

function dependencyShrToResolution (
  pkgShortId: string,
  depShr: DependencyShrinkwrap,
  registry: string
): Resolution {
  if (typeof depShr === 'string') {
    return {
      shasum: depShr,
      tarball: getTarball()
    }
  }
  if (typeof depShr.resolution === 'string') {
    return {
      shasum: depShr.resolution,
      tarball: getTarball(),
    }
  }
  if (!depShr.resolution.type && !depShr.resolution.tarball) {
    return Object.assign({}, depShr.resolution, {
      tarball: getTarball()
    })
  }
  return depShr.resolution

  function getTarball () {
    const parts = pkgShortId.split('/')
    return getNpmTarballUrl(parts[1], parts[2], {registry})
  }
}

async function install (
  spec: PackageSpec,
  ctx: InstallContext,
  options: {
    force: boolean,
    root: string,
    storePath: string,
    localRegistry: string,
    registry: string,
    metaCache: Map<string, PackageMeta>,
    got: Got,
    keypath: string[],
    pkgId?: string,
    shrinkwrapResolution?: Resolution,
    resolvedDependencies: ResolvedDependencies,
    optional: boolean,
    depth: number,
    engineStrict: boolean,
    nodeVersion: string,
    baseNodeModules: string,
    offline: boolean,
  }
) {
  const keypath = options.keypath || []
  const update = keypath.length <= options.depth
  const registry = spec.scope && getRegistryUrl(spec.scope) || options.registry

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

  const fetchedPkg = await fetch(spec, Object.assign({}, options, {
    loggedPkg,
    update,
    fetchingLocker: ctx.fetchingLocker,
    registry,
  }))

  const pkg = await fetchedPkg.fetchingPkg

  const modules = path.join(options.baseNodeModules, `.${fetchedPkg.id}`, 'node_modules')

  const dependency: InstalledPackage = {
    id: fetchedPkg.id,
    resolution: fetchedPkg.resolution,
    srcPath: fetchedPkg.srcPath,
    optional: options.optional === true,
    pkg,
    hardlinkedLocation: path.join(modules, pkg.name),
    modules,
    isInstallable: options.force || await getIsInstallable(fetchedPkg.id, pkg, fetchedPkg, options),
    dependencies: [], // TODO: rewrite to avoid this
    fetchingFiles: fetchedPkg.fetchingFiles,
    path: fetchedPkg.path,
  }

  addInstalledPkg(ctx.installs, dependency)

  if (!ctx.installed.has(dependency.id)) {
    ctx.installed.add(dependency.id)
    const dependencies = await installDependencies(
      pkg,
      dependency,
      ctx,
      options
    )
    const shortId = pkgShortId(fetchedPkg.id, ctx.shrinkwrap.registry)
    ctx.shrinkwrap.packages[shortId] = toShrDependency(shortId, fetchedPkg.resolution, dependencies, ctx.shrinkwrap.registry)
    dependency.dependencies = dependencies.map(dep => dep.id)

    if (ctx.installationSequence.indexOf(dependency.id) === -1) {
      ctx.installationSequence.push(dependency.id)
    }
  }

  logStatus({
    status: 'installed',
    pkg: Object.assign({}, loggedPkg, {version: pkg.version}),
  })

  return dependency
}

function toShrDependency (
  shortId: string,
  resolution: Resolution,
  deps: InstalledPackage[],
  registry: string
): DependencyShrinkwrap {
  const shrResolution = toShrResolution(shortId, resolution)
  if (deps.length) {
    return {
      resolution: shrResolution,
      dependencies: deps
        .reduce((resolutions, dep) => Object.assign(resolutions, {
          [dep.pkg.name]: pkgIdToRef(dep.id, dep.pkg.version, dep.resolution, registry)
        }), {})
    }
  }
  if (typeof shrResolution === 'string') return shrResolution
  return {
    resolution: shrResolution
  }
}

function toShrResolution (shortId: string, resolution: Resolution): string | Resolution {
  if (shortId.startsWith('/') && resolution.type === undefined && resolution.shasum) {
    return resolution.shasum
  }
  return resolution
}

async function getIsInstallable (
  pkgId: string,
  pkg: Package,
  fetchedPkg: FetchedPackage,
  options: {
    optional: boolean,
    engineStrict: boolean,
    nodeVersion: string,
  }
): Promise<boolean> {
  const warn = await installChecks.checkPlatform(pkg) || await installChecks.checkEngine(pkg, {
    pnpmVersion: pnpmPkg.version,
    nodeVersion: options.nodeVersion
  })

  if (!warn) return true

  installCheckLogger.warn(warn)

  if (!options.engineStrict && !options.optional) return true

  await fetchedPkg.abort()

  if (!options.optional) throw warn

  logger.warn({
    message: `Skipping failed optional dependency ${pkgId}`,
    warn,
  })

  return false
}

async function installDependencies (
  pkg: Package,
  dependency: InstalledPackage,
  ctx: InstallContext,
  opts: {
    force: boolean,
    root: string,
    storePath: string,
    localRegistry: string,
    registry: string,
    metaCache: Map<string, PackageMeta>,
    got: Got,
    keypath: string[],
    resolvedDependencies?: ResolvedDependencies,
    optional: boolean,
    depth: number,
    engineStrict: boolean,
    nodeVersion: string,
    baseNodeModules: string,
    offline: boolean,
  }
): Promise<InstalledPackage[]> {
  const depsInstallOpts = Object.assign({}, opts, {
    keypath: opts.keypath.concat([ dependency.id ]),
    root: dependency.srcPath,
  })

  const bundledDeps = pkg.bundleDependencies || pkg.bundledDependencies || []
  const filterDeps = getNotBundledDeps.bind(null, bundledDeps)
  const deps = depsToSpecs(filterDeps(Object.assign({}, pkg.optionalDependencies, pkg.dependencies)))
  const optionalDeps = Object.keys(pkg.optionalDependencies || {})

  const installedDeps: InstalledPackage[] = await installAll(ctx, deps, optionalDeps, depsInstallOpts)

  return installedDeps
}

function getNotBundledDeps (bundledDeps: string[], deps: Dependencies) {
  return Object.keys(deps)
    .filter(depName => bundledDeps.indexOf(depName) === -1)
    .reduce((notBundledDeps, depName) => {
      notBundledDeps[depName] = deps[depName]
      return notBundledDeps
    }, {})
}

function addInstalledPkg (installs: InstalledPackages, newPkg: InstalledPackage) {
  if (!installs[newPkg.id]) {
    installs[newPkg.id] = newPkg
    return
  }
  installs[newPkg.id].optional = installs[newPkg.id].optional && newPkg.optional
}
