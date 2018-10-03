import {
  linkLogger,
  packageJsonLogger,
  progressLogger,
  rootLogger,
  stageLogger,
  statsLogger,
  summaryLogger,
} from '@pnpm/core-loggers'
import runLifecycleHooks from '@pnpm/lifecycle'
import linkBins, { linkBinsOfPackages } from '@pnpm/link-bins'
import logger, {
  LogBase,
  streamParser,
} from '@pnpm/logger'
import { prune } from '@pnpm/modules-cleaner'
import {
  IncludedDependencies,
  read as readModulesYaml,
  write as writeModulesYaml,
} from '@pnpm/modules-yaml'
import {
  getCacheByEngine,
  PackageFilesResponse,
} from '@pnpm/package-requester'
import pkgIdToFilename from '@pnpm/pkgid-to-filename'
import { fromDir as readPackageFromDir } from '@pnpm/read-package-json'
import { PackageJson } from '@pnpm/types'
import { realNodeModulesDir } from '@pnpm/utils'
import dp = require('dependency-path')
import pLimit = require('p-limit')
import { StoreController } from 'package-store'
import path = require('path')
import {
  getImporterPath,
  nameVerFromPkgSnapshot,
  PackageSnapshot,
  pkgSnapshotToResolution,
  readCurrent,
  readWanted,
  satisfiesPackageJson,
  Shrinkwrap,
  writeCurrentOnly as writeCurrentShrinkwrapOnly,
} from 'pnpm-shrinkwrap'
import R = require('ramda')
import symlinkDir = require('symlink-dir')
import {
  ENGINE_NAME,
  LAYOUT_VERSION,
} from './constants'
import runDependenciesScripts from './runDependenciesScripts'

export type ReporterFunction = (logObj: LogBase) => void

export interface HeadlessOptions {
  childConcurrency?: number,
  currentShrinkwrap?: Shrinkwrap,
  prefix: string,
  ignoreScripts: boolean,
  include: IncludedDependencies,
  independentLeaves: boolean,
  importerPath?: string,
  shrinkwrapDirectory?: string,
  storeController: StoreController,
  verifyStoreIntegrity: boolean,
  sideEffectsCache: boolean,
  sideEffectsCacheReadonly: boolean,
  force: boolean,
  store: string,
  rawNpmConfig: object,
  unsafePerm: boolean,
  userAgent: string,
  reporter?: ReporterFunction,
  packageJson?: PackageJson,
  packageManager: {
    name: string,
    version: string,
  },
  pruneStore: boolean,
  wantedShrinkwrap?: Shrinkwrap,
  ownLifecycleHooksStdio?: 'inherit' | 'pipe',
}

