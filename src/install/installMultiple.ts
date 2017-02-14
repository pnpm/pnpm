import path = require('path')
import npa = require('npm-package-arg')
import logger from 'pnpm-logger'
import fetch, {FetchedPackage} from './fetch'
import {InstallContext, InstalledPackages} from '../api/install'
import {Dependencies} from '../types'
import memoize from '../memoize'
import {Package} from '../types'
import hardlinkDir from '../fs/hardlinkDir'
import mkdirp from '../fs/mkdirp'
import installChecks = require('pnpm-install-checks')
import pnpmPkg from '../pnpmPkgJson'
import symlinkDir from 'symlink-dir'
import exists = require('exists-file')
import {Graph} from '../fs/graphController'
import logStatus from '../logging/logInstallStatus'
import rimraf = require('rimraf-then')
import fs = require('mz/fs')
import {PackageMeta} from '../resolve/utils/loadPackageMeta'
import {Got} from '../network/got'
import {
  DependencyShrinkwrap,
  ResolvedDependencies,
} from '../fs/shrinkwrap'
import {PackageSpec} from '../resolve'

const installCheckLogger = logger('install-check')

export type InstalledPackage = FetchedPackage & {
  pkg: Package,
  keypath: string[],
  optional: boolean,
  dependencies: InstalledPackage[], // is needed to support flat tree
  hardlinkedLocation: string,
  modules: string,
}

export default async function installAll (
  ctx: InstallContext,
  dependencies: Dependencies,
  optionalDependencies: Dependencies,
  modules: string,
  options: {
    linkLocal: boolean,
    force: boolean,
    root: string,
    storePath: string,
    metaCache: Map<string, PackageMeta>,
    tag: string,
    got: Got,
    keypath?: string[],
    resolvedDependencies?: ResolvedDependencies,
    dependent: string,
    depth: number,
    engineStrict: boolean,
    nodeVersion: string,
    baseNodeModules: string,
    fetchingFiles?: Promise<Boolean>,
  }
): Promise<InstalledPackage[]> {
  const keypath = options.keypath || []

  const nonOptionalDependencies = Object.keys(dependencies)
    .filter(depName => !optionalDependencies[depName])
    .reduce((nonOptionalDependencies, depName) => {
      nonOptionalDependencies[depName] = dependencies[depName]
      return nonOptionalDependencies
    }, {})

  const installedPkgs: InstalledPackage[] = Array.prototype.concat.apply([], await Promise.all([
    installMultiple(ctx, nonOptionalDependencies, Object.assign({}, options, {optional: false, keypath})),
    installMultiple(ctx, optionalDependencies, Object.assign({}, options, {optional: true, keypath})),
  ]))

  if (options.fetchingFiles) {
    await options.fetchingFiles
  }

  await mkdirp(modules)
  await Promise.all(
    installedPkgs
      .map(async function (subdep) {
        const dest = path.join(modules, subdep.pkg.name)
        await symlinkDir(subdep.hardlinkedLocation, dest)
      })
  )

  return installedPkgs
}

async function installMultiple (
  ctx: InstallContext,
  pkgsMap: Dependencies,
  options: {
    linkLocal: boolean,
    force: boolean,
    root: string,
    storePath: string,
    metaCache: Map<string, PackageMeta>,
    tag: string,
    got: Got,
    keypath: string[],
    resolvedDependencies?: ResolvedDependencies,
    optional: boolean,
    dependent: string,
    depth: number,
    engineStrict: boolean,
    nodeVersion: string,
    baseNodeModules: string,
    fetchingFiles?: Promise<Boolean>,
  }
): Promise<InstalledPackage[]> {
  pkgsMap = pkgsMap || {}

  const pkgs = Object.keys(pkgsMap).map(pkgName => getRawSpec(pkgName, pkgsMap[pkgName]))

  ctx.graph = ctx.graph || {}

  const installedPkgs: InstalledPackage[] = <InstalledPackage[]>(
    await Promise.all(
      pkgs
        .map(npa)
        .map(async (spec: PackageSpec) => {
          const pkgId = options.resolvedDependencies &&
            options.resolvedDependencies[spec.name]
          const dependencyShrinkwrap = pkgId && ctx.shrinkwrap.packages[pkgId]
          try {
            const pkg = await install(spec, ctx, Object.assign({}, options, {
              pkgId,
              dependencyShrinkwrap,
            }))
            if (options.keypath && options.keypath.indexOf(pkg.id) !== -1) {
              return null
            }
            return pkg
          } catch (err) {
            if (options.optional) {
              logger.warn({
                message: `Skipping failed optional dependency ${pkgId || spec.rawSpec}`,
                err,
              })
              return null // is it OK to return null?
            }
            throw err
          }
        })
    )
  )
  .filter(pkg => pkg)

  return installedPkgs
}

