import {
  packageJsonLogger,
  progressLogger,
  rootLogger,
  stageLogger,
  statsLogger,
  summaryLogger,
} from '@pnpm/core-loggers'
import filterShrinkwrap, {
  filterByImporters as filterShrinkwrapByImporters,
} from '@pnpm/filter-shrinkwrap'
import runLifecycleHooks from '@pnpm/lifecycle'
import linkBins, { linkBinsOfPackages } from '@pnpm/link-bins'
import logger, {
  LogBase,
  streamParser,
} from '@pnpm/logger'
import { prune } from '@pnpm/modules-cleaner'
import {
  IncludedDependencies,
  write as writeModulesYaml,
} from '@pnpm/modules-yaml'
import pkgIdToFilename from '@pnpm/pkgid-to-filename'
import { fromDir as readPackageFromDir } from '@pnpm/read-package-json'
import { shamefullyFlattenByShrinkwrap } from '@pnpm/shamefully-flatten'
import {
  PackageSnapshot,
  readCurrent,
  readWanted,
  Shrinkwrap,
  writeCurrentOnly as writeCurrentShrinkwrapOnly,
} from '@pnpm/shrinkwrap-file'
import {
  nameVerFromPkgSnapshot,
  pkgSnapshotToResolution,
  satisfiesPackageJson,
} from '@pnpm/shrinkwrap-utils'
import {
  PackageFilesResponse,
  StoreController,
} from '@pnpm/store-controller-types'
import symlinkDependency, { symlinkDirectRootDependency } from '@pnpm/symlink-dependency'
import { PackageJson, Registries } from '@pnpm/types'
import { realNodeModulesDir } from '@pnpm/utils'
import dp = require('dependency-path')
import pLimit = require('p-limit')
import path = require('path')
import R = require('ramda')
import {
  ENGINE_NAME,
  LAYOUT_VERSION,
} from './constants'
import runDependenciesScripts from './runDependenciesScripts'

export type ReporterFunction = (logObj: LogBase) => void

export interface HeadlessOptions {
  childConcurrency?: number,
  currentShrinkwrap?: Shrinkwrap,
  ignoreScripts: boolean,
  include: IncludedDependencies,
  independentLeaves: boolean,
  importers: Array<{
    bin: string,
    hoistedAliases: {[depPath: string]: string[]}
    modulesDir: string,
    id: string,
    pkg: PackageJson,
    prefix: string,
    pruneDirectDependencies?: boolean,
    shamefullyFlatten: boolean,
  }>,
  shrinkwrapDirectory: string,
  storeController: StoreController,
  verifyStoreIntegrity: boolean,
  sideEffectsCache: boolean,
  sideEffectsCacheReadonly: boolean,
  force: boolean,
  store: string,
  rawNpmConfig: object,
  unsafePerm: boolean,
  userAgent: string,
  registries: Registries,
  reporter?: ReporterFunction,
  packageManager: {
    name: string,
    version: string,
  },
  pruneStore: boolean,
  wantedShrinkwrap?: Shrinkwrap,
  ownLifecycleHooksStdio?: 'inherit' | 'pipe',
  pendingBuilds: string[],
}

