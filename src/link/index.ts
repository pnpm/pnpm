import fs = require('mz/fs')
import path = require('path')
import linkDir = require('link-dir')
import symlinkDir = require('symlink-dir')
import exists = require('path-exists')
import logger from 'pnpm-logger'
import R = require('ramda')
import pLimit = require('p-limit')
import {InstalledPackage} from '../install/installMultiple'
import {InstalledPackages, TreeNode} from '../api/install'
import linkBins from './linkBins'
import {Package, Dependencies} from '../types'
import {Resolution} from '../resolve'
import resolvePeers, {DependencyTreeNode, DependencyTreeNodeMap} from './resolvePeers'
import logStatus from '../logging/logInstallStatus'
import updateShrinkwrap from './updateShrinkwrap'
import {shortIdToFullId} from '../fs/shrinkwrap'
import {Shrinkwrap, DependencyShrinkwrap} from 'pnpm-lockfile'
import removeOrphanPkgs from '../api/removeOrphanPkgs'
import ncpCB = require('ncp')
import thenify = require('thenify')

const ncp = thenify(ncpCB)

export default async function (
  topPkgs: InstalledPackage[],
  rootNodeIds: string[],
  tree: {[nodeId: string]: TreeNode},
  opts: {
    force: boolean,
    global: boolean,
    baseNodeModules: string,
    bin: string,
    topParents: {name: string, version: string}[],
    shrinkwrap: Shrinkwrap,
    privateShrinkwrap: Shrinkwrap,
    production: boolean,
    optional: boolean,
    root: string,
    storePath: string,
    skipped: Set<string>,
    pkg: Package,
  }
): Promise<{
  linkedPkgsMap: DependencyTreeNodeMap,
  shrinkwrap: Shrinkwrap,
  newPkgResolvedIds: string[],
}> {
  const topPkgIds = topPkgs.map(pkg => pkg.id)
  const pkgsToLink = await resolvePeers(tree, rootNodeIds, topPkgIds, opts.topParents)
  const newShr = updateShrinkwrap(pkgsToLink, opts.shrinkwrap, opts.pkg)

  await removeOrphanPkgs(opts.privateShrinkwrap, newShr, opts.root, opts.storePath)

  let flatResolvedDeps =  R.values(pkgsToLink).filter(dep => !opts.skipped.has(dep.id))
  if (opts.production) {
    flatResolvedDeps = flatResolvedDeps.filter(dep => !dep.dev)
  }
  if (!opts.optional) {
    flatResolvedDeps = flatResolvedDeps.filter(dep => !dep.optional)
  }

  const filterOpts = {
    noDev: opts.production,
    noOptional: !opts.optional,
    skipped: opts.skipped,
  }
  const newPkgResolvedIds = await linkNewPackages(
    filterShrinkwrap(opts.privateShrinkwrap, filterOpts),
    filterShrinkwrap(newShr, filterOpts),
    pkgsToLink,
    opts
  )

  for (let pkg of flatResolvedDeps.filter(pkg => pkg.depth === 0)) {
    await symlinkDependencyTo(pkg, opts.baseNodeModules)
    logStatus({
      status: 'installed',
      pkgId: pkg.id,
    })
  }
  await linkBins(opts.baseNodeModules, opts.bin)

  return {
    linkedPkgsMap: pkgsToLink,
    shrinkwrap: newShr,
    newPkgResolvedIds,
  }
}

function filterShrinkwrap (
  shr: Shrinkwrap,
  opts: {
    noDev: boolean,
    noOptional: boolean,
    skipped: Set<string>,
  }
): Shrinkwrap {
  let pairs = R.toPairs<string, DependencyShrinkwrap>(shr.packages)
    .filter(pair => !opts.skipped.has(pair[1].id || shortIdToFullId(pair[0], shr.registry)))
  if (opts.noDev) {
    pairs = pairs.filter(pair => !pair[1].dev)
  }
  if (opts.noOptional) {
    pairs = pairs.filter(pair => !pair[1].optional)
  }
  return {
    version: shr.version,
    registry: shr.registry,
    specifiers: shr.specifiers,
    packages: R.fromPairs(pairs),
  } as Shrinkwrap
}

