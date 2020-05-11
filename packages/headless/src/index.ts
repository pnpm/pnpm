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
import { fromDir as readPackageFromDir } from '@pnpm/read-package-json'
import { readProjectManifestOnly } from '@pnpm/read-project-manifest'
import {
  PackageFilesResponse,
  StoreController,
} from '@pnpm/store-controller-types'
import symlinkDependency, { symlinkDirectRootDependency } from '@pnpm/symlink-dependency'
import { DependencyManifest, ProjectManifest, Registries } from '@pnpm/types'
import dp = require('dependency-path')
import fs = require('mz/fs')
import pLimit from 'p-limit'
import path = require('path')
import pathAbsolute = require('path-absolute')
import R = require('ramda')
import realpathMissing = require('realpath-missing')

const brokenModulesLogger = logger('_broken_node_modules')

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
  projects: Array<{
    binsDir: string,
    buildIndex: number,
    manifest: ProjectManifest,
    modulesDir: string,
    id: string,
    pruneDirectDependencies?: boolean,
    rootDir: string,
  }>,
  hoistedAliases: {[depPath: string]: string[]}
  hoistPattern?: string[],
  lockfileDir: string,
  modulesDir?: string,
  virtualStoreDir?: string,
  shamefullyHoist: boolean,
  storeController: StoreController,
  sideEffectsCacheRead: boolean,
  sideEffectsCacheWrite: boolean,
  force: boolean,
  storeDir: string,
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

  const lockfileDir = opts.lockfileDir
  const wantedLockfile = opts.wantedLockfile || await readWantedLockfile(lockfileDir, { ignoreIncompatible: false })

  if (!wantedLockfile) {
    throw new Error(`Headless installation requires a ${WANTED_LOCKFILE} file`)
  }

  const relativeModulesDir = opts.modulesDir ?? 'node_modules'
  const rootModulesDir = await realpathMissing(path.join(lockfileDir, relativeModulesDir))
  const virtualStoreDir = pathAbsolute(opts.virtualStoreDir ?? path.join(relativeModulesDir, '.pnpm'), lockfileDir)
  const currentLockfile = opts.currentLockfile || await readCurrentLockfile(virtualStoreDir, { ignoreIncompatible: false })
  const hoistedModulesDir = opts.shamefullyHoist
    ? rootModulesDir : path.join(virtualStoreDir, 'node_modules')

  for (const { id, manifest, rootDir } of opts.projects) {
    if (!satisfiesPackageManifest(wantedLockfile, manifest, id)) {
      throw new PnpmError('OUTDATED_LOCKFILE',
        `Cannot install with "frozen-lockfile" because ${WANTED_LOCKFILE} is not up-to-date with ` +
        path.relative(lockfileDir, path.join(rootDir, 'package.json')))
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
      opts.projects,
      opts.childConcurrency || 5,
      scriptsOpts,
    )
  }

  const skipped = opts.skipped || new Set<string>()
  if (currentLockfile) {
    await prune(
      opts.projects,
      {
        currentLockfile,
        dryRun: false,
        hoistedAliases: opts.hoistedAliases,
        hoistedModulesDir: opts.hoistPattern && hoistedModulesDir || undefined,
        include: opts.include,
        lockfileDir,
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
      prefix: lockfileDir,
      removed: 0,
    })
  }

  stageLogger.debug({
    prefix: lockfileDir,
    stage: 'importing_started',
  })

  const filterOpts = {
    include: opts.include,
    registries: opts.registries,
    skipped,
  }
  const filteredLockfile = filterLockfileByImportersAndEngine(wantedLockfile, opts.projects.map(({ id }) => id), {
    ...filterOpts,
    currentEngine: opts.currentEngine,
    engineStrict: opts.engineStrict,
    failOnMissingDependencies: true,
    includeIncompatiblePackages: opts.force === true,
    lockfileDir,
  })
  const { directDependenciesByImporterId, graph } = await lockfileToDepGraph(
    filteredLockfile,
    opts.force ? null : currentLockfile,
    {
      ...opts,
      importerIds: opts.projects.map(({ id }) => id),
      lockfileDir,
      skipped,
      virtualStoreDir,
    } as LockfileToDepGraphOptions,
  )
  const depNodes = R.values(graph)

  statsLogger.debug({
    added: depNodes.length,
    prefix: lockfileDir,
  })

  await Promise.all([
    linkAllModules(depNodes, {
      lockfileDir,
      optional: opts.include.optionalDependencies,
    }),
    linkAllPkgs(opts.storeController, depNodes, opts),
  ])

  stageLogger.debug({
    prefix: lockfileDir,
    stage: 'importing_done',
  })

  function warn (message: string) {
    logger.warn({
      message,
      prefix: lockfileDir,
    })
  }

  const rootImporterWithFlatModules = opts.hoistPattern && opts.projects.find(({ id }) => id === '.')
  let newHoistedAliases!: {[depPath: string]: string[]}
  if (rootImporterWithFlatModules) {
    newHoistedAliases = await hoist(matcher(opts.hoistPattern!), {
      getIndependentPackageLocation: opts.independentLeaves
        ? async (packageId: string, packageName: string) => {
          const { dir } = await opts.storeController.getPackageLocation(packageId, packageName, {
            lockfileDir,
            targetEngine: opts.sideEffectsCacheRead && ENGINE_NAME || undefined,
          })
          return dir
        }
        : undefined,
      lockfile: filteredLockfile,
      lockfileDir,
      modulesDir: hoistedModulesDir,
      registries: opts.registries,
      virtualStoreDir,
    })
  } else {
    newHoistedAliases = {}
  }

  await Promise.all(opts.projects.map(async ({ rootDir, id, manifest, modulesDir }) => {
    await linkRootPackages(filteredLockfile, {
      importerId: id,
      importerModulesDir: modulesDir,
      lockfileDir,
      projectDir: rootDir,
      projects: opts.projects,
      registries: opts.registries,
      rootDependencies: directDependenciesByImporterId[id],
    })

    // Even though headless installation will never update the package.json
    // this needs to be logged because otherwise install summary won't be printed
    packageManifestLogger.debug({
      prefix: rootDir,
      updated: manifest,
    })
  }))

  if (opts.ignoreScripts) {
    for (const { id, manifest } of opts.projects) {
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
          .map(({ depPath }) => depPath),
      )
  } else {
    const directNodes = new Set<string>()
    for (const { id } of opts.projects) {
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
      lockfileDir,
      optional: opts.include.optionalDependencies,
      rawConfig: opts.rawConfig,
      rootModulesDir: virtualStoreDir,
      sideEffectsCacheWrite: opts.sideEffectsCacheWrite,
      storeController: opts.storeController,
      unsafePerm: opts.unsafePerm,
      userAgent: opts.userAgent,
    })
  }

  await linkAllBins(graph, { optional: opts.include.optionalDependencies, warn })
  await Promise.all(opts.projects.map(linkBinsOfImporter))

  if (currentLockfile && !R.equals(opts.projects.map(({ id }) => id).sort(), Object.keys(filteredLockfile.importers).sort())) {
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
    storeDir: opts.storeDir,
    virtualStoreDir,
  })

  // waiting till package requests are finished
  await Promise.all(depNodes.map(({ finishing }) => finishing))

  summaryLogger.debug({ prefix: lockfileDir })

  await opts.storeController.close()

  if (!opts.ignoreScripts) {
    await runLifecycleHooksConcurrently(
      ['install', 'postinstall', 'prepublish', 'prepare'],
      opts.projects,
      opts.childConcurrency || 5,
      scriptsOpts,
    )
  }

  if (reporter) {
    streamParser.removeListener('data', reporter)
  }
}