export default async (opts: HeadlessOptions) => {
  const reporter = opts.reporter
  if (reporter) {
    streamParser.on('data', reporter)
  }

  const shrinkwrapDirectory = opts.shrinkwrapDirectory
  const wantedShrinkwrap = opts.wantedShrinkwrap || await readWanted(shrinkwrapDirectory, { ignoreIncompatible: false })

  if (!wantedShrinkwrap) {
    throw new Error('Headless installation requires a shrinkwrap.yaml file')
  }

  const currentShrinkwrap = opts.currentShrinkwrap || await readCurrent(shrinkwrapDirectory, { ignoreIncompatible: false })
  const virtualStoreDir = await realNodeModulesDir(shrinkwrapDirectory)

  for (const importer of opts.importers) {
    if (!satisfiesPackageJson(wantedShrinkwrap, importer.pkg, importer.id)) {
      const err = new Error('Cannot install with "frozen-shrinkwrap" because shrinkwrap.yaml is not up-to-date with ' +
        path.relative(opts.shrinkwrapDirectory, path.join(importer.prefix, 'package.json')))
      err['code'] = 'ERR_PNPM_OUTDATED_SHRINKWRAP' // tslint:disable-line
      throw err
    }
  }

  if (!opts.ignoreScripts) {
    for (const importer of opts.importers) {
      const scripts = !opts.ignoreScripts && importer.pkg.scripts || {}

      const scriptsOpts = {
        depPath: importer.prefix,
        pkgRoot: importer.prefix,
        rawNpmConfig: opts.rawNpmConfig,
        rootNodeModulesDir: importer.modulesDir,
        stdio: opts.ownLifecycleHooksStdio || 'inherit',
        unsafePerm: opts.unsafePerm || false,
      }

      if (scripts.preinstall) {
        await runLifecycleHooks('preinstall', importer.pkg, scriptsOpts)
      }
    }
  }

  const filterOpts = {
    defaultRegistry: opts.registries.default,
    include: opts.include,
    skipped: new Set<string>(),
  }
  if (currentShrinkwrap) {
    await prune({
      dryRun: false,
      importers: opts.importers,
      newShrinkwrap: filterShrinkwrap(wantedShrinkwrap, filterOpts),
      oldShrinkwrap: currentShrinkwrap,
      pruneStore: opts.pruneStore,
      registries: opts.registries,
      shrinkwrapDirectory,
      storeController: opts.storeController,
      virtualStoreDir,
    })
  } else {
    statsLogger.debug({
      prefix: shrinkwrapDirectory,
      removed: 0,
    })
  }

  stageLogger.debug('importing_started')
  const filteredShrinkwrap = filterShrinkwrapByImporters(wantedShrinkwrap, opts.importers.map((importer) => importer.id), {
    ...filterOpts,
    failOnMissingDependencies: true,
  })
  const res = await shrinkwrapToDepGraph(
    filteredShrinkwrap,
    opts.force ? null : currentShrinkwrap,
    {
      ...opts,
      defaultRegistry: opts.registries.default,
      importerIds: opts.importers.map((importer) => importer.id),
      prefix: shrinkwrapDirectory,
      virtualStoreDir,
    } as ShrinkwrapToDepGraphOptions,
  )
  const depGraph = res.graph

  statsLogger.debug({
    added: Object.keys(depGraph).length,
    prefix: shrinkwrapDirectory,
  })

  await Promise.all([
    linkAllModules(depGraph, { optional: opts.include.optionalDependencies }),
    linkAllPkgs(opts.storeController, R.values(depGraph), opts),
  ])
  stageLogger.debug('importing_done')

  function warn (message: string) {
    logger.warn({
      message,
      prefix: shrinkwrapDirectory,
    })
  }

  await linkAllBins(depGraph, { optional: opts.include.optionalDependencies, warn })

  await Promise.all(opts.importers.map(async (importer) => {
    if (importer.shamefullyFlatten) {
      importer.hoistedAliases = await shamefullyFlattenByShrinkwrap(filteredShrinkwrap, importer.id, {
        defaultRegistry: opts.registries.default,
        modulesDir: importer.modulesDir,
        prefix: importer.prefix,
        virtualStoreDir,
      })
    } else {
      importer.hoistedAliases = {}
    }
  }))

  await Promise.all(opts.importers.map(async (importer) => {
    await linkRootPackages(filteredShrinkwrap, {
      defaultRegistry: opts.registries.default,
      importerId: importer.id,
      importerModulesDir: importer.modulesDir,
      prefix: importer.prefix,
      rootDependencies: res.directDependenciesByImporterId[importer.id],
    })
    const bin = path.join(importer.modulesDir, '.bin')
    await linkBins(importer.modulesDir, bin, { warn })

    // Even though headless installation will never update the package.json
    // this needs to be logged because otherwise install summary won't be printed
    packageJsonLogger.debug({
      prefix: importer.prefix,
      updated: importer.pkg,
    })
  }))

  await writeCurrentShrinkwrapOnly(shrinkwrapDirectory, filteredShrinkwrap)
  if (opts.ignoreScripts) {
    for (const importer of opts.importers) {
      if (opts.ignoreScripts && importer.pkg && importer.pkg.scripts &&
        (importer.pkg.scripts.preinstall || importer.pkg.scripts.prepublish ||
          importer.pkg.scripts.install ||
          importer.pkg.scripts.postinstall ||
          importer.pkg.scripts.prepare)
      ) {
        opts.pendingBuilds.push(importer.id)
      }
    }
    // we can use concat here because we always only append new packages, which are guaranteed to not be there by definition
    opts.pendingBuilds = opts.pendingBuilds
      .concat(
        R.values(depGraph)
          .filter((node) => node.requiresBuild)
          .map((node) => node.relDepPath),
      )
  }
  await writeModulesYaml(virtualStoreDir, {
    importers: opts.importers.reduce((acc, importer) => {
      acc[importer.id] = {
        hoistedAliases: importer.hoistedAliases,
        shamefullyFlatten: importer.shamefullyFlatten,
      }
      return acc
    }, {}),
    included: opts.include,
    independentLeaves: !!opts.independentLeaves,
    layoutVersion: LAYOUT_VERSION,
    packageManager: `${opts.packageManager.name}@${opts.packageManager.version}`,
    pendingBuilds: opts.pendingBuilds,
    registries: opts.registries,
    skipped: [],
    store: opts.store,
  })

  if (!opts.ignoreScripts) {
    for (const importer of opts.importers) {
      await runDependenciesScripts(depGraph, R.values(res.directDependenciesByImporterId[importer.id]).filter((loc) => depGraph[loc]), {
        childConcurrency: opts.childConcurrency,
        prefix: importer.prefix,
        rawNpmConfig: opts.rawNpmConfig,
        rootNodeModulesDir: importer.modulesDir,
        sideEffectsCache: opts.sideEffectsCache,
        sideEffectsCacheReadonly: opts.sideEffectsCacheReadonly,
        storeController: opts.storeController,
        unsafePerm: opts.unsafePerm,
        userAgent: opts.userAgent,
      })
    }
  }

  // waiting till package requests are finished
  await Promise.all(R.values(depGraph).map((depNode) => depNode.finishing))

  summaryLogger.debug({ prefix: opts.shrinkwrapDirectory })

  await opts.storeController.close()

  if (!opts.ignoreScripts) {
    for (const importer of opts.importers) {
      if (!importer.pkg.scripts) continue

      const scriptsOpts = {
        depPath: importer.prefix,
        pkgRoot: importer.prefix,
        rawNpmConfig: opts.rawNpmConfig,
        rootNodeModulesDir: importer.modulesDir,
        stdio: opts.ownLifecycleHooksStdio || 'inherit',
        unsafePerm: opts.unsafePerm || false,
      }

      if (importer.pkg.scripts.install) {
        await runLifecycleHooks('install', importer.pkg, scriptsOpts)
      }
      if (importer.pkg.scripts.postinstall) {
        await runLifecycleHooks('postinstall', importer.pkg, scriptsOpts)
      }
      if (importer.pkg.scripts.prepublish) {
        await runLifecycleHooks('prepublish', importer.pkg, scriptsOpts)
      }
      if (importer.pkg.scripts.prepare) {
        await runLifecycleHooks('prepare', importer.pkg, scriptsOpts)
      }
    }
  }

  if (reporter) {
    streamParser.removeListener('data', reporter)
  }
}

