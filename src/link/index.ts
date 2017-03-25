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

export default async function (
  topPkgs: InstalledPackage[],
  nodeModulesPath: string,
  installedPkgs: InstalledPackages,
  opts: {
    force: boolean,
    global: boolean,
  }
) {
  for (let pkgId of R.keys(installedPkgs)) {
    await linkPkg(installedPkgs[pkgId], opts)
  }

  for (let pkgId of R.keys(installedPkgs)) {
    await linkModules(installedPkgs[pkgId], installedPkgs)
  }

  for (let pkg of topPkgs) {
    if (!pkg.isInstallable) continue
    const dest = path.join(nodeModulesPath, pkg.pkg.name)
    await symlinkDir(pkg.hardlinkedLocation, dest)
  }
  const binPath = opts.global ? globalBinPath() : path.join(nodeModulesPath, '.bin')
  await linkBins(nodeModulesPath, binPath)
}

async function linkPkg (
  dependency: InstalledPackage,
  opts: {
    force: boolean,
  }
) {
  if (!dependency.isInstallable) return
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
  dependency: InstalledPackage,
  installedPkgs: InstalledPackages
) {
  for (let depId of dependency.dependencies) {
    const subdep = installedPkgs[depId]
    if (!subdep.isInstallable) continue
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
