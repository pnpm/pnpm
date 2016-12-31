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
    .reduce((deps, depName) => {
      if (!optionalDependencies[depName]) {
        deps[depName] = dependencies[depName]
      }
      return deps
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
      .map(subdep => symlinkToModules(subdep.path, modules))
  )
  await linkBins(modules)

  return installedPkgs
}

async function installMultiple (ctx: InstallContext, pkgsMap: Dependencies, modules: string, options: MultipleInstallOpts): Promise<InstalledPackage[]> {
  pkgsMap = pkgsMap || {}

  const pkgs = Object.keys(pkgsMap).map(pkgName => getRawSpec(pkgName, pkgsMap[pkgName]))

  ctx.store.packages = ctx.store.packages || {}

  const installedPkgs: InstalledPackage[] = <InstalledPackage[]>(
    await Promise.all(pkgs.map(async function (pkgRawSpec: string) {
      try {
        const pkg = await install(pkgRawSpec, modules, ctx, options)
        if (options.keypath && options.keypath.indexOf(pkg.id) !== -1) {
          return null
        }
        const modulesInStore = path.join(options.nodeModulesStore, pkg.id)
        await linkDir(modulesInStore, path.join(modules, pkg.pkg.name, 'node_modules'))
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
  options.keypath = options.keypath || []
  const update = options.keypath.length <= options.depth

  const fetchedPkg = await fetch(ctx, pkgRawSpec, modules, Object.assign({}, options, {update}))
  const pkg = await fetchedPkg.fetchingPkg

  if (!options.force) {
    await isInstallable(pkg, fetchedPkg, options)
  }

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
    await dependency.fetchingFiles
    return dependency
  }

  ctx.store.packages[options.dependent].dependencies[pkg.name] = dependency.id

  ctx.store.packages[dependency.id] = ctx.store.packages[dependency.id] || {}
  ctx.store.packages[dependency.id].dependents = ctx.store.packages[dependency.id].dependents || []
  if (ctx.store.packages[dependency.id].dependents.indexOf(options.dependent) === -1) {
    ctx.store.packages[dependency.id].dependents.push(options.dependent)
  }

  const modulesInStore = path.join(options.nodeModulesStore, dependency.id)

  // when a package was already installed, update the subdependencies only to the specified depth.
  if ((await exists(modulesInStore)) && options.keypath.length >= options.depth) {
    await dependency.fetchingFiles
    return dependency
  }

  // greedy installation does not work with bundled dependencies
  if (pkg.bundleDependencies && pkg.bundleDependencies.length || pkg.bundledDependencies && pkg.bundledDependencies.length) {
    await dependency.fetchingFiles
  }

  dependency.dependencies = await memoize<InstalledPackage[]>(ctx.installLocks, dependency.id, () => {
    return installDependencies(pkg, dependency, ctx, modulesInStore, options)
  })

  await dependency.fetchingFiles
  return dependency
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
