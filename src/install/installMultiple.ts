import path = require('path')
import logger from 'pnpm-logger'
import pFilter = require('p-filter')
import R = require('ramda')
import fetch, {FetchedPackage} from './fetch'
import {InstallContext, InstalledPackages} from '../api/install'
import {Dependencies} from '../types'
import memoize from '../memoize'
import {Package} from '../types'
import linkDir from 'link-dir'
import installChecks = require('pnpm-install-checks')
import pnpmPkg from '../pnpmPkgJson'
import symlinkDir from 'symlink-dir'
import exists = require('path-exists')
import logStatus from '../logging/logInstallStatus'
import rimraf = require('rimraf-then')
import fs = require('mz/fs')
import {Got} from '../network/got'
import {
  DependencyShrinkwrap,
  ResolvedDependencies,
} from '../fs/shrinkwrap'
import {PackageSpec, PackageMeta} from '../resolve'
import linkBins from '../install/linkBins'
import getLinkTarget = require('get-link-target')
import depsToSpecs from '../depsToSpecs'

const installCheckLogger = logger('install-check')

export type InstalledPackage = {
  id: string,
  pkg: Package,
  srcPath?: string,
  optional: boolean,
  hardlinkedLocation: string,
  modules: string,
}

export default async function installAll (
  ctx: InstallContext,
  specs: PackageSpec[],
  optionalDependencies: string[],
  modules: string,
  options: {
    force: boolean,
    root: string,
    storePath: string,
    localRegistry: string,
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
    installMultiple(ctx, nonOptionalDepSpecs, modules, Object.assign({}, options, {optional: false, keypath})),
    installMultiple(ctx, optionalDepSpecs, modules, Object.assign({}, options, {optional: true, keypath})),
  ]))

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
  specs: PackageSpec[],
  modules: string,
  options: {
    force: boolean,
    root: string,
    storePath: string,
    localRegistry: string,
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
  const nonLinkedPkgs = modules === options.baseNodeModules
    // only check modules on the first level
    ? await pFilter(specs, (spec: PackageSpec) => !spec.name || isInnerLink(modules, spec.name))
    : specs

  const installedPkgs: InstalledPackage[] = <InstalledPackage[]>(
    await Promise.all(
      nonLinkedPkgs
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
                message: `Skipping failed optional dependency ${pkgId || spec.raw}`,
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

async function isInnerLink (modules: string, depName: string) {
  let linkTarget: string
  try {
    const linkPath = path.join(modules, depName)
    linkTarget = await getLinkTarget(linkPath)
  } catch (err) {
    if (err.code === 'ENOENT') return true
    throw err
  }

  if (linkTarget.startsWith(modules)) {
    return true
  }
  logger.info(`${depName} is linked to ${modules} from ${linkTarget}`)
  return false
}

async function install (
  spec: PackageSpec,
  ctx: InstallContext,
  options: {
    force: boolean,
    root: string,
    storePath: string,
    localRegistry: string,
    metaCache: Map<string, PackageMeta>,
    got: Got,
    keypath: string[],
    pkgId?: string,
    dependencyShrinkwrap?: DependencyShrinkwrap,
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
    shrinkwrapResolution: options.dependencyShrinkwrap && options.dependencyShrinkwrap.resolution,
    fetchingLocker: ctx.fetchingLocker,
  }))

  if (keypath.indexOf(fetchedPkg.id) !== -1) {
    return fetchedPkg
  }

  ctx.shrinkwrap.packages[fetchedPkg.id] = ctx.shrinkwrap.packages[fetchedPkg.id] || {}
  ctx.shrinkwrap.packages[fetchedPkg.id].resolution = fetchedPkg.resolution

  const pkg = await fetchedPkg.fetchingPkg

  if (!options.force) {
    await isInstallable(pkg, fetchedPkg, options)
  }

  const modules = path.join(options.baseNodeModules, `.${fetchedPkg.id}`, 'node_modules')

  const dependency: InstalledPackage = {
    id: fetchedPkg.id,
    srcPath: fetchedPkg.srcPath,
    optional: options.optional === true,
    pkg,
    hardlinkedLocation: path.join(modules, pkg.name),
    modules,
  }

  addInstalledPkg(ctx.installs, dependency)

  const linking = ctx.linkingLocker(dependency.hardlinkedLocation, async function () {
    const newlyFetched = await fetchedPkg.fetchingFiles
    const pkgJsonPath = path.join(dependency.hardlinkedLocation, 'package.json')
    if (newlyFetched || options.force || !await exists(pkgJsonPath) || !await pkgLinkedToStore()) {
      await linkDir(fetchedPkg.path, dependency.hardlinkedLocation)

      if (ctx.installationSequence.indexOf(dependency.id) === -1) {
        ctx.installationSequence.push(dependency.id)
      }
    }

    async function pkgLinkedToStore () {
      const pkgJsonPathInStore = path.join(fetchedPkg.path, 'package.json')
      if (await isSameFile(pkgJsonPath, pkgJsonPathInStore)) return true
      logger.info(`Relinking ${dependency.hardlinkedLocation} from the store`)
      return false
    }
  })

  if (!ctx.installed.has(dependency.id)) {
    ctx.installed.add(dependency.id)
    const dependencies = await installDependencies(
      pkg,
      dependency,
      ctx,
      modules,
      Object.assign({}, options, {
        resolvedDependencies: options.dependencyShrinkwrap && options.dependencyShrinkwrap.dependencies
      })
    )
    if (dependencies.length) {
      ctx.shrinkwrap.packages[dependency.id].dependencies = dependencies
        .reduce((resolutions, dep) => Object.assign(resolutions, {
          [dep.pkg.name]: dep.id
        }), {})
    }

    await linking

    const binPath = path.join(dependency.hardlinkedLocation, 'node_modules', '.bin')
    await linkBins(modules, binPath, pkg.name)

    // link also the bundled dependencies` bins
    if (pkg.bundledDependencies || pkg.bundleDependencies) {
      const bundledModules = path.join(dependency.hardlinkedLocation, 'node_modules')
      await linkBins(bundledModules, binPath)
    }
  } else {
    await linking
  }

  logStatus({
    status: 'installed',
    pkg: Object.assign({}, loggedPkg, {version: pkg.version}),
  })

  return dependency
}

async function isSameFile (file1: string, file2: string) {
  const stats = await Promise.all([fs.stat(file1), fs.stat(file2)])
  return stats[0].ino === stats[1].ino
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
    force: boolean,
    root: string,
    storePath: string,
    localRegistry: string,
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
