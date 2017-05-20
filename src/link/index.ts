import fs = require('mz/fs')
import path = require('path')
import linkDir from 'link-dir'
import symlinkDir from 'symlink-dir'
import exists = require('path-exists')
import logger from 'pnpm-logger'
import R = require('ramda')
import pLimit = require('p-limit')
import {InstalledPackage} from '../install/installMultiple'
import {InstalledPackages} from '../api/install'
import linkBins from './linkBins'
import {Package, Dependencies} from '../types'
import {Resolution} from '../resolve'
import resolvePeers, {DependencyTreeNode, DependencyTreeNodeMap} from './resolvePeers'
import logStatus from '../logging/logInstallStatus'
import pkgIdToFilename from '../fs/pkgIdToFilename'
import updateShrinkwrap from './updateShrinkwrap'
import {Shrinkwrap} from '../fs/shrinkwrap'

export type LinkedPackage = {
  id: string,
  name: string,
  version: string,
  peerDependencies: Dependencies,
  hasBundledDependencies: boolean,
  localLocation: string,
  path: string,
  resolution: Resolution,
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
    bin: string,
    topParents: {name: string, version: string}[],
    shrinkwrap: Shrinkwrap,
  }
): Promise<DependencyTreeNodeMap> {
  const pkgsToLinkMap = R.values(installedPkgs)
    .reduce((pkgsToLink, installedPkg) => {
      pkgsToLink[installedPkg.id] = {
        id: installedPkg.id,
        name: installedPkg.pkg.name,
        version: installedPkg.pkg.version,
        peerDependencies: installedPkg.pkg.peerDependencies || {},
        hasBundledDependencies: !!(installedPkg.pkg.bundledDependencies || installedPkg.pkg.bundleDependencies),
        resolution: installedPkg.resolution,
        fetchingFiles: installedPkg.fetchingFiles,
        localLocation: path.join(opts.baseNodeModules, `.${pkgIdToFilename(installedPkg.id)}`),
        path: installedPkg.path,
        dependencies: installedPkg.dependencies,
      }
      return pkgsToLink
    }, {})
  const topPkgIds = topPkgs.filter(pkg => pkg.isInstallable).map(pkg => pkg.id)
  const pkgsToLink = await resolvePeers(pkgsToLinkMap, topPkgIds, opts.topParents)
  updateShrinkwrap(pkgsToLink, opts.shrinkwrap)

  const flatResolvedDeps =  R.values(pkgsToLink)

  await linkAllPkgs(flatResolvedDeps, opts)

  await linkAllModules(flatResolvedDeps, pkgsToLink)

  for (let pkg of flatResolvedDeps.filter(pkg => pkg.depth === 0)) {
    await symlinkDependencyTo(pkg, opts.baseNodeModules)
    logStatus({
      status: 'installed',
      pkgId: pkg.id,
    })
  }
  await linkBins(opts.baseNodeModules, opts.bin)

  return pkgsToLink
}

const limitLinking = pLimit(16)

async function linkAllPkgs (
  alldeps: DependencyTreeNode[],
  opts: {
    force: boolean,
    global: boolean,
    baseNodeModules: string,
  }
) {
  return Promise.all(
    alldeps.map(pkg => limitLinking(() => linkPkg(pkg, opts)))
  )
}

async function linkAllModules (
  pkgs: DependencyTreeNode[],
  pkgMap: DependencyTreeNodeMap
) {
  return Promise.all(
    pkgs.map(pkg => limitLinking(() => linkModules(pkg, pkgMap)))
  )
}

async function linkPkg (
  dependency: DependencyTreeNode,
  opts: {
    force: boolean,
    baseNodeModules: string,
  }
) {
  const newlyFetched = await dependency.fetchingFiles

  const pkgJsonPath = path.join(dependency.hardlinkedLocation, 'package.json')
  if (newlyFetched || opts.force || !await exists(pkgJsonPath) || !await pkgLinkedToStore(pkgJsonPath, dependency)) {
    await linkDir(dependency.path, dependency.hardlinkedLocation)
  }
}

async function pkgLinkedToStore (pkgJsonPath: string, dependency: DependencyTreeNode) {
  const pkgJsonPathInStore = path.join(dependency.path, 'package.json')
  if (await isSameFile(pkgJsonPath, pkgJsonPathInStore)) return true
  logger.info(`Relinking ${dependency.hardlinkedLocation} from the store`)
  return false
}

async function isSameFile (file1: string, file2: string) {
  const stats = await Promise.all([fs.stat(file1), fs.stat(file2)])
  return stats[0].ino === stats[1].ino
}

async function linkModules (
  dependency: DependencyTreeNode,
  pkgMap: DependencyTreeNodeMap
) {
  await Promise.all(
    R.props<DependencyTreeNode>(dependency.children, pkgMap)
      .map(child => symlinkDependencyTo(child, dependency.modules))
  )

  const binPath = path.join(dependency.hardlinkedLocation, 'node_modules', '.bin')
  await linkBins(dependency.modules, binPath, dependency.name)

  // link also the bundled dependencies` bins
  if (dependency.hasBundledDependencies) {
    const bundledModules = path.join(dependency.hardlinkedLocation, 'node_modules')
    await linkBins(bundledModules, binPath)
  }
}

function symlinkDependencyTo (dependency: DependencyTreeNode, dest: string) {
  dest = path.join(dest, dependency.name)
  return symlinkDir(dependency.hardlinkedLocation, dest)
}