async function install (
  spec: PackageSpec,
  ctx: InstallContext,
  options: {
    linkLocal: boolean,
    force: boolean,
    root: string,
    storePath: string,
    metaCache: Map<string, PackageMeta>,
    tag: string,
    got: Got,
    keypath: string[],
    pkgId?: string,
    dependencyShrinkwrap?: DependencyShrinkwrap,
    optional: boolean,
    dependent: string,
    depth: number,
    engineStrict: boolean,
    nodeVersion: string,
    baseNodeModules: string,
  }
) {
  const keypath = options.keypath || []
  const update = keypath.length <= options.depth

  const fetchedPkg = await fetch(spec, Object.assign({}, options, {
    update,
    shrinkwrapResolution: options.dependencyShrinkwrap && options.dependencyShrinkwrap.resolution,
    fetchingLocker: ctx.fetchingLocker,
  }))

  ctx.shrinkwrap.packages[fetchedPkg.id] = ctx.shrinkwrap.packages[fetchedPkg.id] || {}
  ctx.shrinkwrap.packages[fetchedPkg.id].resolution = fetchedPkg.resolution

  logFetchStatus(spec.rawSpec, fetchedPkg)
  const pkg = await fetchedPkg.fetchingPkg

  if (!options.force) {
    await isInstallable(pkg, fetchedPkg, options)
  }

  const realModules = path.join(options.baseNodeModules, `.${fetchedPkg.id}`, 'node_modules')

  const dependency: InstalledPackage = Object.assign({}, fetchedPkg, {
    keypath,
    dependencies: [],
    optional: options.optional === true,
    pkg,
    hardlinkedLocation: path.join(realModules, pkg.name),
    modules: realModules,
  })

  if (keypath.indexOf(dependency.id) !== -1) {
    return dependency
  }

  addInstalledPkg(ctx.installs, dependency)

  // NOTE: the current install implementation
  // does not return enough info for packages that were already installed
  addToGraph(ctx.graph, options.dependent, dependency)

  if (!ctx.installed.has(dependency.id)) {
    ctx.installed.add(dependency.id)
    dependency.dependencies = await installDependencies(
      pkg,
      dependency,
      ctx,
      realModules,
      Object.assign({}, options, {
        resolvedDependencies: options.dependencyShrinkwrap && options.dependencyShrinkwrap.dependencies
      })
    )
    if (dependency.dependencies.length) {
      ctx.shrinkwrap.packages[dependency.id].dependencies = dependency.dependencies
        .reduce((resolutions, dep) => Object.assign(resolutions, {
          [dep.pkg.name]: dep.id
        }), {})
    }
  }

  const newlyFetched = await dependency.fetchingFiles
  await ctx.linkingLocker(dependency.hardlinkedLocation, async function () {
    const pkgJsonPath = path.join(dependency.hardlinkedLocation, 'package.json')
    if (newlyFetched || options.force || !await exists(pkgJsonPath) || !await pkgLinkedToStore()) {
      await rimraf(dependency.hardlinkedLocation)
      const stage = path.join(realModules, `${pkg.name}+stage`)
      await rimraf(stage)
      await hardlinkDir(dependency.path, stage)
      await fs.rename(stage, dependency.hardlinkedLocation)

      if (ctx.installationSequence.indexOf(dependency.id) === -1) {
        ctx.installationSequence.push(dependency.id)
      }
    }

    async function pkgLinkedToStore () {
      const pkgJsonPathInStore = path.join(dependency.path, 'package.json')
      if (await isSameFile(pkgJsonPath, pkgJsonPathInStore)) return true
      logger.info(`Relinking ${dependency.hardlinkedLocation} from the store`)
      return false
    }
  })

  return dependency
}