async function linkNewPackages (
  privateShrinkwrap: Shrinkwrap,
  shrinkwrap: Shrinkwrap,
  pkgsToLink: DependencyTreeNodeMap,
  opts: {
    force: boolean,
    global: boolean,
    baseNodeModules: string,
    optional: boolean,
  }
): Promise<string[]> {
  const nextPkgResolvedIds = R.keys(shrinkwrap.packages)
  const prevPkgResolvedIds = R.keys(privateShrinkwrap.packages)

  // TODO: what if the registries differ?
  const newPkgResolvedIds = (
      opts.force
        ? nextPkgResolvedIds
        : R.difference(nextPkgResolvedIds, prevPkgResolvedIds)
    )
    .map(shortId => shortIdToFullId(shortId, shrinkwrap.registry))

  const newPkgs = R.props<DependencyTreeNode>(newPkgResolvedIds, pkgsToLink)

  try {
    await linkAllPkgs(linkPkg, newPkgs, opts)
  } catch (err) {
    if (!err.message.startsWith('EXDEV: cross-device link not permitted')) throw err
    logger.warn(err.message)
    logger.info('Falling back to copying packages from store')
    await linkAllPkgs(copyPkg, newPkgs, opts)
  }

  if (!opts.force) {
    // add subdependencies that have been updated
    // TODO: no need to relink everything. Can be relinked only what was changed
    for (const shortId of nextPkgResolvedIds) {
      if (privateShrinkwrap.packages[shortId] &&
        !R.equals(privateShrinkwrap.packages[shortId].dependencies, shrinkwrap.packages[shortId].dependencies)) {
        const resolvedId = shortIdToFullId(shortId, shrinkwrap.registry)
        newPkgs.push(pkgsToLink[resolvedId])
      }
    }
  }

  await linkAllModules(newPkgs, pkgsToLink, {optional: opts.optional})

  return newPkgResolvedIds
}

const limitLinking = pLimit(16)

async function linkAllPkgs (
  linkPkg: (dependency: DependencyTreeNode, opts: {
    force: boolean,
    baseNodeModules: string,
  }) => Promise<void>,
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
  pkgMap: DependencyTreeNodeMap,
  opts: {
    optional: boolean,
  }
) {
  return Promise.all(
    pkgs.map(pkg => limitLinking(() => linkModules(pkg, pkgMap, opts)))
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
  if (newlyFetched || opts.force || !await exists(pkgJsonPath)) {
    if (dependency.independent) return

    await linkDir(dependency.path, dependency.hardlinkedLocation)
    return
  }
  if (!await pkgLinkedToStore(pkgJsonPath, dependency)) {
    await linkDir(dependency.path, dependency.hardlinkedLocation)
  }
}

async function copyPkg (
  dependency: DependencyTreeNode,
  opts: {
    force: boolean,
    baseNodeModules: string,
  }
) {
  const newlyFetched = await dependency.fetchingFiles

  const pkgJsonPath = path.join(dependency.hardlinkedLocation, 'package.json')
  if (newlyFetched || opts.force || !await exists(pkgJsonPath)) {
    await ncp(dependency.path, dependency.hardlinkedLocation)
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
  pkgMap: DependencyTreeNodeMap,
  opts: {
    optional: boolean,
  }
) {
  const childrenToLink = opts.optional
    ? dependency.children
    : dependency.children.filter(child => !dependency.optionalDependencies.has(pkgMap[child].name))

  await Promise.all(
    R.props<DependencyTreeNode>(childrenToLink, pkgMap)
      .filter(child => child.installable)
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
