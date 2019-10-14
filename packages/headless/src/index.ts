import buildModules from '@pnpm/build-modules'
import {
  ENGINE_NAME,
  LAYOUT_VERSION,
  WANTED_LOCKFILE,
} from '@pnpm/constants'
import {
  packageManifestLogger,
  progressLogger,
  rootLogger,
  stageLogger,
  statsLogger,
  summaryLogger,
} from '@pnpm/core-loggers'
import PnpmError from '@pnpm/error'
import {
  filterLockfileByImportersAndEngine,
} from '@pnpm/filter-lockfile'
import hoist from '@pnpm/hoist'
import { runLifecycleHooksConcurrently } from '@pnpm/lifecycle'
import linkBins, { linkBinsOfPackages } from '@pnpm/link-bins'
import {
  getLockfileImporterId,
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
  satisfiesPackageManifest,
} from '@pnpm/lockfile-utils'
import logger, {
  LogBase,
  streamParser,
} from '@pnpm/logger'
import matcher from '@pnpm/matcher'
import { prune } from '@pnpm/modules-cleaner'
import {
  IncludedDependencies,
  write as writeModulesYaml,
} from '@pnpm/modules-yaml'
import pkgIdToFilename from '@pnpm/pkgid-to-filename'
import { readImporterManifestOnly } from '@pnpm/read-importer-manifest'
import { fromDir as readPackageFromDir } from '@pnpm/read-package-json'
import {
  PackageFilesResponse,
  StoreController,
} from '@pnpm/store-controller-types'
import symlinkDependency, { symlinkDirectRootDependency } from '@pnpm/symlink-dependency'
import { DependencyManifest, ImporterManifest, Registries } from '@pnpm/types'
import { realNodeModulesDir } from '@pnpm/utils'
import dp = require('dependency-path')
import fs = require('mz/fs')
import pLimit from 'p-limit'
import path = require('path')
import R = require('ramda')

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
  extraBinPaths?: string[],
  ignoreScripts: boolean,
  include: IncludedDependencies,
  independentLeaves: boolean,
  importers: Array<{
    bin: string,
    buildIndex: number,
    manifest: ImporterManifest,
    modulesDir: string,
    id: string,
    prefix: string,
    pruneDirectDependencies?: boolean,
  }>,
  hoistedAliases: {[depPath: string]: string[]}
  hoistPattern?: string[],
  lockfileDirectory: string,
  virtualStoreDir?: string,
  shamefullyHoist: boolean,
  storeController: StoreController,
  sideEffectsCacheRead: boolean,
  sideEffectsCacheWrite: boolean,
  force: boolean,
  store: string,
  rawConfig: object,
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

  const rootModulesDir = await realNodeModulesDir(lockfileDirectory)
  const virtualStoreDir = opts.virtualStoreDir ?? path.join(rootModulesDir, '.pnpm')
  const currentLockfile = opts.currentLockfile || await readCurrentLockfile(virtualStoreDir, { ignoreIncompatible: false })
  const hoistedModulesDir = opts.shamefullyHoist
    ? rootModulesDir : path.join(virtualStoreDir, 'node_modules')

  for (const { id, manifest, prefix } of opts.importers) {
    if (!satisfiesPackageManifest(wantedLockfile, manifest, id)) {
      throw new PnpmError('OUTDATED_LOCKFILE',
        `Cannot install with "frozen-lockfile" because ${WANTED_LOCKFILE} is not up-to-date with ` +
        path.relative(opts.lockfileDirectory, path.join(prefix, 'package.json')))
    }
  }

  const scriptsOpts = {
    optional: false,
    rawConfig: opts.rawConfig,
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
  if (currentLockfile) {
    await prune(
      opts.importers,
      {
        currentLockfile,
        dryRun: false,
        hoistedAliases: opts.hoistedAliases,
        hoistedModulesDir: opts.hoistPattern && hoistedModulesDir || undefined,
        include: opts.include,
        lockfileDirectory,
        pruneStore: opts.pruneStore,
        registries: opts.registries,
        skipped,
        storeController: opts.storeController,
        virtualStoreDir,
        wantedLockfile,
      },
    )
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

  const filterOpts = {
    include: opts.include,
    registries: opts.registries,
    skipped,
  }
  const filteredLockfile = filterLockfileByImportersAndEngine(wantedLockfile, opts.importers.map(({ id }) => id), {
    ...filterOpts,
    currentEngine: opts.currentEngine,
    engineStrict: opts.engineStrict,
    failOnMissingDependencies: true,
    includeIncompatiblePackages: opts.force === true,
    prefix: lockfileDirectory,
  })
  const { directDependenciesByImporterId, graph } = await lockfileToDepGraph(
    filteredLockfile,
    opts.force ? null : currentLockfile,
    {
      ...opts,
      importerIds: opts.importers.map(({ id }) => id),
      prefix: lockfileDirectory,
      skipped,
      virtualStoreDir,
    } as LockfileToDepGraphOptions,
  )
  const depNodes = R.values(graph)

  statsLogger.debug({
    added: depNodes.length,
    prefix: lockfileDirectory,
  })

  await Promise.all([
    linkAllModules(depNodes, {
      lockfileDirectory: opts.lockfileDirectory,
      optional: opts.include.optionalDependencies,
    }),
    linkAllPkgs(opts.storeController, depNodes, opts),
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

  const rootImporterWithFlatModules = opts.hoistPattern && opts.importers.find((importer) => importer.id === '.')
  let newHoistedAliases!: {[depPath: string]: string[]}
  if (rootImporterWithFlatModules) {
    newHoistedAliases = await hoist(matcher(opts.hoistPattern!), {
      getIndependentPackageLocation: opts.independentLeaves
        ? async (packageId: string, packageName: string) => {
          const { directory } = await opts.storeController.getPackageLocation(packageId, packageName, {
            lockfileDirectory: opts.lockfileDirectory,
            targetEngine: opts.sideEffectsCacheRead && ENGINE_NAME || undefined,
          })
          return directory
        }
        : undefined,
      lockfile: filteredLockfile,
      lockfileDirectory: opts.lockfileDirectory,
      modulesDir: hoistedModulesDir,
      registries: opts.registries,
      virtualStoreDir,
    })
  } else {
    newHoistedAliases = {}
  }

  await Promise.all(opts.importers.map(async ({ id, manifest, modulesDir, prefix }) => {
    await linkRootPackages(filteredLockfile, {
      importerId: id,
      importerModulesDir: modulesDir,
      importers: opts.importers,
      lockfileDirectory: opts.lockfileDirectory,
      prefix,
      registries: opts.registries,
      rootDependencies: directDependenciesByImporterId[id],
    })

    // Even though headless installation will never update the package.json
    // this needs to be logged because otherwise install summary won't be printed
    packageManifestLogger.debug({
      prefix,
      updated: manifest,
    })
  }))

  if (opts.ignoreScripts) {
    for (const { id, manifest } of opts.importers) {
      if (opts.ignoreScripts && manifest?.scripts &&
        (manifest.scripts.preinstall || manifest.scripts.prepublish ||
          manifest.scripts.install ||
          manifest.scripts.postinstall ||
          manifest.scripts.prepare)
      ) {
        opts.pendingBuilds.push(id)
      }
    }
    // we can use concat here because we always only append new packages, which are guaranteed to not be there by definition
    opts.pendingBuilds = opts.pendingBuilds
      .concat(
        depNodes
          .filter(({ requiresBuild }) => requiresBuild)
          .map(({ relDepPath }) => relDepPath),
      )
  } else {
    const directNodes = new Set<string>()
    for (const { id } of opts.importers) {
      R
        .values(directDependenciesByImporterId[id])
        .filter((loc) => graph[loc])
        .forEach((loc) => {
          directNodes.add(loc)
        })
    }
    const extraBinPaths = [...opts.extraBinPaths || []]
    if (opts.hoistPattern && !opts.shamefullyHoist) {
      extraBinPaths.unshift(path.join(virtualStoreDir, 'node_modules/.bin'))
    }
    await buildModules(graph, Array.from(directNodes), {
      childConcurrency: opts.childConcurrency,
      extraBinPaths,
      optional: opts.include.optionalDependencies,
      prefix: opts.lockfileDirectory,
      rawConfig: opts.rawConfig,
      rootNodeModulesDir: virtualStoreDir,
      sideEffectsCacheWrite: opts.sideEffectsCacheWrite,
      storeController: opts.storeController,
      unsafePerm: opts.unsafePerm,
      userAgent: opts.userAgent,
    })
  }

  await linkAllBins(graph, { optional: opts.include.optionalDependencies, warn })
  await Promise.all(opts.importers.map(linkBinsOfImporter))

  if (currentLockfile && !R.equals(opts.importers.map(({ id }) => id).sort(), Object.keys(filteredLockfile.importers).sort())) {
    Object.assign(filteredLockfile.packages, currentLockfile.packages)
  }
  await writeCurrentLockfile(virtualStoreDir, filteredLockfile)
  await writeModulesYaml(rootModulesDir, {
    hoistedAliases: newHoistedAliases,
    hoistPattern: opts.hoistPattern,
    included: opts.include,
    independentLeaves: !!opts.independentLeaves,
    layoutVersion: LAYOUT_VERSION,
    packageManager: `${opts.packageManager.name}@${opts.packageManager.version}`,
    pendingBuilds: opts.pendingBuilds,
    registries: opts.registries,
    shamefullyHoist: opts.shamefullyHoist || false,
    skipped: Array.from(skipped),
    store: opts.store,
    virtualStoreDir,
  })

  // waiting till package requests are finished
  await Promise.all(depNodes.map(({ finishing }) => finishing))

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

function linkBinsOfImporter (
  { modulesDir, bin, prefix }: {
    bin: string,
    modulesDir: string,
    prefix: string,
  },
) {
  const warn = (message: string) => logger.warn({ message, prefix })
  return linkBins(modulesDir, bin, {
    allowExoticManifests: true,
    warn,
  })
}

async function linkRootPackages (
  lockfile: Lockfile,
  opts: {
    registries: Registries,
    importerId: string,
    importerModulesDir: string,
    importers: Array<{ id: string, manifest: ImporterManifest }>,
    lockfileDirectory: string,
    prefix: string,
    rootDependencies: {[alias: string]: string},
  },
) {
  const importerManifestsByImporterId = {} as { [id: string]: ImporterManifest }
  for (const { id, manifest } of opts.importers) {
    importerManifestsByImporterId[id] = manifest
  }
  const lockfileImporter = lockfile.importers[opts.importerId]
  const allDeps = {
    ...lockfileImporter.devDependencies,
    ...lockfileImporter.dependencies,
    ...lockfileImporter.optionalDependencies,
  }
  return Promise.all(
    Object.keys(allDeps)
      .map(async (alias) => {
        if (allDeps[alias].startsWith('link:')) {
          const isDev = lockfileImporter.devDependencies?.[alias]
          const isOptional = lockfileImporter.optionalDependencies?.[alias]
          const packageDir = path.join(opts.prefix, allDeps[alias].substr(5))
          const linkedPackage = await (async () => {
            const importerId = getLockfileImporterId(opts.lockfileDirectory, packageDir)
            if (importerManifestsByImporterId[importerId]) {
              return importerManifestsByImporterId[importerId]
            }
            // TODO: cover this case with a test
            return await readImporterManifestOnly(packageDir) as DependencyManifest
          })() as DependencyManifest
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
        const isDev = lockfileImporter.devDependencies?.[alias]
        const isOptional = lockfileImporter.optionalDependencies?.[alias]

        const relDepPath = dp.refToRelative(allDeps[alias], alias)
        if (relDepPath === null) return
        const pkgSnapshot = lockfile.packages?.[relDepPath]
        if (!pkgSnapshot) return // this won't ever happen. Just making typescript happy
        const pkgId = pkgSnapshot.id || depPath || undefined
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
  include: IncludedDependencies,
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
  const currentPackages = currentLockfile?.packages ?? {}
  const graph: DependenciesGraph = {}
  let directDependenciesByImporterId: { [importerId: string]: { [alias: string]: string } } = {}
  if (lockfile.packages) {
    const pkgSnapshotByLocation = {}
    await Promise.all(
      Object.keys(lockfile.packages).map(async (relDepPath) => {
        const depPath = dp.resolve(opts.registries, relDepPath)
        const pkgSnapshot = lockfile.packages![relDepPath]
        // TODO: optimize. This info can be already returned by pkgSnapshotToResolution()
        const pkgName = nameVerFromPkgSnapshot(relDepPath, pkgSnapshot).name
        const modules = path.join(opts.virtualStoreDir, pkgIdToFilename(depPath, opts.lockfileDirectory), 'node_modules')
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
        fetchResponse.files() // tslint:disable-line
          .then(({ fromStore }) => {
            progressLogger.debug({
              packageId,
              requester: opts.lockfileDirectory,
              status: fromStore
                ? 'found_in_store' : 'fetched',
            })
          })
          .catch(() => {
            // ignore
          })
        graph[peripheralLocation] = {
          centralLocation: pkgLocation.directory,
          children: {},
          fetchingFiles: fetchResponse.files,
          finishing: fetchResponse.finishing,
          hasBin: pkgSnapshot.hasBin === true,
          hasBundledDependencies: !!pkgSnapshot.bundledDependencies,
          independent,
          isBuilt: pkgLocation.isBuilt,
          modules,
          name: pkgName,
          optional: !!pkgSnapshot.optional,
          optionalDependencies: new Set(R.keys(pkgSnapshot.optionalDependencies)),
          packageId,
          peripheralLocation,
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
      const allDeps = {
        ...pkgSnapshot.dependencies,
        ...(opts.include.optionalDependencies ? pkgSnapshot.optionalDependencies : {}),
      }

      graph[peripheralLocation].children = await getChildrenPaths(ctx, allDeps)
    }
    for (const importerId of opts.importerIds) {
      const lockfileImporter = lockfile.importers[importerId]
      const rootDeps = {
        ...(opts.include.devDependencies ? lockfileImporter.devDependencies : {}),
        ...(opts.include.dependencies ? lockfileImporter.dependencies : {}),
        ...(opts.include.optionalDependencies ? lockfileImporter.optionalDependencies : {}),
      }
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
  for (const alias of Object.keys(allDeps)) {
    const childDepPath = dp.refToAbsolute(allDeps[alias], alias, ctx.registries)
    if (childDepPath === null) {
      children[alias] = path.resolve(ctx.prefix, allDeps[alias].substr(5))
      continue
    }
    const childRelDepPath = dp.refToRelative(allDeps[alias], alias) as string
    const childPkgSnapshot = ctx.pkgSnapshotsByRelDepPaths[childRelDepPath]
    if (ctx.graph[childDepPath]) {
      children[alias] = ctx.graph[childDepPath].peripheralLocation
    } else if (childPkgSnapshot) {
      if (ctx.independentLeaves && packageIsIndependent(childPkgSnapshot)) {
        const pkgId = childPkgSnapshot.id || childDepPath
        const pkgName = nameVerFromPkgSnapshot(childRelDepPath, childPkgSnapshot).name
        const pkgLocation = await ctx.storeController.getPackageLocation(pkgId, pkgName, {
          lockfileDirectory: ctx.lockfileDirectory,
          targetEngine: ctx.sideEffectsCacheRead && !ctx.force && ENGINE_NAME || undefined,
        })
        children[alias] = pkgLocation.directory
      } else {
        const pkgName = nameVerFromPkgSnapshot(childRelDepPath, childPkgSnapshot).name
        children[alias] = path.join(ctx.virtualStoreDir, pkgIdToFilename(childDepPath, ctx.lockfileDirectory), 'node_modules', pkgName)
      }
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
  fetchingFiles: () => Promise<PackageFilesResponse>,
  finishing: () => Promise<void>,
  peripheralLocation: string,
  children: {[alias: string]: string},
  // an independent package is a package that
  // has neither regular nor peer dependencies
  independent: boolean,
  optionalDependencies: Set<string>,
  optional: boolean,
  relDepPath: string, // this option is only needed for saving pendingBuild when running with --ignore-scripts flag
  packageId: string, // TODO: this option is currently only needed when running postinstall scripts but even there it should be not used
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
      const filesResponse = await depNode.fetchingFiles()

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
          : Object.keys(depNode.children)
            .reduce((nonOptionalChildren, childAlias) => {
              if (!depNode.optionalDependencies.has(childAlias)) {
                nonOptionalChildren[childAlias] = depNode.children[childAlias]
              }
              return nonOptionalChildren
            }, {})

        const binPath = path.join(depNode.peripheralLocation, 'node_modules', '.bin')
        const pkgSnapshots = R.props<string, DependenciesGraphNode>(R.values(childrenToLink), depGraph)

        if (pkgSnapshots.includes(undefined as any)) { // tslint:disable-line
          await linkBins(depNode.modules, binPath, { warn: opts.warn })
        } else {
          const pkgs = await Promise.all(
            pkgSnapshots
              .filter(({ hasBin }) => hasBin)
              .map(async ({ peripheralLocation }) => ({
                location: peripheralLocation,
                manifest: await readPackageFromDir(peripheralLocation) as DependencyManifest,
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
  depNodes: DependenciesGraphNode[],
  opts: {
    optional: boolean,
    lockfileDirectory: string,
  },
) {
  return Promise.all(
    depNodes
      .filter(({ independent }) => !independent)
      .map(async (depNode) => {
        const childrenToLink = opts.optional
          ? depNode.children
          : Object.keys(depNode.children)
            .reduce((nonOptionalChildren, childAlias) => {
              if (!depNode.optionalDependencies.has(childAlias)) {
                nonOptionalChildren[childAlias] = depNode.children[childAlias]
              }
              return nonOptionalChildren
            }, {})

        await Promise.all(
          Object.keys(childrenToLink)
            .map(async (alias) => {
              // if (!pkg.installable && pkg.optional) return
              if (alias === depNode.name) {
                logger.warn({
                  message: `Cannot link dependency with name ${alias} to ${depNode.modules}. Dependency's name should differ from the parent's name.`,
                  prefix: opts.lockfileDirectory,
                })
                return
              }
              await limitLinking(() => symlinkDependency(childrenToLink[alias], depNode.modules, alias))
            }),
        )
      }),
  )
}
