import path = require('path')
import logger from 'pnpm-logger'
import R = require('ramda')
import getNpmTarballUrl from 'get-npm-tarball-url'
import exists = require('path-exists')
import fetch, {FetchedPackage} from './fetch'
import {InstallContext, InstalledPackages} from '../api/install'
import {Dependencies} from '../types'
import memoize from '../memoize'
import {Package} from '../types'
import logStatus from '../logging/logInstallStatus'
import fs = require('mz/fs')
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
import getIsInstallable from './getIsInstallable'

export type InstalledPackage = {
  id: string,
  // optional dependencies are resolved for consistent shrinkwrap.yaml files
  // but installed only on machines that are supported by the package
  isInstallable: boolean,
  resolution: Resolution,
  pkg: Package,
  srcPath?: string,
  optional: boolean,
  dependencies: string[],
  fetchingFiles: Promise<Boolean>,
  path: string,
  specRaw: string,
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
    offline: boolean,
    isInstallable?: boolean,
    rawNpmConfig: Object,
    nodeModules: string,
    update: boolean,
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
    offline: boolean,
    isInstallable?: boolean,
    rawNpmConfig: Object,
    nodeModules: string,
    update: boolean,
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
    const noPrefixPkgShortId = pkgShortId.substr(1)
    const divideAt = noPrefixPkgShortId.lastIndexOf('/')
    return getNpmTarballUrl(
      noPrefixPkgShortId.substr(0, divideAt),
      noPrefixPkgShortId.substr(divideAt + 1),
      {registry})
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
    offline: boolean,
    isInstallable?: boolean,
    rawNpmConfig: Object,
    nodeModules: string,
    update: boolean,
  }
) {
  const keypath = options.keypath || []
  const proceed = keypath.length <= options.depth

  if (!proceed && options.pkgId && await exists(path.join(options.nodeModules, `.${options.pkgId}`))) {
    return null
  }

  const registry = normalizeRegistry(spec.scope && options.rawNpmConfig[`${spec.scope}:registry`] || options.registry)

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
    root: options.root,
    storePath: options.storePath,
    localRegistry: options.localRegistry,
    metaCache: options.metaCache,
    got: options.got,
    shrinkwrapResolution: options.shrinkwrapResolution,
    pkgId: options.pkgId,
    offline: options.offline,
  })

  const pkg = await fetchedPkg.fetchingPkg
  logStatus({status: 'downloaded_manifest', pkgId: fetchedPkg.id, pkgVersion: pkg.version})

  let dependencyIds: string[] | void
  const isInstallable = options.isInstallable !== false && (options.force || await getIsInstallable(fetchedPkg.id, pkg, fetchedPkg, options))

  if (!ctx.installed.has(fetchedPkg.id)) {
    ctx.installed.add(fetchedPkg.id)
    const dependencies = await installDependencies(
      pkg,
      fetchedPkg.id,
      ctx,
      Object.assign({}, options, {
        root: fetchedPkg.srcPath,
        isInstallable,
      })
    )
    const shortId = pkgShortId(fetchedPkg.id, ctx.shrinkwrap.registry)
    ctx.shrinkwrap.packages[shortId] = toShrDependency({
      shortId,
      resolution: fetchedPkg.resolution,
      dependencies,
      registry: ctx.shrinkwrap.registry,
      prevDependencies: ctx.shrinkwrap.packages[shortId] && ctx.shrinkwrap.packages[shortId]['dependencies'] || {},
    })
    dependencyIds = dependencies.filter(dep => dep.isInstallable).map(dep => dep.id)
  }

  if (isInstallable && ctx.installationSequence.indexOf(fetchedPkg.id) === -1) {
    ctx.installationSequence.push(fetchedPkg.id)
  }

  const dependency: InstalledPackage = {
    id: fetchedPkg.id,
    resolution: fetchedPkg.resolution,
    srcPath: fetchedPkg.srcPath,
    optional: options.optional === true,
    pkg,
    isInstallable,
    dependencies: dependencyIds || [],
    fetchingFiles: fetchedPkg.fetchingFiles,
    path: fetchedPkg.path,
    specRaw: spec.raw,
  }

  addInstalledPkg(ctx.installs, dependency)

  logStatus({status: 'dependencies_installed', pkgId: fetchedPkg.id})

  return dependency
}

function normalizeRegistry (registry: string) {
  if (registry.endsWith('/')) return registry
  return `${registry}/`
}

function toShrDependency (
  opts: {
    shortId: string,
    resolution: Resolution,
    dependencies: InstalledPackage[],
    registry: string,
    prevDependencies: ResolvedDependencies,
  }
): DependencyShrinkwrap {
  const shrResolution = toShrResolution(opts.shortId, opts.resolution)
  const newDeps = updateDependencies(opts.dependencies, opts.prevDependencies, opts.registry)
  if (!R.isEmpty(newDeps)) {
    return {
      resolution: shrResolution,
      dependencies: newDeps,
    }
  }
  if (typeof shrResolution === 'string') return shrResolution
  return {
    resolution: shrResolution
  }
}

function updateDependencies (
  deps: InstalledPackage[],
  prevDependencies: ResolvedDependencies,
  registry: string
) {
  if (R.isEmpty(prevDependencies)) {
    return R.fromPairs<string>(
      deps.map((newDep): R.KeyValuePair<string, string> => {
        return [newDep.pkg.name, pkgIdToRef(newDep.id, newDep.pkg.version, newDep.resolution, registry)]
      })
    )
  }
  return R.fromPairs<string>(
    R.keys(prevDependencies)
      .map((depName): R.KeyValuePair<string, string> => {
        const newDep = deps.find(dep => dep.pkg.name === depName)
        if (newDep) {
          return [depName, pkgIdToRef(newDep.id, newDep.pkg.version, newDep.resolution, registry)]
        }
        return [depName, prevDependencies[depName]]
      })
  )
}

function toShrResolution (shortId: string, resolution: Resolution): string | Resolution {
  if (shortId.startsWith('/') && resolution.type === undefined && resolution.shasum) {
    return resolution.shasum
  }
  return resolution
}

async function installDependencies (
  pkg: Package,
  pkgId: string,
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
    offline: boolean,
    isInstallable?: boolean,
    rawNpmConfig: Object,
    nodeModules: string,
    update: boolean,
  }
): Promise<InstalledPackage[]> {
  const depsInstallOpts = Object.assign({}, opts, {
    keypath: opts.keypath.concat([ pkgId ]),
  })

  const bundledDeps = pkg.bundleDependencies || pkg.bundledDependencies || []
  const filterDeps = getNotBundledDeps.bind(null, bundledDeps)
  const deps = depsToSpecs(filterDeps(Object.assign({}, pkg.optionalDependencies, pkg.dependencies)), opts.root)
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
  if (!newPkg.isInstallable) return
  if (!installs[newPkg.id]) {
    installs[newPkg.id] = newPkg
    return
  }
  installs[newPkg.id].optional = installs[newPkg.id].optional && newPkg.optional
  if (!installs[newPkg.id].dependencies.length) {
    installs[newPkg.id].dependencies = newPkg.dependencies
  }
}