export default async (opts: HeadlessOptions) => {
  const reporter = opts.reporter
  if (reporter) {
    streamParser.on('data', reporter)
  }

  if (typeof opts.prefix !== 'string') {
    throw new TypeError('opts.prefix should be a string')
  }

  const shrinkwrapDirectory = opts.shrinkwrapDirectory || opts.prefix
  const wantedShrinkwrap = opts.wantedShrinkwrap || await readWanted(shrinkwrapDirectory, {ignoreIncompatible: false})

  if (!wantedShrinkwrap) {
    throw new Error('Headless installation requires a shrinkwrap.yaml file')
  }

  const currentShrinkwrap = opts.currentShrinkwrap || await readCurrent(shrinkwrapDirectory, {ignoreIncompatible: false})
  const importerPath = getImporterPath(shrinkwrapDirectory, opts.prefix)
  const virtualStoreDir = await realNodeModulesDir(shrinkwrapDirectory)
  const importerModulesDir = await realNodeModulesDir(opts.prefix)
  const modules = await readModulesYaml(importerModulesDir) ||
    virtualStoreDir !== importerModulesDir && await readModulesYaml(virtualStoreDir) ||
    {
      importers: {
        [importerPath]: {
          hoistedAliases: {},
          shamefullyFlatten: false, // shamefully flatten is not supported yet by headless install
        },
      },
      pendingBuilds: [] as string[],
    }

  const pkg = opts.packageJson || await readPackageFromDir(opts.prefix)

  if (!satisfiesPackageJson(wantedShrinkwrap, pkg, importerPath)) {
    throw new Error('Cannot run headless installation because shrinkwrap.yaml is not up-to-date with package.json')
  }

  packageJsonLogger.debug({ initial: pkg, prefix: opts.prefix })

  const scripts = !opts.ignoreScripts && pkg.scripts || {}

  const bin = path.join(importerModulesDir, '.bin')

  const scriptsOpts = {
    depPath: opts.prefix,
    pkgRoot: opts.prefix,
    rawNpmConfig: opts.rawNpmConfig,
    rootNodeModulesDir: importerModulesDir,
    stdio: opts.ownLifecycleHooksStdio || 'inherit',
    unsafePerm: opts.unsafePerm || false,
  }

  if (scripts.preinstall) {
    await runLifecycleHooks('preinstall', pkg, scriptsOpts)
  }

  const filterOpts = {
    importerPath,
    include: opts.include,
  }
  const filteredShrinkwrap = filterShrinkwrap(wantedShrinkwrap, filterOpts)

  if (currentShrinkwrap) {
    await prune({
      dryRun: false,
      importers: {
        [importerPath]: {
          bin,
          hoistedAliases: modules && modules.importers[importerPath] && modules.importers[importerPath].hoistedAliases || {},
          importerModulesDir,
          prefix: opts.prefix,
          shamefullyFlatten: false,
        },
      },
      newShrinkwrap: filteredShrinkwrap,
      oldShrinkwrap: currentShrinkwrap,
      pruneStore: opts.pruneStore,
      storeController: opts.storeController,
      virtualStoreDir,
    })
  } else {
    statsLogger.debug({
      prefix: opts.prefix,
      removed: 0,
    })
  }

  stageLogger.debug('importing_started')
  const res = await shrinkwrapToDepGraph(
    filteredShrinkwrap,
    opts.force ? null : currentShrinkwrap,
    {
      ...opts,
      importerPath,
      virtualStoreDir,
    } as ShrinkwrapToDepGraphOptions,
  )
  const depGraph = res.graph

  statsLogger.debug({
    added: Object.keys(depGraph).length,
    prefix: opts.prefix,
  })

  await Promise.all([
    linkAllModules(depGraph, {optional: opts.include.optionalDependencies}),
    linkAllPkgs(opts.storeController, R.values(depGraph), opts),
  ])
  stageLogger.debug('importing_done')

  function warn (message: string) {
    logger.warn({
      message,
      prefix: opts.prefix,
    })
  }

  await linkAllBins(depGraph, {optional: opts.include.optionalDependencies, warn})

  await linkRootPackages(filteredShrinkwrap, opts.prefix, res.rootDependencies, importerModulesDir, importerPath)
  await linkBins(importerModulesDir, bin, {warn})

  await writeCurrentShrinkwrapOnly(shrinkwrapDirectory, filteredShrinkwrap)
  if (opts.ignoreScripts) {
    // we can use concat here because we always only append new packages, which are guaranteed to not be there by definition
    modules.pendingBuilds = modules.pendingBuilds
      .concat(
        R.values(depGraph)
          .filter((node) => node.requiresBuild)
          .map((node) => node.relDepPath),
      )
  }
  await writeModulesYaml(virtualStoreDir, {
    ...modules,
    included: opts.include,
    independentLeaves: !!opts.independentLeaves,
    layoutVersion: LAYOUT_VERSION,
    packageManager: `${opts.packageManager.name}@${opts.packageManager.version}`,
    pendingBuilds: modules.pendingBuilds,
    skipped: [],
    store: opts.store,
  })

  if (!opts.ignoreScripts) {
    await runDependenciesScripts(depGraph, R.values(res.rootDependencies).filter((loc) => depGraph[loc]), {
      childConcurrency: opts.childConcurrency,
      prefix: opts.prefix,
      rawNpmConfig: opts.rawNpmConfig,
      rootNodeModulesDir: importerModulesDir,
      sideEffectsCache: opts.sideEffectsCache,
      sideEffectsCacheReadonly: opts.sideEffectsCacheReadonly,
      storeController: opts.storeController,
      unsafePerm: opts.unsafePerm,
      userAgent: opts.userAgent,
    })
  }

  // waiting till package requests are finished
  await Promise.all(R.values(depGraph).map((depNode) => depNode.finishing))

  summaryLogger.debug({prefix: opts.prefix})

  await opts.storeController.close()

  if (scripts.install) {
    await runLifecycleHooks('install', pkg, scriptsOpts)
  }
  if (scripts.postinstall) {
    await runLifecycleHooks('postinstall', pkg, scriptsOpts)
  }
  if (scripts.prepublish) {
    await runLifecycleHooks('prepublish', pkg, scriptsOpts)
  }
  if (scripts.prepare) {
    await runLifecycleHooks('prepare', pkg, scriptsOpts)
  }

  if (reporter) {
    streamParser.removeListener('data', reporter)
  }
}

