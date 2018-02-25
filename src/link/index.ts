import path = require('path')
import symlinkDir = require('symlink-dir')
import R = require('ramda')
import pLimit = require('p-limit')
import {TreeNode} from '../api/install'
import linkBins, {linkPkgBins} from './linkBins'
import {PackageJson} from '@pnpm/types'
import {StoreController} from 'package-store'
import resolvePeers, {DependencyTreeNode, DependencyTreeNodeMap} from './resolvePeers'
import logStatus from '../logging/logInstallStatus'
import updateShrinkwrap from './updateShrinkwrap'
import * as dp from 'dependency-path'
import {Shrinkwrap, DependencyShrinkwrap} from 'pnpm-shrinkwrap'
import removeOrphanPkgs from '../api/removeOrphanPkgs'
import {
  rootLogger,
  statsLogger,
  stageLogger,
} from '../loggers'

export default async function linkPackages (
  rootNodeIdsByAlias: {[alias: string]: string},
  tree: {[nodeId: string]: TreeNode},
  opts: {
    force: boolean,
    dryRun: boolean,
    global: boolean,
    baseNodeModules: string,
    bin: string,
    topParents: {name: string, version: string}[],
    wantedShrinkwrap: Shrinkwrap,
    currentShrinkwrap: Shrinkwrap,
    makePartialCurrentShrinkwrap: boolean,
    production: boolean,
    development: boolean,
    optional: boolean,
    root: string,
    storeController: StoreController,
    skipped: Set<string>,
    pkg: PackageJson,
    independentLeaves: boolean,
    // This is only needed till shrinkwrap v4
    updateShrinkwrapMinorVersion: boolean,
    outdatedPkgs: {[pkgId: string]: string},
    sideEffectsCache: boolean,
    shamefullyFlatten: boolean,
    reinstallForFlatten: boolean,
    hoistedAliases: {[pkgId: string]: string[]},
  }
): Promise<{
  linkedPkgsMap: DependencyTreeNodeMap,
  wantedShrinkwrap: Shrinkwrap,
  currentShrinkwrap: Shrinkwrap,
  newDepPaths: string[],
  removedPkgIds: Set<string>,
  hoistedAliases: {[pkgId: string]: string[]},
}> {
  // TODO: decide what kind of logging should be here.
  // The `Creating dependency tree` is not good to report in all cases as
  // sometimes node_modules is alread up-to-date
  // logger.info(`Creating dependency tree`)
  const resolvePeersResult = await resolvePeers(tree, rootNodeIdsByAlias, opts.topParents, opts.independentLeaves, opts.baseNodeModules)
  const pkgsToLink = resolvePeersResult.resolvedTree
  const newShr = updateShrinkwrap(pkgsToLink, opts.wantedShrinkwrap, opts.pkg)

  const removedPkgIds = await removeOrphanPkgs({
    dryRun: opts.dryRun,
    oldShrinkwrap: opts.currentShrinkwrap,
    newShrinkwrap: newShr,
    prefix: opts.root,
    shamefullyFlatten: opts.shamefullyFlatten,
    storeController: opts.storeController,
    bin: opts.bin,
    hoistedAliases: opts.hoistedAliases,
  })

  let flatResolvedDeps =  R.values(pkgsToLink).filter(dep => !opts.skipped.has(dep.id))
  if (!opts.production) {
    flatResolvedDeps = flatResolvedDeps.filter(dep => dep.dev !== false || dep.optional)
  }
  if (!opts.development) {
    flatResolvedDeps = flatResolvedDeps.filter(dep => dep.dev !== true)
  }
  if (!opts.optional) {
    flatResolvedDeps = flatResolvedDeps.filter(dep => !dep.optional)
  }

  const filterOpts = {
    noProd: !opts.production,
    noDev: !opts.development,
    noOptional: !opts.optional,
    skipped: opts.skipped,
  }
  const newCurrentShrinkwrap = filterShrinkwrap(newShr, filterOpts)
  stageLogger.debug('importing_started')
  const newDepPaths = await linkNewPackages(
    filterShrinkwrap(opts.currentShrinkwrap, filterOpts),
    newCurrentShrinkwrap,
    pkgsToLink,
    opts
  )
  stageLogger.debug('importing_done')

  const rootPkgsToLinkByAbsolutePath = flatResolvedDeps
    .filter(pkg => pkg.depth === 0)
    .reduce((rootPkgsToLink, pkg) => {
      rootPkgsToLink[pkg.absolutePath] = pkg
      return rootPkgsToLink
    }, {})
  for (let rootAlias of R.keys(resolvePeersResult.rootAbsolutePathsByAlias)) {
    const pkg = rootPkgsToLinkByAbsolutePath[resolvePeersResult.rootAbsolutePathsByAlias[rootAlias]]
    if (!pkg) continue
    if (opts.dryRun || !(await symlinkDependencyTo(rootAlias, pkg, opts.baseNodeModules)).reused) {
      const isDev = opts.pkg.devDependencies && opts.pkg.devDependencies[pkg.name]
      const isOptional = opts.pkg.optionalDependencies && opts.pkg.optionalDependencies[pkg.name]
      rootLogger.info({
        added: {
          id: pkg.id,
          name: rootAlias,
          realName: pkg.name,
          version: pkg.version,
          latest: opts.outdatedPkgs[pkg.id],
          dependencyType: isDev && 'dev' || isOptional && 'optional' || 'prod',
        },
      })
    }
    logStatus({
      status: 'installed',
      pkgId: pkg.id,
    })
  }

  if (!opts.dryRun) {
    await linkBins(opts.baseNodeModules, opts.bin)
  }

  if (opts.updateShrinkwrapMinorVersion) {
    // Setting `shrinkwrapMinorVersion` is a temporary solution to
    // have new backward-compatible versions of `shrinkwrap.yaml`
    // w/o changing `shrinkwrapVersion`. From version 4, the
    // `shrinkwrapVersion` field allows numbers like 4.1
    newShr.shrinkwrapMinorVersion = 4
  }
  let currentShrinkwrap: Shrinkwrap
  if (opts.makePartialCurrentShrinkwrap) {
    const packages = opts.currentShrinkwrap.packages || {}
    if (newShr.packages) {
      for (const relDepPath in newShr.packages) {
        const depPath = dp.resolve(newShr.registry, relDepPath)
        if (pkgsToLink[depPath]) {
          packages[relDepPath] = newShr.packages[relDepPath]
        }
      }
    }
    currentShrinkwrap = {...newShr, packages}
  } else if (opts.production && opts.development && opts.optional) {
    currentShrinkwrap = newShr
  } else {
    currentShrinkwrap = newCurrentShrinkwrap
  }

  // Important: shamefullyFlattenTree changes flatResolvedDeps, so keep this at the end
  if (opts.shamefullyFlatten && (opts.reinstallForFlatten || newDepPaths.length > 0 || removedPkgIds.size > 0)) {
    opts.hoistedAliases = await shamefullyFlattenTree(flatResolvedDeps, currentShrinkwrap, opts)
  }

  return {
    linkedPkgsMap: pkgsToLink,
    wantedShrinkwrap: newShr,
    currentShrinkwrap,
    newDepPaths,
    removedPkgIds,
    hoistedAliases: opts.hoistedAliases,
  }
}

