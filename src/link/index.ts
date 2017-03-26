import fs = require('mz/fs')
import path = require('path')
import linkDir from 'link-dir'
import symlinkDir from 'symlink-dir'
import exists = require('path-exists')
import logger from 'pnpm-logger'
import R = require('ramda')
import globalBinPath = require('global-bin-path')
import {InstalledPackage} from '../install/installMultiple'
import {InstalledPackages} from '../api/install'
import linkBins from './linkBins'
import {Package} from '../types'
import resolvePeers from './resolvePeers'

export type LinkedPackage = {
  id: string,
  pkg: Package,
  hardlinkedLocation: string,
  modules: string,
  path: string,
  fetchingFiles: Promise<boolean>,
  dependencies: string[],
}

export type LinkedPackagesMap = {
  [id: string]: LinkedPackage
}

export default async function (
  topPkgs: InstalledPackage[],
  installedPkgs: InstalledPackages,
  opts: {
    force: boolean,
    global: boolean,
    baseNodeModules: string,
  }
): Promise<LinkedPackagesMap> {
  const pkgsToLink = await resolvePeers(R.values(installedPkgs)
    .filter(installedPkg => installedPkg.isInstallable)
    .reduce((pkgsToLink, installedPkg) => {
      const modules = path.join(opts.baseNodeModules, `.${installedPkg.id}`, 'node_modules')
      pkgsToLink[installedPkg.id] = {
        id: installedPkg.id,
        pkg: installedPkg.pkg,
        fetchingFiles: installedPkg.fetchingFiles,
        modules,
        hardlinkedLocation: path.join(modules, installedPkg.pkg.name),
        path: installedPkg.path,
        dependencies: installedPkg.dependencies,
      }
      return pkgsToLink
    }, {}))

  for (let id of R.keys(pkgsToLink)) {
    await linkPkg(pkgsToLink[id], opts)
  }

  for (let id of R.keys(pkgsToLink)) {
    await linkModules(pkgsToLink[id], pkgsToLink)
  }

  for (let pkg of topPkgs) {
    if (!pkg.isInstallable) continue
    const dest = path.join(opts.baseNodeModules, pkg.pkg.name)
    await symlinkDir(pkgsToLink[pkg.id].hardlinkedLocation, dest)
  }
  const binPath = opts.global ? globalBinPath() : path.join(opts.baseNodeModules, '.bin')
  await linkBins(opts.baseNodeModules, binPath)

  return pkgsToLink
}

async function linkPkg (
  dependency: LinkedPackage,
  opts: {
    force: boolean,
    baseNodeModules: string,
  }
) {
  const newlyFetched = await dependency.fetchingFiles
  const pkgJsonPath = path.join(dependency.hardlinkedLocation, 'package.json')
  if (newlyFetched || opts.force || !await exists(pkgJsonPath) || !await pkgLinkedToStore()) {
    await linkDir(dependency.path, dependency.hardlinkedLocation)
  }

  async function pkgLinkedToStore () {
    const pkgJsonPathInStore = path.join(dependency.path, 'package.json')
    if (await isSameFile(pkgJsonPath, pkgJsonPathInStore)) return true
    logger.info(`Relinking ${dependency.hardlinkedLocation} from the store`)
    return false
  }
}

async function isSameFile (file1: string, file2: string) {
  const stats = await Promise.all([fs.stat(file1), fs.stat(file2)])
  return stats[0].ino === stats[1].ino
}

async function linkModules (
  dependency: LinkedPackage,
  pkgsToLink: LinkedPackagesMap
) {
  for (let depId of dependency.dependencies) {
    const subdep = pkgsToLink[depId]
    if (!subdep) continue
    const dest = path.join(dependency.modules, subdep.pkg.name)
    await symlinkDir(subdep.hardlinkedLocation, dest)
  }

  const binPath = path.join(dependency.hardlinkedLocation, 'node_modules', '.bin')
  await linkBins(dependency.modules, binPath, dependency.pkg.name)

  // link also the bundled dependencies` bins
  if (dependency.pkg.bundledDependencies || dependency.pkg.bundleDependencies) {
    const bundledModules = path.join(dependency.hardlinkedLocation, 'node_modules')
    await linkBins(bundledModules, binPath)
  }
}