async function linkRootPackages (
  shr: Shrinkwrap,
  prefix: string,
  rootDependencies: {[alias: string]: string},
  importerModulesDir: string,
  importerPath: string,
) {
  const shrImporter = shr.importers[importerPath]
  const allDeps = {
    ...shrImporter.devDependencies,
    ...shrImporter.dependencies,
    ...shrImporter.optionalDependencies,
  }
  return Promise.all(
    R.keys(allDeps)
      .map(async (alias) => {
        const depPath = dp.refToAbsolute(allDeps[alias], alias, shr.registry)
        const peripheralLocation = rootDependencies[alias]
        // Skipping linked packages
        if (!peripheralLocation) {
          return
        }
        if ((await symlinkDependencyTo(alias, peripheralLocation, importerModulesDir)).reused) {
          return
        }
        const isDev = shrImporter.devDependencies && shrImporter.devDependencies[alias]
        const isOptional = shrImporter.optionalDependencies && shrImporter.optionalDependencies[alias]

        const relDepPath = dp.refToRelative(allDeps[alias], alias)
        if (relDepPath === null) return
        const pkgSnapshot = shr.packages && shr.packages[relDepPath]
        if (!pkgSnapshot) return // this won't ever happen. Just making typescript happy
        const pkgId = pkgSnapshot.id || depPath
        const pkgInfo = nameVerFromPkgSnapshot(relDepPath, pkgSnapshot)
        rootLogger.debug({
          added: {
            dependencyType: isDev && 'dev' || isOptional && 'optional' || 'prod',
            id: pkgId,
            // latest: opts.outdatedPkgs[pkg.id],
            name: alias,
            realName: pkgInfo.name,
            version: pkgInfo.version,
          },
          prefix,
        })
      }),
  )
}

interface ShrinkwrapToDepGraphOptions {
  force: boolean,
  independentLeaves: boolean,
  importerPath: string,
  storeController: StoreController,
  store: string,
  prefix: string,
  verifyStoreIntegrity: boolean,
  virtualStoreDir: string,
}

async function shrinkwrapToDepGraph (
  shr: Shrinkwrap,
  currentShrinkwrap: Shrinkwrap | null,
  opts: ShrinkwrapToDepGraphOptions,
) {
  const currentPackages = currentShrinkwrap && currentShrinkwrap.packages || {}
  const graph: DependenciesGraph = {}
  let rootDependencies: {[alias: string]: string} = {}
  if (shr.packages) {
    const pkgSnapshotByLocation = {}
    for (const relDepPath of R.keys(shr.packages)) {
      if (currentPackages[relDepPath] && R.equals(currentPackages[relDepPath].dependencies, shr.packages[relDepPath].dependencies) &&
        R.equals(currentPackages[relDepPath].optionalDependencies, shr.packages[relDepPath].optionalDependencies)) {
          continue
        }
      const depPath = dp.resolve(shr.registry, relDepPath)
      const pkgSnapshot = shr.packages[relDepPath]
      const independent = opts.independentLeaves && pkgIsIndependent(pkgSnapshot)
      const resolution = pkgSnapshotToResolution(relDepPath, pkgSnapshot, shr.registry)
      // TODO: optimize. This info can be already returned by pkgSnapshotToResolution()
      const pkgName = nameVerFromPkgSnapshot(relDepPath, pkgSnapshot).name
      const pkgId = pkgSnapshot.id || depPath
      progressLogger.debug({
        pkgId,
        status: 'resolving_content',
      })
      let fetchResponse = opts.storeController.fetchPackage({
        force: false,
        pkgId,
        prefix: opts.prefix,
        resolution,
        verifyStoreIntegrity: opts.verifyStoreIntegrity,
      })
      if (fetchResponse instanceof Promise) fetchResponse = await fetchResponse
      fetchResponse.fetchingFiles
        .then((fetchResult) => {
          progressLogger.debug({
            pkgId,
            status: fetchResult.fromStore
              ? 'found_in_store' : 'fetched',
          })
        })
      const cache = !opts.force && await getCache(opts.store, pkgId)
      const centralLocation = cache || path.join(fetchResponse.inStoreLocation, 'node_modules', pkgName)

      // NOTE: This code will not convert the depPath with peer deps correctly
      // Unfortunately, there is currently no way to tell if the last dir in the path is originally there or added to separate
      // the diferent peer dependency sets
      const modules = path.join(opts.virtualStoreDir, `.${pkgIdToFilename(depPath, opts.prefix)}`, 'node_modules')
      const peripheralLocation = !independent
        ? path.join(modules, pkgName)
        : centralLocation
      graph[peripheralLocation] = {
        centralLocation,
        children: {},
        fetchingFiles: fetchResponse.fetchingFiles,
        finishing: fetchResponse.finishing,
        hasBin: pkgSnapshot.hasBin === true,
        hasBundledDependencies: !!pkgSnapshot.bundledDependencies,
        independent,
        isBuilt: !!cache,
        modules,
        optional: !!pkgSnapshot.optional,
        optionalDependencies: new Set(R.keys(pkgSnapshot.optionalDependencies)),
        peripheralLocation,
        pkgId,
        prepare: pkgSnapshot.prepare === true,
        relDepPath: depPath,
        requiresBuild: pkgSnapshot.requiresBuild === true,
      }
      pkgSnapshotByLocation[peripheralLocation] = pkgSnapshot
    }
    const ctx = {
      force: opts.force,
      graph,
      independentLeaves: opts.independentLeaves,
      pkgSnapshotsByRelDepPaths: shr.packages,
      prefix: opts.prefix,
      registry: shr.registry,
      store: opts.store,
      virtualStoreDir: opts.virtualStoreDir,
    }
    for (const peripheralLocation of R.keys(graph)) {
      const pkgSnapshot = pkgSnapshotByLocation[peripheralLocation]
      const allDeps = {...pkgSnapshot.dependencies, ...pkgSnapshot.optionalDependencies}

      graph[peripheralLocation].children = await getChildrenPaths(ctx, allDeps)
    }
    const shrImporter = shr.importers[opts.importerPath]
    const rootDeps = {...shrImporter.devDependencies, ...shrImporter.dependencies, ...shrImporter.optionalDependencies}
    rootDependencies = await getChildrenPaths(ctx, rootDeps)
  }
  return {graph, rootDependencies}
}