async function shamefullyFlattenTree(
  flatResolvedDeps: DependencyTreeNode[],
  currentShrinkwrap: Shrinkwrap,
  opts: {
    force: boolean,
    dryRun: boolean,
    baseNodeModules: string,
    bin: string,
    pkg: PackageJson,
    outdatedPkgs: {[pkgId: string]: string},
  },
): Promise<{[alias: string]: string[]}> {
  const pkgIdByAlias = {}
  const aliasByPkgId: {[pkgId: string]: string[]} = {}

  await Promise.all(flatResolvedDeps
    // sort by depth and then alphabetically
    .sort((a, b) => {
      const depthDiff = a.depth - b.depth
      return depthDiff === 0 ? a.name.localeCompare(b.name) : depthDiff
    })
    // build the alias map and the id map
    .map(pkg => {
      for (let childAlias of R.keys(pkg.children)) {
        // if this alias is in the root dependencies, skip it
        if (currentShrinkwrap.specifiers[childAlias]) {
          continue
        }
        // if this alias has already been taken, skip it
        if (pkgIdByAlias[childAlias]) {
          continue
        }
        const childId = pkg.children[childAlias]
        pkgIdByAlias[childAlias] = childId
        if (!aliasByPkgId[childId]) {
          aliasByPkgId[childId] = []
        }
        aliasByPkgId[childId].push(childAlias)
      }
      return pkg
    })
    .map(async pkg => {
      const pkgAliases = aliasByPkgId[pkg.id]
      if (!pkgAliases) {
        return
      }
      // TODO when putting logs back in for hoisted packages, you've to put back the condition inside the map,
      // TODO look how it is done in linkPackages
      if (!opts.dryRun) {
        await Promise.all(pkgAliases.map(async pkgAlias => {
          await symlinkDependencyTo(pkgAlias, pkg, opts.baseNodeModules)
        }))
      }
    }))

  return aliasByPkgId
}