async function isSameFile (file1: string, file2: string) {
  const stats = await Promise.all([fs.stat(file1), fs.stat(file2)])
  return stats[0].ino === stats[1].ino
}

async function logFetchStatus(pkgRawSpec: string, fetchedPkg: FetchedPackage) {
  const pkg = await fetchedPkg.fetchingPkg
  await fetchedPkg.fetchingFiles
  logStatus({ status: 'done', pkg: {rawSpec: pkgRawSpec, name: pkg.name, version: pkg.version}})
}

function addToGraph (graph: Graph, dependent: string, dependency: InstalledPackage) {
  graph[dependent] = graph[dependent] || {}
  graph[dependent].dependencies = graph[dependent].dependencies || {}

  updateDependencyResolution(graph, dependent, dependency.pkg.name, dependency.id)

  graph[dependency.id] = graph[dependency.id] || {}
  graph[dependency.id].dependents = graph[dependency.id].dependents || []

  if (graph[dependency.id].dependents.indexOf(dependent) === -1) {
    graph[dependency.id].dependents.push(dependent)
  }
}

function updateDependencyResolution (graph: Graph, dependent: string, depName: string, newDepId: string) {
  if (graph[dependent].dependencies[depName] &&
    graph[dependent].dependencies[depName] !== newDepId) {
    removeIfNoDependents(graph, graph[dependent].dependencies[depName], dependent)
  }
  graph[dependent].dependencies[depName] = newDepId
}

function removeIfNoDependents(graph: Graph, id: string, removedDependent: string) {
  if (graph[id] && graph[id].dependents && graph[id].dependents.length === 1 &&
    graph[id].dependents[0] === removedDependent) {
      Object.keys(graph[id].dependencies || {}).forEach(depName => removeIfNoDependents(graph, graph[id].dependencies[depName], id))
      delete graph[id]
  }
}

async function isInstallable (
  pkg: Package,
  fetchedPkg: FetchedPackage,
  options: {
    optional: boolean,
    engineStrict: boolean,
    nodeVersion: string,
  }
): Promise<void> {
  const warn = await installChecks.checkPlatform(pkg) || await installChecks.checkEngine(pkg, {
    pnpmVersion: pnpmPkg.version,
    nodeVersion: options.nodeVersion
  })
  if (!warn) return
  installCheckLogger.warn(warn)
  if (options.engineStrict || options.optional) {
    await fetchedPkg.abort()
    throw warn
  }
}

async function installDependencies (
  pkg: Package,
  dependency: InstalledPackage,
  ctx: InstallContext,
  modules: string,
  opts: {
    linkLocal: boolean,
    force: boolean,
    root: string,
    storePath: string,
    metaCache: Map<string, PackageMeta>,
    tag: string,
    got: Got,
    keypath: string[],
    resolvedDependencies?: ResolvedDependencies,
    optional: boolean,
    dependent: string,
    depth: number,
    engineStrict: boolean,
    nodeVersion: string,
    baseNodeModules: string,
  }
): Promise<InstalledPackage[]> {
  const depsInstallOpts = Object.assign({}, opts, {
    keypath: opts.keypath.concat([ dependency.id ]),
    dependent: dependency.id,
    root: dependency.srcPath,
    fetchingFiles: dependency.fetchingFiles,
  })

  const bundledDeps = pkg.bundleDependencies || pkg.bundledDependencies || []
  const filterDeps = getNotBundledDeps.bind(null, bundledDeps)
  const deps = filterDeps(pkg.dependencies || {})
  const optionalDeps = filterDeps(pkg.optionalDependencies || {})

  const installedDeps: InstalledPackage[] = await installAll(ctx, deps, optionalDeps, modules, depsInstallOpts)

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

function getRawSpec (name: string, version: string) {
  return version === '*' ? name : `${name}@${version}`
}