function linkBinsOfImporter (
  { modulesDir, binsDir, rootDir }: {
    binsDir: string,
    modulesDir: string,
    rootDir: string,
  },
) {
  const warn = (message: string) => logger.warn({ message, prefix: rootDir })
  return linkBins(modulesDir, binsDir, {
    allowExoticManifests: true,
    warn,
  })
}

async function linkRootPackages (
  lockfile: Lockfile,
  opts: {
    registries: Registries,
    projectDir: string,
    importerId: string,
    importerModulesDir: string,
    projects: Array<{ id: string, manifest: ProjectManifest }>,
    lockfileDir: string,
    rootDependencies: {[alias: string]: string},
  },
) {
  const importerManifestsByImporterId = {} as { [id: string]: ProjectManifest }
  for (const { id, manifest } of opts.projects) {
    importerManifestsByImporterId[id] = manifest
  }
  const projectSnapshot = lockfile.importers[opts.importerId]
  const allDeps = {
    ...projectSnapshot.devDependencies,
    ...projectSnapshot.dependencies,
    ...projectSnapshot.optionalDependencies,
  }
  return Promise.all(
    Object.keys(allDeps)
      .map(async (alias) => {
        if (allDeps[alias].startsWith('link:')) {
          const isDev = projectSnapshot.devDependencies?.[alias]
          const isOptional = projectSnapshot.optionalDependencies?.[alias]
          const packageDir = path.join(opts.projectDir, allDeps[alias].substr(5))
          const linkedPackage = await (async () => {
            const importerId = getLockfileImporterId(opts.lockfileDir, packageDir)
            if (importerManifestsByImporterId[importerId]) {
              return importerManifestsByImporterId[importerId]
            }
            // TODO: cover this case with a test
            return await readProjectManifestOnly(packageDir) as DependencyManifest
          })() as DependencyManifest
          await symlinkDirectRootDependency(packageDir, opts.importerModulesDir, alias, {
            fromDependenciesField: isDev && 'devDependencies' ||
              isOptional && 'optionalDependencies' ||
              'dependencies',
            linkedPackage,
            prefix: opts.projectDir,
          })
          return
        }
        const peripheralLocation = opts.rootDependencies[alias]
        // Skipping linked packages
        if (!peripheralLocation) {
          return
        }
        if ((await symlinkDependency(peripheralLocation, opts.importerModulesDir, alias)).reused) {
          return
        }
        const isDev = projectSnapshot.devDependencies?.[alias]
        const isOptional = projectSnapshot.optionalDependencies?.[alias]

        const depPath = dp.refToRelative(allDeps[alias], alias)
        if (depPath === null) return
        const pkgSnapshot = lockfile.packages?.[depPath]
        if (!pkgSnapshot) return // this won't ever happen. Just making typescript happy
        const pkgId = pkgSnapshot.id || dp.refToAbsolute(allDeps[alias], alias, opts.registries) || undefined
        const pkgInfo = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
        rootLogger.debug({
          added: {
            dependencyType: isDev && 'dev' || isOptional && 'optional' || 'prod',
            id: pkgId,
            // latest: opts.outdatedPkgs[pkg.id],
            name: alias,
            realName: pkgInfo.name,
            version: pkgInfo.version,
          },
          prefix: opts.projectDir,
        })
      }),
  )
}