function filterShrinkwrap (
  shr: Shrinkwrap,
  opts: {
    noProd: boolean,
    noDev: boolean,
    noOptional: boolean,
    skipped: Set<string>,
  }
): Shrinkwrap {
  let pairs = R.toPairs<string, DependencyShrinkwrap>(shr.packages || {})
    .filter(pair => !opts.skipped.has(pair[1].id || dp.resolve(shr.registry, pair[0])))
  if (opts.noProd) {
    pairs = pairs.filter(pair => pair[1].dev !== false || pair[1].optional)
  }
  if (opts.noDev) {
    pairs = pairs.filter(pair => pair[1].dev !== true)
  }
  if (opts.noOptional) {
    pairs = pairs.filter(pair => !pair[1].optional)
  }
  return {
    shrinkwrapVersion: shr.shrinkwrapVersion,
    registry: shr.registry,
    specifiers: shr.specifiers,
    packages: R.fromPairs(pairs),
    dependencies: opts.noProd ? {} : shr.dependencies || {},
    devDependencies: opts.noDev ? {} : shr.devDependencies || {},
    optionalDependencies: opts.noOptional ? {} : shr.optionalDependencies || {},
  } as Shrinkwrap
}

async function linkNewPackages (
  currentShrinkwrap: Shrinkwrap,
  wantedShrinkwrap: Shrinkwrap,
  pkgsToLink: DependencyTreeNodeMap,
  opts: {
    dryRun: boolean,
    force: boolean,
    global: boolean,
    baseNodeModules: string,
    optional: boolean,
    storeController: StoreController,
    sideEffectsCache: boolean,
  }
): Promise<string[]> {
  const wantedRelDepPaths = R.keys(wantedShrinkwrap.packages)
  const prevRelDepPaths = R.keys(currentShrinkwrap.packages)

  // TODO: what if the registries differ?
  const newDepPathsSet = new Set(
    (
      opts.force
        ? wantedRelDepPaths
        : R.difference(wantedRelDepPaths, prevRelDepPaths)
    )
    .map(relDepPath => dp.resolve(wantedShrinkwrap.registry, relDepPath))
    // when installing a new package, not all the nodes are analyzed
    // just skip the ones that are in the lockfile but were not analyzed
    .filter(depPath => pkgsToLink[depPath])
  )
  statsLogger.debug({added: newDepPathsSet.size})

  const existingWithUpdatedDeps = []
  if (!opts.force && currentShrinkwrap.packages && wantedShrinkwrap.packages) {
    // add subdependencies that have been updated
    // TODO: no need to relink everything. Can be relinked only what was changed
    for (const relDepPath of wantedRelDepPaths) {
      if (currentShrinkwrap.packages[relDepPath] &&
        (!R.equals(currentShrinkwrap.packages[relDepPath].dependencies, wantedShrinkwrap.packages[relDepPath].dependencies) ||
        !R.equals(currentShrinkwrap.packages[relDepPath].optionalDependencies, wantedShrinkwrap.packages[relDepPath].optionalDependencies))) {
        const depPath = dp.resolve(wantedShrinkwrap.registry, relDepPath)

        // TODO: come up with a test that triggers the usecase of pkgsToLink[depPath] undefined
        // see related issue: https://github.com/pnpm/pnpm/issues/870
        if (pkgsToLink[depPath] && !newDepPathsSet.has(depPath)) {
          existingWithUpdatedDeps.push(pkgsToLink[depPath])
        }
      }
    }
  }

  if (!newDepPathsSet.size && !existingWithUpdatedDeps.length) return []

  const newDepPaths = Array.from(newDepPathsSet)

  if (opts.dryRun) return newDepPaths

  const newPkgs = R.props<string, DependencyTreeNode>(newDepPaths, pkgsToLink)

  await Promise.all([
    linkAllModules(newPkgs, pkgsToLink, {optional: opts.optional}),
    linkAllModules(existingWithUpdatedDeps, pkgsToLink, {optional: opts.optional}),
    linkAllPkgs(opts.storeController, newPkgs, opts),
  ])

  await linkAllBins(newPkgs, pkgsToLink, {optional: opts.optional})

  return newDepPaths
}