async function linkRootPackages (
  shr: Shrinkwrap,
  opts: {
    defaultRegistry: string,
    importerId: string,
    importerModulesDir: string,
    prefix: string,
    rootDependencies: {[alias: string]: string},
  },
) {
  const shrImporter = shr.importers[opts.importerId]
  const allDeps = {
    ...shrImporter.devDependencies,
    ...shrImporter.dependencies,
    ...shrImporter.optionalDependencies,
  }
  return Promise.all(
    R.keys(allDeps)
      .map(async (alias) => {
        if (allDeps[alias].startsWith('link:')) {
          const isDev = shrImporter.devDependencies && shrImporter.devDependencies[alias]
          const isOptional = shrImporter.optionalDependencies && shrImporter.optionalDependencies[alias]
          const packageDir = path.join(opts.prefix, allDeps[alias].substr(5))
          const linkedPackage = await readPackageFromDir(packageDir)
          await symlinkDirectRootDependency(packageDir, opts.importerModulesDir, alias, {
            fromDependenciesField: isDev && 'devDependencies' ||
              isOptional && 'optionalDependencies' ||
              'dependencies',
            linkedPackage,
            prefix: opts.prefix,
          })
          return
        }
        const depPath = dp.refToAbsolute(allDeps[alias], alias, opts.defaultRegistry)
        const peripheralLocation = opts.rootDependencies[alias]
        // Skipping linked packages
        if (!peripheralLocation) {
          return
        }
        if ((await symlinkDependency(peripheralLocation, opts.importerModulesDir, alias)).reused) {
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
          prefix: opts.prefix,
        })
      }),
  )
}

