import path = require('path')
import fetch, {FetchedPackage, FetchOptions} from './fetch'
import {InstallContext, InstalledPackages} from '../api/install'
import {Dependencies} from '../types'
import linkBins from './linkBins'
import memoize from '../memoize'
import {Package} from '../types'
import symlinkToModules from './symlinkToModules'
import mkdirp from '../fs/mkdirp'

export type InstallOptions = FetchOptions & {
  optional?: boolean,
  dependent: string,
  depth: number,
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

/**
 * Install multiple modules into `modules`.
 *
 * @example
 *     ctx = { }
 *     installMultiple(ctx, { minimatch: '^2.0.0' }, {chokidar: '^1.6.0'}, './node_modules')
 */
export default async function installMultiple (ctx: InstallContext, pkgsMap: Dependencies, modules: string, options: MultipleInstallOpts): Promise<InstalledPackage[]> {
  pkgsMap = pkgsMap || {}

  const pkgs = Object.keys(pkgsMap).map(pkgName => getRawSpec(pkgName, pkgsMap[pkgName]))

  ctx.store.packages = ctx.store.packages || {}

  const installedPkgs: InstalledPackage[] = <InstalledPackage[]>(
    await Promise.all(pkgs.map(async function (pkgRawSpec: string) {
      try {
        return await install(pkgRawSpec, modules, ctx, options)
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

  await options.fetchingFiles

  await mkdirp(modules)
  await Promise.all(
    installedPkgs
      .filter(subdep => !subdep.fromCache)
      .map(subdep => symlinkToModules(subdep.path, modules))
  )
  await linkBins(modules)

  return installedPkgs
}

async function install (pkgRawSpec: string, modules: string, ctx: InstallContext, options: InstallOptions) {
  options.keypath = options.keypath || []

  const fetchedPkg = await fetch(ctx.fetchLocks, pkgRawSpec, modules, options)
  const pkg = await fetchedPkg.fetchingPkg
  const dependency: InstalledPackage = Object.assign({}, fetchedPkg, {
    keypath: options.keypath,
    dependencies: [],
    optional: options.optional === true,
    pkg,
  })

  addInstalledPkg(ctx.installs, dependency)

  ctx.store.packages[options.dependent] = ctx.store.packages[options.dependent] || {}
  ctx.store.packages[options.dependent].dependencies = ctx.store.packages[options.dependent].dependencies || {}

  // NOTE: the current install implementation
  // does not return enough info for packages that were already installed
  if (dependency.fromCache || options.keypath.indexOf(dependency.id) !== -1) {
    return dependency
  }

  ctx.store.packages[options.dependent].dependencies[pkg.name] = dependency.id

  ctx.store.packages[dependency.id] = ctx.store.packages[dependency.id] || {}
  ctx.store.packages[dependency.id].dependents = ctx.store.packages[dependency.id].dependents || []
  if (ctx.store.packages[dependency.id].dependents.indexOf(options.dependent) === -1) {
    ctx.store.packages[dependency.id].dependents.push(options.dependent)
  }

  // when a package was already installed, update the subdependencies only to the specified depth.
  if (!dependency.justFetched && options.keypath.length >= options.depth) {
    return dependency
  }

  // greedy installation does not work with bundled dependencies
  if (pkg.bundleDependencies && pkg.bundleDependencies.length || pkg.bundledDependencies && pkg.bundledDependencies.length) {
    await dependency.fetchingFiles
  }

  dependency.dependencies = await memoize(ctx.installLocks, dependency.id, () => {
    return installDependencies(pkg, dependency, ctx, options)
  })

  return dependency
}

async function installDependencies (pkg: Package, dependency: InstalledPackage, ctx: InstallContext, opts: InstallOptions) {
  const depsInstallOpts = Object.assign({}, opts, {
    keypath: (opts.keypath || []).concat([ dependency.id ]),
    dependent: dependency.id,
    root: dependency.srcPath,
    fetchingFiles: dependency.fetchingFiles,
  })
  const modules = path.join(dependency.path, 'node_modules')
  const optionalDependencies = pkg.optionalDependencies || {}
  const dependencies = pkg.dependencies || {}
  const nonOptionalDependencies = Object.keys(dependencies)
    .reduce((deps, depName) => {
      if (!optionalDependencies[depName]) {
        deps[depName] = dependencies[depName]
      }
      return deps
    }, {})

  const installedDeps = Array.prototype.concat.apply([], await Promise.all([
    installMultiple(
      ctx,
      nonOptionalDependencies,
      modules,
      Object.assign({}, depsInstallOpts, {
        optional: dependency.optional
      })
    ),
    installMultiple(
      ctx,
      optionalDependencies,
      modules,
      Object.assign({}, depsInstallOpts, {
        optional: true
      })
    ),
  ]))
  ctx.piq = ctx.piq || []
  ctx.piq.push({
    path: dependency.path,
    pkgId: dependency.id
  })

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