interface LockfileToDepGraphOptions {
  force: boolean,
  include: IncludedDependencies,
  independentLeaves: boolean,
  importerIds: string[],
  lockfileDir: string,
  skipped: Set<string>,
  storeController: StoreController,
  storeDir: string,
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
      Object.keys(lockfile.packages).map(async (depPath) => {
        const pkgSnapshot = lockfile.packages![depPath]
        // TODO: optimize. This info can be already returned by pkgSnapshotToResolution()
        const pkgName = nameVerFromPkgSnapshot(depPath, pkgSnapshot).name
        const modules = path.join(opts.virtualStoreDir, pkgIdToFilename(depPath, opts.lockfileDir), 'node_modules')
        const packageId = packageIdFromSnapshot(depPath, pkgSnapshot, opts.registries)
        const pkgLocation = await opts.storeController.getPackageLocation(packageId, pkgName, {
          lockfileDir: opts.lockfileDir,
          targetEngine: opts.sideEffectsCacheRead && !opts.force && ENGINE_NAME || undefined,
        })

        const independent = opts.independentLeaves && packageIsIndependent(pkgSnapshot)
        const peripheralLocation = !independent
          ? path.join(modules, pkgName)
          : pkgLocation.dir
        if (
          currentPackages[depPath] && R.equals(currentPackages[depPath].dependencies, lockfile.packages![depPath].dependencies) &&
          R.equals(currentPackages[depPath].optionalDependencies, lockfile.packages![depPath].optionalDependencies)
        ) {
          if (await fs.exists(peripheralLocation)) {
            return
          }

          brokenModulesLogger.debug({
            missing: peripheralLocation,
          })
        }
        const resolution = pkgSnapshotToResolution(depPath, pkgSnapshot, opts.registries)
        progressLogger.debug({
          packageId,
          requester: opts.lockfileDir,
          status: 'resolved',
        })
        let fetchResponse = opts.storeController.fetchPackage({
          force: false,
          lockfileDir: opts.lockfileDir,
          pkgId: packageId,
          resolution,
        })
        if (fetchResponse instanceof Promise) fetchResponse = await fetchResponse
        fetchResponse.files() // tslint:disable-line
          .then(({ fromStore }) => {
            progressLogger.debug({
              packageId,
              requester: opts.lockfileDir,
              status: fromStore
                ? 'found_in_store' : 'fetched',
            })
          })
          .catch(() => {
            // ignore
          })
        graph[peripheralLocation] = {
          children: {},
          depPath,
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
          requiresBuild: pkgSnapshot.requiresBuild === true,
        }
        pkgSnapshotByLocation[peripheralLocation] = pkgSnapshot
      }),
    )
    const ctx = {
      force: opts.force,
      graph,
      independentLeaves: opts.independentLeaves,
      lockfileDir: opts.lockfileDir,
      pkgSnapshotsByDepPaths: lockfile.packages,
      registries: opts.registries,
      sideEffectsCacheRead: opts.sideEffectsCacheRead,
      skipped: opts.skipped,
      storeController: opts.storeController,
      storeDir: opts.storeDir,
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
      const projectSnapshot = lockfile.importers[importerId]
      const rootDeps = {
        ...(opts.include.devDependencies ? projectSnapshot.devDependencies : {}),
        ...(opts.include.dependencies ? projectSnapshot.dependencies : {}),
        ...(opts.include.optionalDependencies ? projectSnapshot.optionalDependencies : {}),
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
    storeDir: string,
    skipped: Set<string>,
    pkgSnapshotsByDepPaths: Record<string, PackageSnapshot>,
    lockfileDir: string,
    sideEffectsCacheRead: boolean,
    storeController: StoreController,
  },
  allDeps: {[alias: string]: string},
) {
  const children: {[alias: string]: string} = {}
  for (const alias of Object.keys(allDeps)) {
    const childDepPath = dp.refToAbsolute(allDeps[alias], alias, ctx.registries)
    if (childDepPath === null) {
      children[alias] = path.resolve(ctx.lockfileDir, allDeps[alias].substr(5))
      continue
    }
    const childRelDepPath = dp.refToRelative(allDeps[alias], alias) as string
    const childPkgSnapshot = ctx.pkgSnapshotsByDepPaths[childRelDepPath]
    if (ctx.graph[childRelDepPath]) {
      children[alias] = ctx.graph[childRelDepPath].peripheralLocation
    } else if (childPkgSnapshot) {
      if (ctx.independentLeaves && packageIsIndependent(childPkgSnapshot)) {
        const pkgId = childPkgSnapshot.id || childDepPath
        const pkgName = nameVerFromPkgSnapshot(childRelDepPath, childPkgSnapshot).name
        const pkgLocation = await ctx.storeController.getPackageLocation(pkgId, pkgName, {
          lockfileDir: ctx.lockfileDir,
          targetEngine: ctx.sideEffectsCacheRead && !ctx.force && ENGINE_NAME || undefined,
        })
        children[alias] = pkgLocation.dir
      } else {
        const pkgName = nameVerFromPkgSnapshot(childRelDepPath, childPkgSnapshot).name
        children[alias] = path.join(ctx.virtualStoreDir, pkgIdToFilename(childRelDepPath, ctx.lockfileDir), 'node_modules', pkgName)
      }
    } else if (allDeps[alias].indexOf('file:') === 0) {
      children[alias] = path.resolve(ctx.lockfileDir, allDeps[alias].substr(5))
    } else if (!ctx.skipped.has(childRelDepPath)) {
      throw new Error(`${childRelDepPath} not found in ${WANTED_LOCKFILE}`)
    }
  }
  return children
}

export interface DependenciesGraphNode {
  hasBundledDependencies: boolean,
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
  depPath: string, // this option is only needed for saving pendingBuild when running with --ignore-scripts flag
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

      return storeController.importPackage(depNode.peripheralLocation, {
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
    lockfileDir: string,
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
                  prefix: opts.lockfileDir,
                })
                return
              }
              await limitLinking(() => symlinkDependency(childrenToLink[alias], depNode.modules, alias))
            }),
        )
      }),
  )
}