interface ShrinkwrapToDepGraphOptions {
  defaultRegistry: string,
  force: boolean,
  independentLeaves: boolean,
  importerIds: string[],
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
  let directDependenciesByImporterId: { [importerId: string]: { [alias: string]: string } } = {}
  if (shr.packages) {
    const pkgSnapshotByLocation = {}
    for (const relDepPath of R.keys(shr.packages)) {
      if (currentPackages[relDepPath] && R.equals(currentPackages[relDepPath].dependencies, shr.packages[relDepPath].dependencies) &&
        R.equals(currentPackages[relDepPath].optionalDependencies, shr.packages[relDepPath].optionalDependencies)) {
        continue
      }
      const depPath = dp.resolve(opts.defaultRegistry, relDepPath)
      const pkgSnapshot = shr.packages[relDepPath]
      const independent = opts.independentLeaves && pkgIsIndependent(pkgSnapshot)
      const resolution = pkgSnapshotToResolution(relDepPath, pkgSnapshot, opts.defaultRegistry)
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
      fetchResponse.fetchingFiles // tslint:disable-line
        .then((fetchResult) => {
          progressLogger.debug({
            pkgId,
            status: fetchResult.fromStore
              ? 'found_in_store' : 'fetched',
          })
        })
      const cache = !opts.force && await getCache(opts.storeController, opts.store, pkgId)
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
      registry: opts.defaultRegistry,
      store: opts.store,
      storeController: opts.storeController,
      virtualStoreDir: opts.virtualStoreDir,
    }
    for (const peripheralLocation of R.keys(graph)) {
      const pkgSnapshot = pkgSnapshotByLocation[peripheralLocation]
      const allDeps = { ...pkgSnapshot.dependencies, ...pkgSnapshot.optionalDependencies }

      graph[peripheralLocation].children = await getChildrenPaths(ctx, allDeps)
    }
    for (const importerId of opts.importerIds) {
      const shrImporter = shr.importers[importerId]
      const rootDeps = { ...shrImporter.devDependencies, ...shrImporter.dependencies, ...shrImporter.optionalDependencies }
      directDependenciesByImporterId[importerId] = await getChildrenPaths(ctx, rootDeps)
    }
  }
  return { graph, directDependenciesByImporterId }
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
    storeController: StoreController,
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
      const cache = !ctx.force && await getCache(ctx.storeController, ctx.store, pkgId)
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

async function getCache (store: StoreController, storePath: string, pkgId: string) {
  return (await store.getCacheByEngine(storePath, pkgId))[ENGINE_NAME] as string
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
          await linkBins(depNode.modules, binPath, { warn: opts.warn })
        } else {
          const pkgs = await Promise.all(
            pkgSnapshots
              .filter((dep) => dep.hasBin)
              .map(async (dep) => ({
                location: dep.peripheralLocation,
                manifest: await readPackageFromDir(dep.peripheralLocation),
              })),
          )

          await linkBinsOfPackages(pkgs, binPath, { warn: opts.warn })
        }

        // link also the bundled dependencies` bins
        if (depNode.hasBundledDependencies) {
          const bundledModules = path.join(depNode.peripheralLocation, 'node_modules')
          await linkBins(bundledModules, binPath, { warn: opts.warn })
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
              // if (!pkg.installable && pkg.optional) return
              await symlinkDependency(childrenToLink[alias], depNode.modules, alias)
            }),
        )
      })),
  )
}
