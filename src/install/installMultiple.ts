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
  dev: boolean,
  optional: boolean,
  dependencies: string[],
  fetchingFiles: Promise<Boolean>,
  path: string,
  specRaw: string,
}

export default async function installMultiple (
  ctx: InstallContext,
  specs: PackageSpec[],
  options: {
    force: boolean,
    prefix: string,
    referencedFrom: string,
    storePath: string,
    localRegistry: string,
    registry: string,
    metaCache: Map<string, PackageMeta>,
    got: Got,
    keypath: string[],
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
    prefix: string,
    referencedFrom: string,
    storePath: string,
    localRegistry: string,
    registry: string,
    metaCache: Map<string, PackageMeta>,
    got: Got,
    keypath: string[],
    pkgId?: string,
    shrinkwrapResolution?: Resolution,
    resolvedDependencies: ResolvedDependencies,
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
    prefix: options.prefix,
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
  const isInstallable = options.isInstallable !== false &&
    (
      options.force ||
      await getIsInstallable(fetchedPkg.id, pkg, fetchedPkg, {
        optional: spec.optional,
        engineStrict: options.engineStrict,
        nodeVersion: options.nodeVersion,
      })
    )

  if (!ctx.installed.has(fetchedPkg.id)) {
    ctx.installed.add(fetchedPkg.id)
    const dependencies = await installDependencies(
      pkg,
      spec,
      fetchedPkg.id,
      ctx,
      Object.assign({}, options, {
        referencedFrom: fetchedPkg.srcPath,
        isInstallable,
      })
    )
    const shortId = pkgShortId(fetchedPkg.id, ctx.shrinkwrap.registry)
    ctx.shrinkwrap.packages[shortId] = toShrDependency({
      shortId,
      resolution: fetchedPkg.resolution,
      updatedDeps: dependencies,
      registry: ctx.shrinkwrap.registry,
      prevResolvedDeps: ctx.shrinkwrap.packages[shortId] && ctx.shrinkwrap.packages[shortId]['dependencies'] || {},
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
    optional: spec.optional,
    dev: spec.dev,
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
    registry: string,
    updatedDeps: InstalledPackage[],
    prevResolvedDeps: ResolvedDependencies,
  }
): DependencyShrinkwrap {
  const shrResolution = toShrResolution(opts.shortId, opts.resolution)
  const newResolvedDeps = updateResolvedDeps(opts.prevResolvedDeps, opts.updatedDeps, opts.registry)
  if (!R.isEmpty(newResolvedDeps)) {
    return {
      resolution: shrResolution,
      dependencies: newResolvedDeps,
    }
  }
  if (typeof shrResolution === 'string') return shrResolution
  return {
    resolution: shrResolution
  }
}

// previous resolutions should not be removed from shrinkwrap
// as installation might not reanalize the whole dependency tree
// the `depth` property defines how deep should dependencies be checked
function updateResolvedDeps (
  prevResolvedDeps: ResolvedDependencies,
  updatedDeps: InstalledPackage[],
  registry: string
) {
  const newResolvedDeps = R.fromPairs<string>(
    updatedDeps.map((dep): R.KeyValuePair<string, string> => [
      dep.pkg.name,
      pkgIdToRef(dep.id, dep.pkg.version, dep.resolution, registry)
    ])
  )
  return R.merge(
    prevResolvedDeps,
    newResolvedDeps
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
  parentSpec: PackageSpec,
  pkgId: string,
  ctx: InstallContext,
  opts: {
    force: boolean,
    prefix: string,
    referencedFrom: string,
    storePath: string,
    localRegistry: string,
    registry: string,
    metaCache: Map<string, PackageMeta>,
    got: Got,
    keypath: string[],
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
  const depsInstallOpts = Object.assign({}, opts, {
    keypath: opts.keypath.concat([ pkgId ]),
  })

  const bundledDeps = pkg.bundleDependencies || pkg.bundledDependencies || []
  const filterDeps = getNotBundledDeps.bind(null, bundledDeps)
  const deps = depsToSpecs(
    filterDeps(Object.assign({}, pkg.optionalDependencies, pkg.dependencies)),
    {
      where: opts.referencedFrom,
      devDependencies: pkg.devDependencies || {},
      optionalDependencies: pkg.optionalDependencies || {},
    }
  )

  const installedDeps: InstalledPackage[] = await installMultiple(ctx, deps, depsInstallOpts)

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
  installs[newPkg.id].dev = installs[newPkg.id].dev && newPkg.dev
  installs[newPkg.id].optional = installs[newPkg.id].optional && newPkg.optional
  if (!installs[newPkg.id].dependencies.length) {
    installs[newPkg.id].dependencies = newPkg.dependencies
  }
}
