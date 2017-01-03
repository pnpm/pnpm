import path = require('path')
import fetch, {FetchedPackage, FetchOptions} from './fetch'
import {InstallContext, InstalledPackages} from '../api/install'
import {Dependencies} from '../types'
import linkBins from './linkBins'
import memoize from '../memoize'
import {Package} from '../types'
import symlinkToModules from './symlinkToModules'
import mkdirp from '../fs/mkdirp'
import installChecks = require('pnpm-install-checks')
import pnpmPkg from '../pnpmPkgJson'
import linkDir from 'link-dir'
import exists = require('exists-file')
import {Graph} from '../fs/graphController'

export type InstallOptions = FetchOptions & {
  optional?: boolean,
  dependent: string,
  depth: number,
  engineStrict: boolean,
  nodeVersion: string,
  nodeModulesStore: string,
}

export type MultipleInstallOpts = InstallOptions & {
  fetchingFiles: Promise<void>
}

export type InstalledPackage = FetchedPackage & {
  pkg: Package,
  keypath: string[],
  optional: boolean,
  dependencies: InstalledPackage[], // is needed to support flat tree
}

export default async function installAll (ctx: InstallContext, dependencies: Dependencies, optionalDependencies: Dependencies, modules: string, options: MultipleInstallOpts): Promise<InstalledPackage[]> {
  const nonOptionalDependencies = Object.keys(dependencies)
    .filter(depName => !optionalDependencies[depName])
    .reduce((nonOptionalDependencies, depName) => {
      nonOptionalDependencies[depName] = dependencies[depName]
      return nonOptionalDependencies
    }, {})

  const installedPkgs: InstalledPackage[] = Array.prototype.concat.apply([], await Promise.all([
    installMultiple(ctx, nonOptionalDependencies, modules, Object.assign({}, options, {optional: false})),
    installMultiple(ctx, optionalDependencies, modules, Object.assign({}, options, {optional: true})),
  ]))

  if (options.fetchingFiles) {
    await options.fetchingFiles
  }

  await mkdirp(modules)
  await Promise.all(
    installedPkgs
      .filter(subdep => !subdep.fromCache)
      .map(subdep => {
        ctx.piq = ctx.piq || []
        ctx.piq.push({
          path: path.join(modules, subdep.pkg.name),
          pkgId: subdep.id
        })
        return linkDir(subdep.path, path.join(modules, subdep.pkg.name))
      })
  )
  await linkBins(modules)

  return installedPkgs
}

async function installMultiple (ctx: InstallContext, pkgsMap: Dependencies, modules: string, options: MultipleInstallOpts): Promise<InstalledPackage[]> {
  pkgsMap = pkgsMap || {}

  const pkgs = Object.keys(pkgsMap).map(pkgName => getRawSpec(pkgName, pkgsMap[pkgName]))

  ctx.graph = ctx.graph || {}

  const installedPkgs: InstalledPackage[] = <InstalledPackage[]>(
    await Promise.all(pkgs.map(async function (pkgRawSpec: string) {
      try {
        const pkg = await install(pkgRawSpec, modules, ctx, options)
        if (options.keypath && options.keypath.indexOf(pkg.id) !== -1) {
          return null
        }
        return pkg
      } catch (err) {
        if (options.optional) {
          console.log(`Skipping failed optional dependency ${pkgRawSpec}:`)
          console.log(err.message || err)
          return null // is it OK to return null?  
        }
        throw err
      }
    }))
  )
  .filter(pkg => pkg)

  return installedPkgs
}

async function install (pkgRawSpec: string, modules: string, ctx: InstallContext, options: InstallOptions) {
  const keypath = options.keypath || []
  const update = keypath.length <= options.depth

  const fetchedPkg = await fetch(ctx, pkgRawSpec, modules, Object.assign({}, options, {update}))
  const pkg = await fetchedPkg.fetchingPkg

  if (!options.force) {
    await isInstallable(pkg, fetchedPkg, options)
  }

  const dependency: InstalledPackage = Object.assign({}, fetchedPkg, {
    keypath,
    dependencies: [],
    optional: options.optional === true,
    pkg,
  })

  if (dependency.fromCache || keypath.indexOf(dependency.id) !== -1) {
    return dependency
  }

  addInstalledPkg(ctx.installs, dependency)

  // NOTE: the current install implementation
  // does not return enough info for packages that were already installed
  addToGraph(ctx.graph, options.dependent, dependency)

  const resolutionPath = path.join(options.nodeModulesStore, dependency.id)
  const modulesInStore = path.join(resolutionPath, 'node_modules')

  dependency.dependencies = await installDependencies(pkg, dependency, ctx, modulesInStore, options)

  await dependency.fetchingFiles
  await memoize(ctx.resolutionLinked, resolutionPath, async function () {
    if (!await exists(path.join(resolutionPath, 'package.json'))) { // in case it was created by a separate installation
      await symlinkToModules(dependency.path, resolutionPath)
    }
  })

  return {...dependency, path: resolutionPath}
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

async function isInstallable (pkg: Package, fetchedPkg: FetchedPackage, options: InstallOptions): Promise<void> {
  const warn = await installChecks.checkPlatform(pkg) || await installChecks.checkEngine(pkg, {
    pnpmVersion: pnpmPkg.version,
    nodeVersion: options.nodeVersion
  })
  if (!warn) return
  switch (warn.code) {
    case 'EBADPLATFORM':
      console.warn(`Unsupported system. Skipping dependency ${fetchedPkg.id}`)
      break
    case 'ENOTSUP':
      console.warn(warn)
      break
  }
  if (options.engineStrict || options.optional) {
    await fetchedPkg.abort()
    throw warn
  }
}

async function installDependencies (pkg: Package, dependency: InstalledPackage, ctx: InstallContext, modules: string, opts: InstallOptions): Promise<InstalledPackage[]> {
  const depsInstallOpts = Object.assign({}, opts, {
    keypath: (opts.keypath || []).concat([ dependency.id ]),
    dependent: dependency.id,
    root: dependency.srcPath,
    fetchingFiles: dependency.fetchingFiles,
  })

  const installedDeps: InstalledPackage[] = await installAll(ctx, pkg.dependencies || {}, pkg.optionalDependencies || {}, modules, depsInstallOpts)

  return installedDeps
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