const limitLinking = pLimit(16)

async function linkAllPkgs (
  storeController: StoreController,
  alldeps: DependencyTreeNode[],
  opts: {
    force: boolean,
    sideEffectsCache: boolean,
  }
) {
  return Promise.all(
    alldeps.map(async pkg => {
      const filesResponse = await pkg.fetchingFiles

      if (pkg.independent) return
      return storeController.importPackage(pkg.centralLocation, pkg.peripheralLocation, {
        force: opts.force,
        filesResponse,
      })
    })
  )
}

async function linkAllBins (
  pkgs: DependencyTreeNode[],
  pkgMap: DependencyTreeNodeMap,
  opts: {
    optional: boolean,
  }
) {
  return Promise.all(
    pkgs.map(dependency => limitLinking(async () => {
      const binPath = path.join(dependency.peripheralLocation, 'node_modules', '.bin')

      const childrenToLink = opts.optional
          ? dependency.children
          : R.keys(dependency.children)
            .reduce((nonOptionalChildren, childAlias) => {
              if (!dependency.optionalDependencies.has(childAlias)) {
                nonOptionalChildren[childAlias] = dependency.children[childAlias]
              }
              return nonOptionalChildren
            }, {})

      await Promise.all(
        R.keys(childrenToLink)
          .map(async alias => {
            const childToLink = childrenToLink[alias]
            const child = pkgMap[childToLink]
            if (child.installable) {
              await linkPkgBins(path.join(dependency.modules, alias), binPath)
            }
          })
      )

      // link also the bundled dependencies` bins
      if (dependency.hasBundledDependencies) {
        const bundledModules = path.join(dependency.peripheralLocation, 'node_modules')
        await linkBins(bundledModules, binPath)
      }
    }))
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
    pkgs
      .filter(dependency => !dependency.independent)
      .map(dependency => limitLinking(async () => {
        const childrenToLink = opts.optional
          ? dependency.children
          : R.keys(dependency.children)
            .reduce((nonOptionalChildren, childAlias) => {
              if (!dependency.optionalDependencies.has(childAlias)) {
                nonOptionalChildren[childAlias] = dependency.children[childAlias]
              }
              return nonOptionalChildren
            }, {})

        await Promise.all(
          R.keys(childrenToLink)
            .map(async alias => {
              const pkg = pkgMap[childrenToLink[alias]]
              if (!pkg.installable) return
              await symlinkDependencyTo(alias, pkg, dependency.modules)
            })
        )
      }))
  )
}

function symlinkDependencyTo (alias: string, dependency: DependencyTreeNode, dest: string) {
  dest = path.join(dest, alias)
  return symlinkDir(dependency.peripheralLocation, dest)
}
