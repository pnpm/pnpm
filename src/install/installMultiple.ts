import path = require('path')
import fetch, {FetchedPackage, FetchOptions} from './fetch'
import {InstallContext, InstalledPackages} from '../api/install'
import {Dependencies} from '../types'
import linkBins from './linkBins'
import memoize from '../memoize'

export type InstallOptions = FetchOptions & {
  optional?: boolean,
  dependent: string,
  depth: number,
}

export type InstalledPackage = FetchedPackage & {
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
export default async function installMultiple (ctx: InstallContext, pkgsMap: Dependencies, modules: string, options: InstallOptions): Promise<InstalledPackage[]> {
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

  await linkBins(modules)

  return installedPkgs
}

async function install (pkgRawSpec: string, modules: string, ctx: InstallContext, options: InstallOptions) {
  options.keypath = options.keypath || []

  const fetchedPkg = await fetch(ctx.fetchLocks, pkgRawSpec, modules, options)
  const dependency: InstalledPackage = Object.assign({}, fetchedPkg, {
    keypath: options.keypath,
    dependencies: [],
    optional: options.optional === true,
  })

  addInstalledPkg(ctx.installs, dependency)

  ctx.store.packages[options.dependent] = ctx.store.packages[options.dependent] || {}
  ctx.store.packages[options.dependent].dependencies = ctx.store.packages[options.dependent].dependencies || {}

  // NOTE: the current install implementation
  // does not return enough info for packages that were already installed
  if (dependency.fromCache || options.keypath.indexOf(dependency.id) !== -1) {
    return dependency
  }

  ctx.store.packages[options.dependent].dependencies[dependency.pkg.name] = dependency.id

  ctx.store.packages[dependency.id] = ctx.store.packages[dependency.id] || {}
  ctx.store.packages[dependency.id].dependents = ctx.store.packages[dependency.id].dependents || []
  if (ctx.store.packages[dependency.id].dependents.indexOf(options.dependent) === -1) {
    ctx.store.packages[dependency.id].dependents.push(options.dependent)
  }

  // when a package was already installed, update the subdependencies only to the specified depth.
  if (!dependency.justFetched && options.keypath.length >= options.depth) {
    return dependency
  }

  dependency.dependencies = await memoize(ctx.installLocks, dependency.id, () => {
    return installDependencies(dependency, ctx, options)
  })

  return dependency
}

async function installDependencies (dependency: InstalledPackage, ctx: InstallContext, opts: InstallOptions) {
  const depsInstallOpts = Object.assign({}, opts, {
    keypath: (opts.keypath || []).concat([ dependency.id ]),
    dependent: dependency.id,
    root: dependency.srcPath,
  })
  const modules = path.join(dependency.path, 'node_modules')

  const dependencies = Array.prototype.concat.apply([], await Promise.all([
    installMultiple(
      ctx,
      dependency.pkg.dependencies || {},
      modules,
      Object.assign({}, depsInstallOpts, {
        optional: dependency.optional
      })
    ),
    installMultiple(
      ctx,
      dependency.pkg.optionalDependencies || {},
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

  return dependencies
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
