import {
  ENGINE_NAME,
  LAYOUT_VERSION,
  WANTED_LOCKFILE,
} from '@pnpm/constants'
import {
  packageJsonLogger,
  progressLogger,
  rootLogger,
  stageLogger,
  statsLogger,
  summaryLogger,
} from '@pnpm/core-loggers'
import filterLockfile, {
  filterLockfileByImportersAndEngine,
} from '@pnpm/filter-lockfile'
import { runLifecycleHooksConcurrently } from '@pnpm/lifecycle'
import linkBins, { linkBinsOfPackages } from '@pnpm/link-bins'
import {
  Lockfile,
  PackageSnapshot,
  readCurrentLockfile,
  readWantedLockfile,
  writeCurrentLockfile,
} from '@pnpm/lockfile-file'
import {
  nameVerFromPkgSnapshot,
  packageIdFromSnapshot,
  packageIsIndependent,
  pkgSnapshotToResolution,
  satisfiesPackageJson,
} from '@pnpm/lockfile-utils'
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
import { shamefullyFlattenByLockfile } from '@pnpm/shamefully-flatten'
import {
  PackageFilesResponse,
  StoreController,
} from '@pnpm/store-controller-types'
import symlinkDependency, { symlinkDirectRootDependency } from '@pnpm/symlink-dependency'
import { PackageJson, Registries } from '@pnpm/types'
import { realNodeModulesDir } from '@pnpm/utils'
import dp = require('dependency-path')
import fs = require('mz/fs')
import pLimit from 'p-limit'
import path = require('path')
import R = require('ramda')
import runDependenciesScripts from './runDependenciesScripts'

const brokenNodeModulesLogger = logger('_broken_node_modules')

export type ReporterFunction = (logObj: LogBase) => void

export interface HeadlessOptions {
  childConcurrency?: number,
  currentLockfile?: Lockfile,
  currentEngine: {
    nodeVersion: string,
    pnpmVersion: string,
  },
  engineStrict: boolean,
  ignoreScripts: boolean,
  include: IncludedDependencies,
  independentLeaves: boolean,
  importers: Array<{
    bin: string,
    buildIndex: number,
    hoistedAliases: {[depPath: string]: string[]}
    modulesDir: string,
    id: string,
    pkg: PackageJson,
    prefix: string,
    pruneDirectDependencies?: boolean,
    shamefullyFlatten: boolean,
  }>,
  lockfileDirectory: string,
  storeController: StoreController,
  sideEffectsCacheRead: boolean,
  sideEffectsCacheWrite: boolean,
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
  wantedLockfile?: Lockfile,
  ownLifecycleHooksStdio?: 'inherit' | 'pipe',
  pendingBuilds: string[],
  skipped: Set<string>,
}