async function getChildrenPaths (
  ctx: {
    graph: DependenciesGraph,
    force: boolean,
    registry: string,
    virtualStoreDir: string,
    independentLeaves: boolean,
    store: string,
    pkgSnapshotsByRelDepPaths: {[relDepPath: string]: PackageSnapshot},
    prefix: string,
  },
  allDeps: {[alias: string]: string},
) {
  const children: {[alias: string]: string} = {}
  for (const alias of R.keys(allDeps)) {
    const childDepPath = dp.refToAbsolute(allDeps[alias], alias, ctx.registry)
    if (childDepPath === null) {
      children[alias] = path.resolve(ctx.prefix, allDeps[alias].substr(5))
      continue
    }
    const childRelDepPath = dp.relative(ctx.registry, childDepPath)
    const childPkgSnapshot = ctx.pkgSnapshotsByRelDepPaths[childRelDepPath]
    if (ctx.graph[childDepPath]) {
      children[alias] = ctx.graph[childDepPath].peripheralLocation
    } else if (ctx.independentLeaves && pkgIsIndependent(childPkgSnapshot)) {
      const pkgId = childPkgSnapshot.id || childDepPath
      const cache = !ctx.force && await getCache(ctx.store, pkgId)
      const pkgName = nameVerFromPkgSnapshot(childRelDepPath, childPkgSnapshot).name
      const inStoreLocation = pkgIdToFilename(pkgId, ctx.prefix)
      children[alias] = cache || path.join(inStoreLocation, 'node_modules', pkgName)
    } else if (childPkgSnapshot) {
      const relDepPath = dp.relative(ctx.registry, childDepPath)
      const pkgName = nameVerFromPkgSnapshot(relDepPath, childPkgSnapshot).name
      children[alias] = path.join(ctx.virtualStoreDir, `.${pkgIdToFilename(childDepPath, ctx.prefix)}`, 'node_modules', pkgName)
    } else if (allDeps[alias].indexOf('file:') === 0) {
      children[alias] = path.resolve(ctx.prefix, allDeps[alias].substr(5))
    } else {
      throw new Error(`${childRelDepPath} not found in shrinkwrap.yaml`)
    }
  }
  return children
}

async function getCache (storePath: string, pkgId: string) {
  return (await getCacheByEngine(storePath, pkgId))[ENGINE_NAME] as string
}

function pkgIsIndependent (pkgSnapshot: PackageSnapshot) {
  return pkgSnapshot.dependencies === undefined && pkgSnapshot.optionalDependencies === undefined
}

