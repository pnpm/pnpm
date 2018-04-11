import linkBins, {linkPackageBins} from '@pnpm/link-bins'
import {PackageJson} from '@pnpm/types'
import * as dp from 'dependency-path'
import pLimit = require('p-limit')
import {StoreController} from 'package-store'
import path = require('path')
import {PackageSnapshot, Shrinkwrap} from 'pnpm-shrinkwrap'
import R = require('ramda')
import symlinkDir = require('symlink-dir')
import {PkgGraphNodeByNodeId} from '../api/install'
import removeOrphanPkgs from '../api/removeOrphanPkgs'
import {SHRINKWRAP_MINOR_VERSION} from '../constants'
import {
  rootLogger,
  stageLogger,
  statsLogger,
} from '../loggers'
import logStatus from '../logging/logInstallStatus'
import resolvePeers, {DepGraphNode, DepGraphNodesByDepPath} from './resolvePeers'
import updateShrinkwrap from './updateShrinkwrap'

export {DepGraphNodesByDepPath}

export default async function linkPackages (
  rootNodeIdsByAlias: {[alias: string]: string},
  pkgGraph: PkgGraphNodeByNodeId,
  opts: {
    force: boolean,
    dryRun: boolean,
    global: boolean,
    baseNodeModules: string,
    bin: string,
    topParents: Array<{name: string, version: string}>,
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
    hoistedAliases: {[depPath: string]: string[]},
  },
): Promise<{
  currentShrinkwrap: Shrinkwrap,
  depGraph: DepGraphNodesByDepPath,
  hoistedAliases: {[depPath: string]: string[]},
  newDepPaths: string[],
  removedDepPaths: Set<string>,
  wantedShrinkwrap: Shrinkwrap,
}> {
  // TODO: decide what kind of logging should be here.
  // The `Creating dependency graph` is not good to report in all cases as
  // sometimes node_modules is alread up-to-date
  // logger.info(`Creating dependency graph`)
  const resolvePeersResult = await resolvePeers(pkgGraph, rootNodeIdsByAlias, opts.topParents, opts.independentLeaves, opts.baseNodeModules)
  const depGraph = resolvePeersResult.depGraph
  const newShr = updateShrinkwrap(depGraph, opts.wantedShrinkwrap, opts.pkg)

  const removedDepPaths = await removeOrphanPkgs({
    bin: opts.bin,
    dryRun: opts.dryRun,
    hoistedAliases: opts.hoistedAliases,
    newShrinkwrap: newShr,
    oldShrinkwrap: opts.currentShrinkwrap,
    prefix: opts.root,
    shamefullyFlatten: opts.shamefullyFlatten,
    storeController: opts.storeController,
  })

  let depNodes =  R.values(depGraph).filter((depNode) => !opts.skipped.has(depNode.id))
  if (!opts.production) {
    depNodes = depNodes.filter((depNode) => depNode.dev !== false || depNode.optional)
  }
  if (!opts.development) {
    depNodes = depNodes.filter((depNode) => depNode.dev !== true)
  }
  if (!opts.optional) {
    depNodes = depNodes.filter((depNode) => !depNode.optional)
  }

  const filterOpts = {
    noDev: !opts.development,
    noOptional: !opts.optional,
    noProd: !opts.production,
    skipped: opts.skipped,
  }
  const newCurrentShrinkwrap = filterShrinkwrap(newShr, filterOpts)
  stageLogger.debug('importing_started')
  const newDepPaths = await linkNewPackages(
    filterShrinkwrap(opts.currentShrinkwrap, filterOpts),
    newCurrentShrinkwrap,
    depGraph,
    opts,
  )
  stageLogger.debug('importing_done')

  const rootDepsByDepPath = depNodes
    .filter((depNode) => depNode.depth === 0)
    .reduce((acc, depNode) => {
      acc[depNode.absolutePath] = depNode
      return acc
    }, {})
  for (const rootAlias of R.keys(resolvePeersResult.rootAbsolutePathsByAlias)) {
    const pkg = rootDepsByDepPath[resolvePeersResult.rootAbsolutePathsByAlias[rootAlias]]
    if (!pkg) continue
    if (opts.dryRun || !(await symlinkDependencyTo(rootAlias, pkg, opts.baseNodeModules)).reused) {
      const isDev = opts.pkg.devDependencies && opts.pkg.devDependencies[pkg.name]
      const isOptional = opts.pkg.optionalDependencies && opts.pkg.optionalDependencies[pkg.name]
      rootLogger.info({
        added: {
          dependencyType: isDev && 'dev' || isOptional && 'optional' || 'prod',
          id: pkg.id,
          latest: opts.outdatedPkgs[pkg.id],
          name: rootAlias,
          realName: pkg.name,
          version: pkg.version,
        },
      })
    }
  }

  if (!opts.dryRun) {
    await linkBins(opts.baseNodeModules, opts.bin)
  }

  if (opts.updateShrinkwrapMinorVersion) {
    // Setting `shrinkwrapMinorVersion` is a temporary solution to
    // have new backward-compatible versions of `shrinkwrap.yaml`
    // w/o changing `shrinkwrapVersion`. From version 4, the
    // `shrinkwrapVersion` field allows numbers like 4.1
    newShr.shrinkwrapMinorVersion = SHRINKWRAP_MINOR_VERSION
  }
  let currentShrinkwrap: Shrinkwrap
  if (opts.makePartialCurrentShrinkwrap) {
    const packages = opts.currentShrinkwrap.packages || {}
    if (newShr.packages) {
      for (const relDepPath in newShr.packages) { // tslint:disable-line:forin
        const depPath = dp.resolve(newShr.registry, relDepPath)
        if (depGraph[depPath]) {
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

  // Important: shamefullyFlattenGraph changes depGraph, so keep this at the end
  if (opts.shamefullyFlatten && (opts.reinstallForFlatten || newDepPaths.length > 0 || removedDepPaths.size > 0)) {
    opts.hoistedAliases = await shamefullyFlattenGraph(depNodes, currentShrinkwrap, opts)
  }

  return {
    currentShrinkwrap,
    depGraph,
    hoistedAliases: opts.hoistedAliases,
    newDepPaths,
    removedDepPaths,
    wantedShrinkwrap: newShr,
  }
}

async function shamefullyFlattenGraph (
  depNodes: DepGraphNode[],
  currentShrinkwrap: Shrinkwrap,
  opts: {
    baseNodeModules: string,
    dryRun: boolean,
  },
): Promise<{[alias: string]: string[]}> {
  const dependencyPathByAlias = {}
  const aliasesByDependencyPath: {[depPath: string]: string[]} = {}

  await Promise.all(depNodes
    // sort by depth and then alphabetically
    .sort((a, b) => {
      const depthDiff = a.depth - b.depth
      return depthDiff === 0 ? a.name.localeCompare(b.name) : depthDiff
    })
    // build the alias map and the id map
    .map((depNode) => {
      for (const childAlias of R.keys(depNode.children)) {
        // if this alias is in the root dependencies, skip it
        if (currentShrinkwrap.specifiers[childAlias]) {
          continue
        }
        // if this alias has already been taken, skip it
        if (dependencyPathByAlias[childAlias]) {
          continue
        }
        const childPath = depNode.children[childAlias]
        dependencyPathByAlias[childAlias] = childPath
        if (!aliasesByDependencyPath[childPath]) {
          aliasesByDependencyPath[childPath] = []
        }
        aliasesByDependencyPath[childPath].push(childAlias)
      }
      return depNode
    })
    .map(async (depNode) => {
      const pkgAliases = aliasesByDependencyPath[depNode.absolutePath]
      if (!pkgAliases) {
        return
      }
      // TODO when putting logs back in for hoisted packages, you've to put back the condition inside the map,
      // TODO look how it is done in linkPackages
      if (!opts.dryRun) {
        await Promise.all(pkgAliases.map(async (pkgAlias) => {
          await symlinkDependencyTo(pkgAlias, depNode, opts.baseNodeModules)
        }))
      }
    }))

  return aliasesByDependencyPath
}

function filterShrinkwrap (
  shr: Shrinkwrap,
  opts: {
    noDev: boolean,
    noOptional: boolean,
    noProd: boolean,
    skipped: Set<string>,
  },
): Shrinkwrap {
  let pairs = (R.toPairs(shr.packages || {}) as Array<[string, PackageSnapshot]>)
    .filter((pair) => !opts.skipped.has(pair[1].id || dp.resolve(shr.registry, pair[0])))
  if (opts.noProd) {
    pairs = pairs.filter((pair) => pair[1].dev !== false || pair[1].optional)
  }
  if (opts.noDev) {
    pairs = pairs.filter((pair) => pair[1].dev !== true)
  }
  if (opts.noOptional) {
    pairs = pairs.filter((pair) => !pair[1].optional)
  }
  return {
    dependencies: opts.noProd ? {} : shr.dependencies || {},
    devDependencies: opts.noDev ? {} : shr.devDependencies || {},
    optionalDependencies: opts.noOptional ? {} : shr.optionalDependencies || {},
    packages: R.fromPairs(pairs),
    registry: shr.registry,
    shrinkwrapVersion: shr.shrinkwrapVersion,
    specifiers: shr.specifiers,
  } as Shrinkwrap
}

async function linkNewPackages (
  currentShrinkwrap: Shrinkwrap,
  wantedShrinkwrap: Shrinkwrap,
  depGraph: DepGraphNodesByDepPath,
  opts: {
    baseNodeModules: string,
    dryRun: boolean,
    force: boolean,
    global: boolean,
    optional: boolean,
    sideEffectsCache: boolean,
    storeController: StoreController,
    root: string,
  },
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
    .map((relDepPath) => dp.resolve(wantedShrinkwrap.registry, relDepPath))
    // when installing a new package, not all the nodes are analyzed
    // just skip the ones that are in the lockfile but were not analyzed
    .filter((depPath) => depGraph[depPath]),
  )
  statsLogger.debug({
    added: newDepPathsSet.size,
    prefix: opts.root,
  })

  const existingWithUpdatedDeps = []
  if (!opts.force && currentShrinkwrap.packages && wantedShrinkwrap.packages) {
    // add subdependencies that have been updated
    // TODO: no need to relink everything. Can be relinked only what was changed
    for (const relDepPath of wantedRelDepPaths) {
      if (currentShrinkwrap.packages[relDepPath] &&
        (!R.equals(currentShrinkwrap.packages[relDepPath].dependencies, wantedShrinkwrap.packages[relDepPath].dependencies) ||
        !R.equals(currentShrinkwrap.packages[relDepPath].optionalDependencies, wantedShrinkwrap.packages[relDepPath].optionalDependencies))) {
        const depPath = dp.resolve(wantedShrinkwrap.registry, relDepPath)

        // TODO: come up with a test that triggers the usecase of depGraph[depPath] undefined
        // see related issue: https://github.com/pnpm/pnpm/issues/870
        if (depGraph[depPath] && !newDepPathsSet.has(depPath)) {
          existingWithUpdatedDeps.push(depGraph[depPath])
        }
      }
    }
  }

  if (!newDepPathsSet.size && !existingWithUpdatedDeps.length) return []

  const newDepPaths = Array.from(newDepPathsSet)

  if (opts.dryRun) return newDepPaths

  const newPkgs = R.props<string, DepGraphNode>(newDepPaths, depGraph)

  await Promise.all([
    linkAllModules(newPkgs, depGraph, {optional: opts.optional}),
    linkAllModules(existingWithUpdatedDeps, depGraph, {optional: opts.optional}),
    linkAllPkgs(opts.storeController, newPkgs, opts),
  ])

  await linkAllBins(newPkgs, depGraph, {optional: opts.optional})

  return newDepPaths
}

const limitLinking = pLimit(16)

async function linkAllPkgs (
  storeController: StoreController,
  depNodes: DepGraphNode[],
  opts: {
    force: boolean,
    sideEffectsCache: boolean,
  },
) {
  return Promise.all(
    depNodes.map(async (depNode) => {
      const filesResponse = await depNode.fetchingFiles
      if (!depNode.requiresBuild) {
        depNode.requiresBuild = Boolean(filesResponse.filenames.indexOf('binding.gyp') !== -1 ||
          filesResponse.filenames.some((filename) => !!filename.match(/^[.]hooks[\\/]/))) // TODO: optimize this
      }

      if (depNode.independent) return
      return storeController.importPackage(depNode.centralLocation, depNode.peripheralLocation, {
        filesResponse,
        force: opts.force,
      })
    }),
  )
}

async function linkAllBins (
  depNodes: DepGraphNode[],
  depGraph: DepGraphNodesByDepPath,
  opts: {
    optional: boolean,
  },
) {
  return Promise.all(
    depNodes.map((depNode) => limitLinking(async () => {
      const binPath = path.join(depNode.peripheralLocation, 'node_modules', '.bin')

      const childrenToLink = opts.optional
          ? depNode.children
          : R.keys(depNode.children)
            .reduce((nonOptionalChildren, childAlias) => {
              if (!depNode.optionalDependencies.has(childAlias)) {
                nonOptionalChildren[childAlias] = depNode.children[childAlias]
              }
              return nonOptionalChildren
            }, {})

      await Promise.all(
        R.keys(childrenToLink)
          .filter((alias) => depGraph[childrenToLink[alias]].installable)
          .map((alias) => path.join(depNode.modules, alias))
          .map((target) => linkPackageBins(target, binPath)),
      )

      // link also the bundled dependencies` bins
      if (depNode.hasBundledDependencies) {
        const bundledModules = path.join(depNode.peripheralLocation, 'node_modules')
        await linkBins(bundledModules, binPath)
      }
    })),
  )
}

async function linkAllModules (
  depNodes: DepGraphNode[],
  depGraph: DepGraphNodesByDepPath,
  opts: {
    optional: boolean,
  },
) {
  return Promise.all(
    depNodes
      .filter((depNode) => !depNode.independent)
      .map((depNode) => limitLinking(async () => {
        const childrenToLink = opts.optional
          ? depNode.children
          : R.keys(depNode.children)
            .reduce((nonOptionalChildren, childAlias) => {
              if (!depNode.optionalDependencies.has(childAlias)) {
                nonOptionalChildren[childAlias] = depNode.children[childAlias]
              }
              return nonOptionalChildren
            }, {})

        await Promise.all(
          R.keys(childrenToLink)
            .map(async (alias) => {
              const pkg = depGraph[childrenToLink[alias]]
              if (!pkg.installable) return
              await symlinkDependencyTo(alias, pkg, depNode.modules)
            }),
        )
      })),
  )
}

function symlinkDependencyTo (alias: string, depNode: DepGraphNode, dest: string) {
  dest = path.join(dest, alias)
  return symlinkDir(depNode.peripheralLocation, dest)
}