export default async (opts: HeadlessOptions) => {
  const reporter = opts.reporter
  if (reporter) {
    streamParser.on('data', reporter)
  }

  const lockfileDirectory = opts.lockfileDirectory
  const wantedLockfile = opts.wantedLockfile || await readWantedLockfile(lockfileDirectory, { ignoreIncompatible: false })

  if (!wantedLockfile) {
    throw new Error(`Headless installation requires a ${WANTED_LOCKFILE} file`)
  }

  const currentLockfile = opts.currentLockfile || await readCurrentLockfile(lockfileDirectory, { ignoreIncompatible: false })
  const virtualStoreDir = await realNodeModulesDir(lockfileDirectory)

  for (const importer of opts.importers) {
    if (!satisfiesPackageJson(wantedLockfile, importer.pkg, importer.id)) {
      const err = new Error(`Cannot install with "frozen-lockfile" because ${WANTED_LOCKFILE} is not up-to-date with ` +
        path.relative(opts.lockfileDirectory, path.join(importer.prefix, 'package.json')))
      err['code'] = 'ERR_PNPM_OUTDATED_LOCKFILE' // tslint:disable-line
      throw err
    }
  }

  const scriptsOpts = {
    optional: false,
    rawNpmConfig: opts.rawNpmConfig,
    stdio: opts.ownLifecycleHooksStdio || 'inherit',
    unsafePerm: opts.unsafePerm || false,
  }

  if (!opts.ignoreScripts) {
    await runLifecycleHooksConcurrently(
      ['preinstall'],
      opts.importers,
      opts.childConcurrency || 5,
      scriptsOpts,
    )
  }

  const skipped = opts.skipped || new Set<string>()
  const filterOpts = {
    include: opts.include,
    registries: opts.registries,
    skipped,
  }
  if (currentLockfile) {
    await prune({
      dryRun: false,
      importers: opts.importers,
      lockfileDirectory,
      newLockfile: filterLockfile(wantedLockfile, filterOpts),
      oldLockfile: currentLockfile,
      pruneStore: opts.pruneStore,
      registries: opts.registries,
      storeController: opts.storeController,
      virtualStoreDir,
    })
  } else {
    statsLogger.debug({
      prefix: lockfileDirectory,
      removed: 0,
    })
  }

  stageLogger.debug({
    prefix: opts.lockfileDirectory,
    stage: 'importing_started',
  })

  const filteredLockfile = filterLockfileByImportersAndEngine(wantedLockfile, opts.importers.map((importer) => importer.id), {
    ...filterOpts,
    currentEngine: opts.currentEngine,
    engineStrict: opts.engineStrict,
    failOnMissingDependencies: true,
    includeIncompatiblePackages: opts.force === true,
    prefix: lockfileDirectory,
  })
  const res = await lockfileToDepGraph(
    filteredLockfile,
    opts.force ? null : currentLockfile,
    {
      ...opts,
      importerIds: opts.importers.map((importer) => importer.id),
      prefix: lockfileDirectory,
      skipped,
      virtualStoreDir,
    } as LockfileToDepGraphOptions,
  )
  const depGraph = res.graph

  statsLogger.debug({
    added: Object.keys(depGraph).length,
    prefix: lockfileDirectory,
  })

  await Promise.all([
    linkAllModules(depGraph, {
      lockfileDirectory: opts.lockfileDirectory,
      optional: opts.include.optionalDependencies,
    }),
    linkAllPkgs(opts.storeController, R.values(depGraph), opts),
  ])

  stageLogger.debug({
    prefix: opts.lockfileDirectory,
    stage: 'importing_done',
  })

  function warn (message: string) {
    logger.warn({
      message,
      prefix: lockfileDirectory,
    })
  }

  await linkAllBins(depGraph, { optional: opts.include.optionalDependencies, warn })

  await Promise.all(opts.importers.map(async (importer) => {
    if (importer.shamefullyFlatten) {
      importer.hoistedAliases = await shamefullyFlattenByLockfile(filteredLockfile, importer.id, {
        getIndependentPackageLocation: opts.independentLeaves
          ? async (packageId: string, packageName: string) => {
            const { directory } = await opts.storeController.getPackageLocation(packageId, packageName, {
              lockfileDirectory: opts.lockfileDirectory,
              targetEngine: opts.sideEffectsCacheRead && ENGINE_NAME || undefined,
            })
            return directory
          }
          : undefined,
        lockfileDirectory: opts.lockfileDirectory,
        modulesDir: importer.modulesDir,
        registries: opts.registries,
        virtualStoreDir,
      })
    } else {
      importer.hoistedAliases = {}
    }
  }))

  await Promise.all(opts.importers.map(async (importer) => {
    await linkRootPackages(filteredLockfile, {
      importerId: importer.id,
      importerModulesDir: importer.modulesDir,
      prefix: importer.prefix,
      registries: opts.registries,
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

  if (currentLockfile && !R.equals(opts.importers.map((importer) => importer.id).sort(), Object.keys(filteredLockfile.importers).sort())) {
    Object.assign(filteredLockfile.packages, currentLockfile.packages)
  }
  await writeCurrentLockfile(lockfileDirectory, filteredLockfile)

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
    skipped: Array.from(skipped),
    store: opts.store,
  })

  if (!opts.ignoreScripts) {
    for (const importer of opts.importers) {
      await runDependenciesScripts(depGraph, R.values(res.directDependenciesByImporterId[importer.id]).filter((loc) => depGraph[loc]), {
        childConcurrency: opts.childConcurrency,
        prefix: importer.prefix,
        rawNpmConfig: opts.rawNpmConfig,
        rootNodeModulesDir: importer.modulesDir,
        sideEffectsCacheWrite: opts.sideEffectsCacheWrite,
        storeController: opts.storeController,
        unsafePerm: opts.unsafePerm,
        userAgent: opts.userAgent,
      })
    }
  }

  // waiting till package requests are finished
  await Promise.all(R.values(depGraph).map((depNode) => depNode.finishing))

  summaryLogger.debug({ prefix: opts.lockfileDirectory })

  await opts.storeController.close()

  if (!opts.ignoreScripts) {
    await runLifecycleHooksConcurrently(
      ['install', 'postinstall', 'prepublish', 'prepare'],
      opts.importers,
      opts.childConcurrency || 5,
      scriptsOpts,
    )
  }

  if (reporter) {
    streamParser.removeListener('data', reporter)
  }
}

async function linkRootPackages (
  lockfile: Lockfile,
  opts: {
    registries: Registries,
    importerId: string,
    importerModulesDir: string,
    prefix: string,
    rootDependencies: {[alias: string]: string},
  },
) {
  const lockfileImporter = lockfile.importers[opts.importerId]
  const allDeps = {
    ...lockfileImporter.devDependencies,
    ...lockfileImporter.dependencies,
    ...lockfileImporter.optionalDependencies,
  }
  return Promise.all(
    R.keys(allDeps)
      .map(async (alias) => {
        if (allDeps[alias].startsWith('link:')) {
          const isDev = lockfileImporter.devDependencies && lockfileImporter.devDependencies[alias]
          const isOptional = lockfileImporter.optionalDependencies && lockfileImporter.optionalDependencies[alias]
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
        const depPath = dp.refToAbsolute(allDeps[alias], alias, opts.registries)
        const peripheralLocation = opts.rootDependencies[alias]
        // Skipping linked packages
        if (!peripheralLocation) {
          return
        }
        if ((await symlinkDependency(peripheralLocation, opts.importerModulesDir, alias)).reused) {
          return
        }
        const isDev = lockfileImporter.devDependencies && lockfileImporter.devDependencies[alias]
        const isOptional = lockfileImporter.optionalDependencies && lockfileImporter.optionalDependencies[alias]

        const relDepPath = dp.refToRelative(allDeps[alias], alias)
        if (relDepPath === null) return
        const pkgSnapshot = lockfile.packages && lockfile.packages[relDepPath]
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

interface LockfileToDepGraphOptions {
  force: boolean,
  independentLeaves: boolean,
  importerIds: string[],
  lockfileDirectory: string,
  skipped: Set<string>,
  storeController: StoreController,
  store: string,
  prefix: string,
  registries: Registries,
  sideEffectsCacheRead: boolean,
  virtualStoreDir: string,
}

async function lockfileToDepGraph (
  lockfile: Lockfile,
  currentLockfile: Lockfile | null,
  opts: LockfileToDepGraphOptions,
) {
  const currentPackages = currentLockfile && currentLockfile.packages || {}
  const graph: DependenciesGraph = {}
  let directDependenciesByImporterId: { [importerId: string]: { [alias: string]: string } } = {}
  if (lockfile.packages) {
    const pkgSnapshotByLocation = {}
    await Promise.all(
      R.keys(lockfile.packages).map(async (relDepPath) => {
        const depPath = dp.resolve(opts.registries, relDepPath)
        const pkgSnapshot = lockfile.packages![relDepPath]
        // TODO: optimize. This info can be already returned by pkgSnapshotToResolution()
        const pkgName = nameVerFromPkgSnapshot(relDepPath, pkgSnapshot).name
        const modules = path.join(opts.virtualStoreDir, `.${pkgIdToFilename(depPath, opts.lockfileDirectory)}`, 'node_modules')
        const packageId = packageIdFromSnapshot(relDepPath, pkgSnapshot, opts.registries)
        const pkgLocation = await opts.storeController.getPackageLocation(packageId, pkgName, {
          lockfileDirectory: opts.lockfileDirectory,
          targetEngine: opts.sideEffectsCacheRead && !opts.force && ENGINE_NAME || undefined,
        })

        const independent = opts.independentLeaves && packageIsIndependent(pkgSnapshot)
        const peripheralLocation = !independent
          ? path.join(modules, pkgName)
          : pkgLocation.directory
        if (
          currentPackages[relDepPath] && R.equals(currentPackages[relDepPath].dependencies, lockfile.packages![relDepPath].dependencies) &&
          R.equals(currentPackages[relDepPath].optionalDependencies, lockfile.packages![relDepPath].optionalDependencies)
        ) {
          if (await fs.exists(peripheralLocation)) {
            return
          }

          brokenNodeModulesLogger.debug({
            missing: peripheralLocation,
          })
        }
        const resolution = pkgSnapshotToResolution(relDepPath, pkgSnapshot, opts.registries)
        progressLogger.debug({
          packageId,
          requester: opts.lockfileDirectory,
          status: 'resolved',
        })
        let fetchResponse = opts.storeController.fetchPackage({
          force: false,
          pkgId: packageId,
          prefix: opts.prefix,
          resolution,
        })
        if (fetchResponse instanceof Promise) fetchResponse = await fetchResponse
        fetchResponse.fetchingFiles // tslint:disable-line
          .then((fetchResult) => {
            progressLogger.debug({
              packageId,
              requester: opts.lockfileDirectory,
              status: fetchResult.fromStore
                ? 'found_in_store' : 'fetched',
            })
          })
        graph[peripheralLocation] = {
          centralLocation: pkgLocation.directory,
          children: {},
          fetchingFiles: fetchResponse.fetchingFiles,
          finishing: fetchResponse.finishing,
          hasBin: pkgSnapshot.hasBin === true,
          hasBundledDependencies: !!pkgSnapshot.bundledDependencies,
          independent,
          isBuilt: pkgLocation.isBuilt,
          modules,
          name: pkgName,
          optional: !!pkgSnapshot.optional,
          optionalDependencies: new Set(R.keys(pkgSnapshot.optionalDependencies)),
          peripheralLocation,
          pkgId: packageId,
          prepare: pkgSnapshot.prepare === true,
          relDepPath,
          requiresBuild: pkgSnapshot.requiresBuild === true,
        }
        pkgSnapshotByLocation[peripheralLocation] = pkgSnapshot
      })
    )
    const ctx = {
      force: opts.force,
      graph,
      independentLeaves: opts.independentLeaves,
      lockfileDirectory: opts.lockfileDirectory,
      pkgSnapshotsByRelDepPaths: lockfile.packages,
      prefix: opts.prefix,
      registries: opts.registries,
      sideEffectsCacheRead: opts.sideEffectsCacheRead,
      skipped: opts.skipped,
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
      const lockfileImporter = lockfile.importers[importerId]
      const rootDeps = { ...lockfileImporter.devDependencies, ...lockfileImporter.dependencies, ...lockfileImporter.optionalDependencies }
      directDependenciesByImporterId[importerId] = await getChildrenPaths(ctx, rootDeps)
    }
  }
  return { graph, directDependenciesByImporterId }
}

async function getChildrenPaths (
  ctx: {
    graph: DependenciesGraph,
    force: boolean,
    registries: Registries,
    virtualStoreDir: string,
    independentLeaves: boolean,
    store: string,
    skipped: Set<string>,
    pkgSnapshotsByRelDepPaths: {[relDepPath: string]: PackageSnapshot},
    prefix: string,
    lockfileDirectory: string,
    sideEffectsCacheRead: boolean,
    storeController: StoreController,
  },
  allDeps: {[alias: string]: string},
) {
  const children: {[alias: string]: string} = {}
  for (const alias of R.keys(allDeps)) {
    const childDepPath = dp.refToAbsolute(allDeps[alias], alias, ctx.registries)
    if (childDepPath === null) {
      children[alias] = path.resolve(ctx.prefix, allDeps[alias].substr(5))
      continue
    }
    const childRelDepPath = dp.refToRelative(allDeps[alias], alias) as string
    const childPkgSnapshot = ctx.pkgSnapshotsByRelDepPaths[childRelDepPath]
    if (ctx.graph[childDepPath]) {
      children[alias] = ctx.graph[childDepPath].peripheralLocation
    } else if (ctx.independentLeaves && packageIsIndependent(childPkgSnapshot)) {
      const pkgId = childPkgSnapshot.id || childDepPath
      const pkgName = nameVerFromPkgSnapshot(childRelDepPath, childPkgSnapshot).name
      const pkgLocation = await ctx.storeController.getPackageLocation(pkgId, pkgName, {
        lockfileDirectory: ctx.lockfileDirectory,
        targetEngine: ctx.sideEffectsCacheRead && !ctx.force && ENGINE_NAME || undefined,
      })
      children[alias] = pkgLocation.directory
    } else if (childPkgSnapshot) {
      const pkgName = nameVerFromPkgSnapshot(childRelDepPath, childPkgSnapshot).name
      children[alias] = path.join(ctx.virtualStoreDir, `.${pkgIdToFilename(childDepPath, ctx.lockfileDirectory)}`, 'node_modules', pkgName)
    } else if (allDeps[alias].indexOf('file:') === 0) {
      children[alias] = path.resolve(ctx.prefix, allDeps[alias].substr(5))
    } else if (!ctx.skipped.has(childRelDepPath)) {
      throw new Error(`${childRelDepPath} not found in ${WANTED_LOCKFILE}`)
    }
  }
  return children
}

export interface DependenciesGraphNode {
  hasBundledDependencies: boolean,
  centralLocation: string,
  modules: string,
  name: string,
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
    lockfileDirectory: string,
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
              if (alias === depNode.name) {
                logger.warn({
                  message: `Cannot link dependency with name ${alias} to ${depNode.modules}. Dependency's name should differ from the parent's name.`,
                  prefix: opts.lockfileDirectory,
                })
                return
              }
              await symlinkDependency(childrenToLink[alias], depNode.modules, alias)
            }),
        )
      })),
  )
}