export interface DependenciesGraphNode {
  hasBundledDependencies: boolean,
  centralLocation: string,
  modules: string,
  fetchingFiles: Promise<PackageFilesResponse>,
  finishing: Promise<void>,
  peripheralLocation: string,
  children: {[alias: string]: string},
  // an independent package is a package that
  // has neither regular nor peer dependencies
  independent: boolean,
  optionalDependencies: Set<string>,
  optional: boolean,
  relDepPath: string, // this option is only needed for saving pendingBuild when running with --ignore-scripts flag
  pkgId: string, // TODO: this option is currently only needed when running postinstall scripts but even there it should be not used
  isBuilt: boolean,
  requiresBuild: boolean,
  prepare: boolean,
  hasBin: boolean,
}

export interface DependenciesGraph {
  [depPath: string]: DependenciesGraphNode
}

const limitLinking = pLimit(16)

async function linkAllPkgs (
  storeController: StoreController,
  depNodes: DependenciesGraphNode[],
  opts: {
    force: boolean,
    sideEffectsCache: boolean,
  },
) {
  return Promise.all(
    depNodes.map(async (depNode) => {
      const filesResponse = await depNode.fetchingFiles

      if (depNode.independent) return
      return storeController.importPackage(depNode.centralLocation, depNode.peripheralLocation, {
        filesResponse,
        force: opts.force,
      })
    }),
  )
}

async function linkAllBins (
  depGraph: DependenciesGraph,
  opts: {
    optional: boolean,
    warn: (message: string) => void,
  },
) {
  return Promise.all(
    R.values(depGraph)
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

        const binPath = path.join(depNode.peripheralLocation, 'node_modules', '.bin')
        const pkgSnapshots = R.props<string, DependenciesGraphNode>(R.values(childrenToLink), depGraph)

        if (pkgSnapshots.indexOf(undefined as any) !== -1) { // tslint:disable-line
          await linkBins(depNode.modules, binPath, {warn: opts.warn})
        } else {
          const pkgs = await Promise.all(
            pkgSnapshots
              .filter((dep) => dep.hasBin)
              .map(async (dep) => ({
                  location: dep.peripheralLocation,
                  manifest: await readPackageFromDir(dep.peripheralLocation),
              })),
          )

          await linkBinsOfPackages(pkgs, binPath, {warn: opts.warn})
        }

        // link also the bundled dependencies` bins
        if (depNode.hasBundledDependencies) {
          const bundledModules = path.join(depNode.peripheralLocation, 'node_modules')
          await linkBins(bundledModules, binPath, {warn: opts.warn})
        }
      })),
  )
}

async function linkAllModules (
  depGraph: DependenciesGraph,
  opts: {
    optional: boolean,
  },
) {
  return Promise.all(
    R.values(depGraph)
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
              // if (!pkg.installable) return
              await symlinkDependencyTo(alias, childrenToLink[alias], depNode.modules)
            }),
        )
      })),
  )
}

function symlinkDependencyTo (alias: string, peripheralLocation: string, dest: string) {
  const linkPath = path.join(dest, alias)
  linkLogger.debug({target: peripheralLocation, link: linkPath})
  return symlinkDir(peripheralLocation, linkPath)
}

// TODO: move this to separate package
// the version of the function which is in supi also accepts `opts.skip`
// headless will never skip anything
function filterShrinkwrap (
  shr: Shrinkwrap,
  opts: {
    importerPath: string,
    include: IncludedDependencies,
  },
): Shrinkwrap {
  let pairs = R.toPairs(shr.packages || {}) as Array<[string, PackageSnapshot]>
  if (!opts.include.dependencies) {
    pairs = pairs.filter((pair) => pair[1].dev !== false || pair[1].optional)
  }
  if (!opts.include.devDependencies) {
    pairs = pairs.filter((pair) => pair[1].dev !== true)
  }
  if (!opts.include.optionalDependencies) {
    pairs = pairs.filter((pair) => !pair[1].optional)
  }
  const shrImporter = shr.importers[opts.importerPath]
  return {
    importers: {
      ...shr.importers,
      [opts.importerPath]: {
        dependencies: !opts.include.dependencies ? {} : shrImporter.dependencies || {},
        devDependencies: !opts.include.devDependencies ? {} : shrImporter.devDependencies || {},
        optionalDependencies: !opts.include.optionalDependencies ? {} : shrImporter.optionalDependencies || {},
        specifiers: shrImporter.specifiers,
      },
    },
    packages: R.fromPairs(pairs),
    registry: shr.registry,
    shrinkwrapVersion: shr.shrinkwrapVersion,
  } as Shrinkwrap
}
