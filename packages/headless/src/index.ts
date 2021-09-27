import { promises as fs } from 'fs'
import path from 'path'
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
import {
  runLifecycleHooksConcurrently,
  makeNodeRequireOption,
} from '@pnpm/lifecycle'
import linkBins, { linkBinsOfPackages } from '@pnpm/link-bins'
import {
  getLockfileImporterId,
  Lockfile,
  PackageSnapshot,
  readCurrentLockfile,
  readWantedLockfile,
  writeCurrentLockfile,
} from '@pnpm/lockfile-file'
import { writePnpFile } from '@pnpm/lockfile-to-pnp'
import {
  nameVerFromPkgSnapshot,
  packageIdFromSnapshot,
  pkgSnapshotToResolution,
  satisfiesPackageManifest,
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
import packageIsInstallable from '@pnpm/package-is-installable'
import { fromDir as readPackageFromDir } from '@pnpm/read-package-json'
import { readProjectManifestOnly, safeReadProjectManifestOnly } from '@pnpm/read-project-manifest'
import {
  FetchPackageToStoreFunction,
  PackageFilesResponse,
  StoreController,
} from '@pnpm/store-controller-types'
import symlinkDependency, { symlinkDirectRootDependency } from '@pnpm/symlink-dependency'
import { DependencyManifest, HoistedDependencies, ProjectManifest, Registries } from '@pnpm/types'
import * as dp from 'dependency-path'
import pLimit from 'p-limit'
import pathAbsolute from 'path-absolute'
import pathExists from 'path-exists'
import equals from 'ramda/src/equals'
import fromPairs from 'ramda/src/fromPairs'
import omit from 'ramda/src/omit'
import props from 'ramda/src/props'
import realpathMissing from 'realpath-missing'

const brokenModulesLogger = logger('_broken_node_modules')

export type ReporterFunction = (logObj: LogBase) => void

export interface HeadlessOptions {
  childConcurrency?: number
  currentLockfile?: Lockfile
  currentEngine: {
    nodeVersion: string
    pnpmVersion: string
  }
  enablePnp?: boolean
  engineStrict: boolean
  extendNodePath?: boolean
  extraBinPaths?: string[]
  ignoreScripts: boolean
  ignorePackageManifest?: boolean
  include: IncludedDependencies
  projects: Array<{
    binsDir: string
    buildIndex: number
    manifest: ProjectManifest
    modulesDir: string
    id: string
    pruneDirectDependencies?: boolean
    rootDir: string
  }>
  prunedAt?: string
  hoistedDependencies: HoistedDependencies
  hoistPattern?: string[]
  publicHoistPattern?: string[]
  lockfileDir: string
  modulesDir?: string
  virtualStoreDir?: string
  scriptShell?: string
  shellEmulator?: boolean
  storeController: StoreController
  sideEffectsCacheRead: boolean
  sideEffectsCacheWrite: boolean
  symlink?: boolean
  force: boolean
  storeDir: string
  rawConfig: object
  unsafePerm: boolean
  userAgent: string
  registries: Registries
  reporter?: ReporterFunction
  packageManager: {
    name: string
    version: string
  }
  pruneStore: boolean
  pruneVirtualStore?: boolean
  wantedLockfile?: Lockfile
  ownLifecycleHooksStdio?: 'inherit' | 'pipe'
  pendingBuilds: string[]
  skipped: Set<string>
  enableModulesDir?: boolean
}

export default async (opts: HeadlessOptions) => {
  const reporter = opts.reporter
  if ((reporter != null) && typeof reporter === 'function') {
    streamParser.on('data', reporter)
  }

  const lockfileDir = opts.lockfileDir
  const wantedLockfile = opts.wantedLockfile ?? await readWantedLockfile(lockfileDir, { ignoreIncompatible: false })

  if (wantedLockfile == null) {
    throw new Error(`Headless installation requires a ${WANTED_LOCKFILE} file`)
  }

  const relativeModulesDir = opts.modulesDir ?? 'node_modules'
  const rootModulesDir = await realpathMissing(path.join(lockfileDir, relativeModulesDir))
  const virtualStoreDir = pathAbsolute(opts.virtualStoreDir ?? path.join(relativeModulesDir, '.pnpm'), lockfileDir)
  const currentLockfile = opts.currentLockfile ?? await readCurrentLockfile(virtualStoreDir, { ignoreIncompatible: false })
  const hoistedModulesDir = path.join(virtualStoreDir, 'node_modules')
  const publicHoistedModulesDir = rootModulesDir

  if (!opts.ignorePackageManifest) {
    for (const { id, manifest, rootDir } of opts.projects) {
      if (!satisfiesPackageManifest(wantedLockfile, manifest, id)) {
        throw new PnpmError('OUTDATED_LOCKFILE',
          `Cannot install with "frozen-lockfile" because ${WANTED_LOCKFILE} is not up-to-date with ` +
          path.relative(lockfileDir, path.join(rootDir, 'package.json')))
      }
    }
  }

  const scriptsOpts = {
    optional: false,
    extraBinPaths: opts.extraBinPaths,
    rawConfig: opts.rawConfig,
    scriptShell: opts.scriptShell,
    shellEmulator: opts.shellEmulator,
    stdio: opts.ownLifecycleHooksStdio ?? 'inherit',
    unsafePerm: opts.unsafePerm || false,
  }

  const skipped = opts.skipped || new Set<string>()
  if (currentLockfile != null && !opts.ignorePackageManifest) {
    await prune(
      opts.projects,
      {
        currentLockfile,
        dryRun: false,
        hoistedDependencies: opts.hoistedDependencies,
        hoistedModulesDir: (opts.hoistPattern == null) ? undefined : hoistedModulesDir,
        include: opts.include,
        lockfileDir,
        pruneStore: opts.pruneStore,
        pruneVirtualStore: opts.pruneVirtualStore,
        publicHoistedModulesDir: (opts.publicHoistPattern == null) ? undefined : publicHoistedModulesDir,
        registries: opts.registries,
        skipped,
        storeController: opts.storeController,
        virtualStoreDir,
        wantedLockfile,
      }
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
  const importerIds = opts.ignorePackageManifest
    ? Object.keys(wantedLockfile.importers)
    : opts.projects.map(({ id }) => id)
  const filteredLockfile = filterLockfileByImportersAndEngine(wantedLockfile, importerIds, {
    ...filterOpts,
    currentEngine: opts.currentEngine,
    engineStrict: opts.engineStrict,
    failOnMissingDependencies: true,
    includeIncompatiblePackages: opts.force,
    lockfileDir,
  })
  const { directDependenciesByImporterId, graph } = await lockfileToDepGraph(
    filteredLockfile,
    opts.force ? null : currentLockfile,
    {
      ...opts,
      importerIds,
      lockfileDir,
      skipped,
      virtualStoreDir,
      nodeVersion: opts.currentEngine.nodeVersion,
      pnpmVersion: opts.currentEngine.pnpmVersion,
    } as LockfileToDepGraphOptions
  )
  if (opts.enablePnp) {
    const importerNames = fromPairs(
      opts.projects.map(({ manifest, id }) => [id, manifest.name ?? id])
    )
    await writePnpFile(filteredLockfile, {
      importerNames,
      lockfileDir,
      virtualStoreDir,
      registries: opts.registries,
    })
  }
  const depNodes = Object.values(graph)

  statsLogger.debug({
    added: depNodes.length,
    prefix: lockfileDir,
  })

  function warn (message: string) {
    logger.info({
      message,
      prefix: lockfileDir,
    })
  }

  if (opts.enableModulesDir !== false) {
    await Promise.all(depNodes.map(async (depNode) => fs.mkdir(depNode.modules, { recursive: true })))
    await Promise.all([
      opts.symlink === false
        ? Promise.resolve()
        : linkAllModules(depNodes, {
          lockfileDir,
          optional: opts.include.optionalDependencies,
        }),
      linkAllPkgs(opts.storeController, depNodes, {
        force: opts.force,
        lockfileDir: opts.lockfileDir,
        targetEngine: opts.sideEffectsCacheRead && ENGINE_NAME || undefined,
      }),
    ])

    stageLogger.debug({
      prefix: lockfileDir,
      stage: 'importing_done',
    })

    let newHoistedDependencies!: HoistedDependencies
    if (opts.ignorePackageManifest !== true && (opts.hoistPattern != null || opts.publicHoistPattern != null)) {
      // It is important to keep the skipped packages in the lockfile which will be saved as the "current lockfile".
      // pnpm is comparing the current lockfile to the wanted one and they should much.
      // But for hoisting, we need a version of the lockfile w/o the skipped packages, so we're making a copy.
      const hoistLockfile = {
        ...filteredLockfile,
        packages: omit(Array.from(skipped), filteredLockfile.packages),
      }
      newHoistedDependencies = await hoist({
        extendNodePath: opts.extendNodePath,
        lockfile: hoistLockfile,
        lockfileDir,
        privateHoistedModulesDir: hoistedModulesDir,
        privateHoistPattern: opts.hoistPattern ?? [],
        publicHoistedModulesDir,
        publicHoistPattern: opts.publicHoistPattern ?? [],
        virtualStoreDir,
      })
    } else {
      newHoistedDependencies = {}
    }

    if (opts.ignoreScripts) {
      for (const { id, manifest } of opts.projects) {
        if (opts.ignoreScripts && ((manifest?.scripts) != null) &&
          (manifest.scripts.preinstall ?? manifest.scripts.prepublish ??
            manifest.scripts.install ??
            manifest.scripts.postinstall ??
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
            .map(({ depPath }) => depPath)
        )
    } else {
      const directNodes = new Set<string>()
      for (const id of importerIds) {
        Object
          .values(directDependenciesByImporterId[id])
          .filter((loc) => graph[loc])
          .forEach((loc) => {
            directNodes.add(loc)
          })
      }
      const extraBinPaths = [...opts.extraBinPaths ?? []]
      if (opts.hoistPattern != null) {
        extraBinPaths.unshift(path.join(virtualStoreDir, 'node_modules/.bin'))
      }
      let extraEnv: Record<string, string> | undefined
      if (opts.enablePnp) {
        extraEnv = makeNodeRequireOption(path.join(opts.lockfileDir, '.pnp.cjs'))
      }
      await buildModules(graph, Array.from(directNodes), {
        childConcurrency: opts.childConcurrency,
        extraBinPaths,
        extendNodePath: opts.extendNodePath,
        extraEnv,
        lockfileDir,
        optional: opts.include.optionalDependencies,
        rawConfig: opts.rawConfig,
        rootModulesDir: virtualStoreDir,
        scriptShell: opts.scriptShell,
        shellEmulator: opts.shellEmulator,
        sideEffectsCacheWrite: opts.sideEffectsCacheWrite,
        storeController: opts.storeController,
        unsafePerm: opts.unsafePerm,
        userAgent: opts.userAgent,
      })
    }

    await writeModulesYaml(rootModulesDir, {
      hoistedDependencies: newHoistedDependencies,
      hoistPattern: opts.hoistPattern,
      included: opts.include,
      layoutVersion: LAYOUT_VERSION,
      packageManager: `${opts.packageManager.name}@${opts.packageManager.version}`,
      pendingBuilds: opts.pendingBuilds,
      publicHoistPattern: opts.publicHoistPattern,
      prunedAt: opts.pruneVirtualStore === true || opts.prunedAt == null
        ? new Date().toUTCString()
        : opts.prunedAt,
      registries: opts.registries,
      skipped: Array.from(skipped),
      storeDir: opts.storeDir,
      virtualStoreDir,
    })

    await linkAllBins(graph, { extendNodePath: opts.extendNodePath, optional: opts.include.optionalDependencies, warn })

    if ((currentLockfile != null) && !equals(importerIds.sort(), Object.keys(filteredLockfile.importers).sort())) {
      Object.assign(filteredLockfile.packages, currentLockfile.packages)
    }
    await writeCurrentLockfile(virtualStoreDir, filteredLockfile)

    /** Skip linking and due to no project manifest */
    if (!opts.ignorePackageManifest) {
      await Promise.all(opts.projects.map(async ({ rootDir, id, manifest, modulesDir }) => {
        if (opts.symlink !== false) {
          await linkRootPackages(filteredLockfile, {
            importerId: id,
            importerModulesDir: modulesDir,
            lockfileDir,
            projectDir: rootDir,
            projects: opts.projects,
            registries: opts.registries,
            rootDependencies: directDependenciesByImporterId[id],
          })
        }

        // Even though headless installation will never update the package.json
        // this needs to be logged because otherwise install summary won't be printed
        packageManifestLogger.debug({
          prefix: rootDir,
          updated: manifest,
        })
      }))

      await Promise.all(opts.projects.map(async (project) => {
        if (opts.publicHoistPattern?.length && path.relative(opts.lockfileDir, project.rootDir) === '') {
          await linkBinsOfImporter(project, { extendNodePath: opts.extendNodePath })
        } else {
          const directPkgDirs = Object.values(directDependenciesByImporterId[project.id])
          await linkBinsOfPackages(
            (
              await Promise.all(
                directPkgDirs.map(async (dir) => ({
                  location: dir,
                  manifest: await safeReadProjectManifestOnly(dir),
                }))
              )
            )
              .filter(({ manifest }) => manifest != null) as Array<{ location: string, manifest: DependencyManifest }>,
            project.binsDir,
            { warn: (message: string) => logger.info({ message, prefix: project.rootDir }) }
          )
        }
      }))
    }
  }
  // waiting till package requests are finished
  await Promise.all(depNodes.map(({ finishing }) => finishing))

  summaryLogger.debug({ prefix: lockfileDir })

  await opts.storeController.close()

  if (!opts.ignoreScripts && !opts.ignorePackageManifest) {
    await runLifecycleHooksConcurrently(
      ['preinstall', 'install', 'postinstall', 'prepublish', 'prepare'],
      opts.projects,
      opts.childConcurrency ?? 5,
      scriptsOpts
    )
  }

  if ((reporter != null) && typeof reporter === 'function') {
    streamParser.removeListener('data', reporter)
  }
}

async function linkBinsOfImporter (
  { manifest, modulesDir, binsDir, rootDir }: {
    binsDir: string
    manifest: ProjectManifest
    modulesDir: string
    rootDir: string
  },
  opts: {
    extendNodePath?: boolean
  }
) {
  const warn = (message: string) => logger.info({ message, prefix: rootDir })
  return linkBins(modulesDir, binsDir, {
    allowExoticManifests: true,
    extendNodePath: opts.extendNodePath,
    projectManifest: manifest,
    warn,
  })
}

async function linkRootPackages (
  lockfile: Lockfile,
  opts: {
    registries: Registries
    projectDir: string
    importerId: string
    importerModulesDir: string
    projects: Array<{ id: string, manifest: ProjectManifest }>
    lockfileDir: string
    rootDependencies: {[alias: string]: string}
  }
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
          const isDev = Boolean(projectSnapshot.devDependencies?.[alias])
          const isOptional = Boolean(projectSnapshot.optionalDependencies?.[alias])
          const packageDir = path.join(opts.projectDir, allDeps[alias].substr(5))
          const linkedPackage = await (async () => {
            const importerId = getLockfileImporterId(opts.lockfileDir, packageDir)
            if (importerManifestsByImporterId[importerId]) {
              return importerManifestsByImporterId[importerId]
            }
            try {
              // TODO: cover this case with a test
              return await readProjectManifestOnly(packageDir) as DependencyManifest
            } catch (err) {
              if (err['code'] !== 'ERR_PNPM_NO_IMPORTER_MANIFEST_FOUND') throw err
              return { name: alias, version: '0.0.0' }
            }
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
        const dir = opts.rootDependencies[alias]
        // Skipping linked packages
        if (!dir) {
          return
        }
        if ((await symlinkDependency(dir, opts.importerModulesDir, alias)).reused) {
          return
        }
        const isDev = Boolean(projectSnapshot.devDependencies?.[alias])
        const isOptional = Boolean(projectSnapshot.optionalDependencies?.[alias])

        const depPath = dp.refToRelative(allDeps[alias], alias)
        if (depPath === null) return
        const pkgSnapshot = lockfile.packages?.[depPath]
        if (pkgSnapshot == null) return // this won't ever happen. Just making typescript happy
        const pkgId = pkgSnapshot.id ?? dp.refToAbsolute(allDeps[alias], alias, opts.registries) ?? undefined
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
      })
  )
}

interface LockfileToDepGraphOptions {
  engineStrict: boolean
  force: boolean
  importerIds: string[]
  include: IncludedDependencies
  lockfileDir: string
  nodeVersion: string
  pnpmVersion: string
  registries: Registries
  sideEffectsCacheRead: boolean
  skipped: Set<string>
  storeController: StoreController
  storeDir: string
  virtualStoreDir: string
}

async function lockfileToDepGraph (
  lockfile: Lockfile,
  currentLockfile: Lockfile | null,
  opts: LockfileToDepGraphOptions
) {
  const currentPackages = currentLockfile?.packages ?? {}
  const graph: DependenciesGraph = {}
  const directDependenciesByImporterId: { [importerId: string]: { [alias: string]: string } } = {}
  if (lockfile.packages != null) {
    const pkgSnapshotByLocation = {}
    await Promise.all(
      Object.keys(lockfile.packages).map(async (depPath) => {
        if (opts.skipped.has(depPath)) return
        const pkgSnapshot = lockfile.packages![depPath]
        // TODO: optimize. This info can be already returned by pkgSnapshotToResolution()
        const { name: pkgName, version: pkgVersion } = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
        const modules = path.join(opts.virtualStoreDir, dp.depPathToFilename(depPath, opts.lockfileDir), 'node_modules')
        const packageId = packageIdFromSnapshot(depPath, pkgSnapshot, opts.registries)

        const pkg = {
          name: pkgName,
          version: pkgVersion,
          engines: pkgSnapshot.engines,
          cpu: pkgSnapshot.cpu,
          os: pkgSnapshot.os,
        }
        if (!opts.force &&
          packageIsInstallable(packageId, pkg, {
            engineStrict: opts.engineStrict,
            lockfileDir: opts.lockfileDir,
            nodeVersion: opts.nodeVersion,
            optional: pkgSnapshot.optional === true,
            pnpmVersion: opts.pnpmVersion,
          }) === false
        ) {
          opts.skipped.add(depPath)
          return
        }
        const dir = path.join(modules, pkgName)
        if (
          currentPackages[depPath] && equals(currentPackages[depPath].dependencies, lockfile.packages![depPath].dependencies) &&
          equals(currentPackages[depPath].optionalDependencies, lockfile.packages![depPath].optionalDependencies)
        ) {
          if (await pathExists(dir)) {
            return
          }

          brokenModulesLogger.debug({
            missing: dir,
          })
        }
        const resolution = pkgSnapshotToResolution(depPath, pkgSnapshot, opts.registries)
        progressLogger.debug({
          packageId,
          requester: opts.lockfileDir,
          status: 'resolved',
        })
        let fetchResponse!: ReturnType<FetchPackageToStoreFunction>
        try {
          fetchResponse = opts.storeController.fetchPackage({
            force: false,
            lockfileDir: opts.lockfileDir,
            pkg: {
              name: pkgName,
              version: pkgVersion,
              id: packageId,
              resolution,
            },
          })
          if (fetchResponse instanceof Promise) fetchResponse = await fetchResponse
        } catch (err) {
          if (pkgSnapshot.optional) return
          throw err
        }
        fetchResponse.files() // eslint-disable-line
          .then(({ fromStore }) => {
            progressLogger.debug({
              packageId,
              requester: opts.lockfileDir,
              status: fromStore
                ? 'found_in_store'
                : 'fetched',
            })
          })
          .catch(() => {
            // ignore
          })
        graph[dir] = {
          children: {},
          depPath,
          dir,
          fetchingFiles: fetchResponse.files,
          filesIndexFile: fetchResponse.filesIndexFile,
          finishing: fetchResponse.finishing,
          hasBin: pkgSnapshot.hasBin === true,
          hasBundledDependencies: pkgSnapshot.bundledDependencies != null,
          modules,
          name: pkgName,
          optional: !!pkgSnapshot.optional,
          optionalDependencies: new Set(Object.keys(pkgSnapshot.optionalDependencies ?? {})),
          prepare: pkgSnapshot.prepare === true,
          requiresBuild: pkgSnapshot.requiresBuild === true,
        }
        pkgSnapshotByLocation[dir] = pkgSnapshot
      })
    )
    const ctx = {
      force: opts.force,
      graph,
      lockfileDir: opts.lockfileDir,
      pkgSnapshotsByDepPaths: lockfile.packages,
      registries: opts.registries,
      sideEffectsCacheRead: opts.sideEffectsCacheRead,
      skipped: opts.skipped,
      storeController: opts.storeController,
      storeDir: opts.storeDir,
      virtualStoreDir: opts.virtualStoreDir,
    }
    for (const dir of Object.keys(graph)) {
      const pkgSnapshot = pkgSnapshotByLocation[dir]
      const allDeps = {
        ...pkgSnapshot.dependencies,
        ...(opts.include.optionalDependencies ? pkgSnapshot.optionalDependencies : {}),
      }

      const peerDeps = pkgSnapshot.peerDependencies ? new Set(Object.keys(pkgSnapshot.peerDependencies)) : null
      graph[dir].children = await getChildrenPaths(ctx, allDeps, peerDeps, '.')
    }
    for (const importerId of opts.importerIds) {
      const projectSnapshot = lockfile.importers[importerId]
      const rootDeps = {
        ...(opts.include.devDependencies ? projectSnapshot.devDependencies : {}),
        ...(opts.include.dependencies ? projectSnapshot.dependencies : {}),
        ...(opts.include.optionalDependencies ? projectSnapshot.optionalDependencies : {}),
      }
      directDependenciesByImporterId[importerId] = await getChildrenPaths(ctx, rootDeps, null, importerId)
    }
  }
  return { graph, directDependenciesByImporterId }
}

async function getChildrenPaths (
  ctx: {
    graph: DependenciesGraph
    force: boolean
    registries: Registries
    virtualStoreDir: string
    storeDir: string
    skipped: Set<string>
    pkgSnapshotsByDepPaths: Record<string, PackageSnapshot>
    lockfileDir: string
    sideEffectsCacheRead: boolean
    storeController: StoreController
  },
  allDeps: {[alias: string]: string},
  peerDeps: Set<string> | null,
  importerId: string
) {
  const children: {[alias: string]: string} = {}
  for (const alias of Object.keys(allDeps)) {
    const childDepPath = dp.refToAbsolute(allDeps[alias], alias, ctx.registries)
    if (childDepPath === null) {
      children[alias] = path.resolve(ctx.lockfileDir, importerId, allDeps[alias].substr(5))
      continue
    }
    const childRelDepPath = dp.refToRelative(allDeps[alias], alias) as string
    const childPkgSnapshot = ctx.pkgSnapshotsByDepPaths[childRelDepPath]
    if (ctx.graph[childRelDepPath]) {
      children[alias] = ctx.graph[childRelDepPath].dir
    } else if (childPkgSnapshot) {
      if (ctx.skipped.has(childRelDepPath)) continue
      const pkgName = nameVerFromPkgSnapshot(childRelDepPath, childPkgSnapshot).name
      children[alias] = path.join(ctx.virtualStoreDir, dp.depPathToFilename(childRelDepPath, ctx.lockfileDir), 'node_modules', pkgName)
    } else if (allDeps[alias].indexOf('file:') === 0) {
      children[alias] = path.resolve(ctx.lockfileDir, allDeps[alias].substr(5))
    } else if (!ctx.skipped.has(childRelDepPath) && ((peerDeps == null) || !peerDeps.has(alias))) {
      throw new Error(`${childRelDepPath} not found in ${WANTED_LOCKFILE}`)
    }
  }
  return children
}

export interface DependenciesGraphNode {
  hasBundledDependencies: boolean
  modules: string
  name: string
  fetchingFiles: () => Promise<PackageFilesResponse>
  finishing: () => Promise<void>
  dir: string
  children: {[alias: string]: string}
  optionalDependencies: Set<string>
  optional: boolean
  depPath: string // this option is only needed for saving pendingBuild when running with --ignore-scripts flag
  isBuilt?: boolean
  requiresBuild: boolean
  prepare: boolean
  hasBin: boolean
  filesIndexFile: string
}

export interface DependenciesGraph {
  [depPath: string]: DependenciesGraphNode
}

const limitLinking = pLimit(16)

async function linkAllPkgs (
  storeController: StoreController,
  depNodes: DependenciesGraphNode[],
  opts: {
    force: boolean
    lockfileDir: string
    targetEngine?: string
  }
) {
  return Promise.all(
    depNodes.map(async (depNode) => {
      let filesResponse!: PackageFilesResponse
      try {
        filesResponse = await depNode.fetchingFiles()
      } catch (err) {
        if (depNode.optional) return
        throw err
      }

      const { importMethod, isBuilt } = await storeController.importPackage(depNode.dir, {
        filesResponse,
        force: opts.force,
        targetEngine: opts.targetEngine,
      })
      if (importMethod) {
        progressLogger.debug({
          method: importMethod,
          requester: opts.lockfileDir,
          status: 'imported',
          to: depNode.dir,
        })
      }
      depNode.isBuilt = isBuilt
    })
  )
}

async function linkAllBins (
  depGraph: DependenciesGraph,
  opts: {
    extendNodePath?: boolean
    optional: boolean
    warn: (message: string) => void
  }
) {
  return Promise.all(
    Object.values(depGraph)
      .map(async (depNode) => limitLinking(async () => {
        const childrenToLink = opts.optional
          ? depNode.children
          : Object.keys(depNode.children)
            .reduce((nonOptionalChildren, childAlias) => {
              if (!depNode.optionalDependencies.has(childAlias)) {
                nonOptionalChildren[childAlias] = depNode.children[childAlias]
              }
              return nonOptionalChildren
            }, {})

        const binPath = path.join(depNode.dir, 'node_modules/.bin')
        const pkgSnapshots = props<string, DependenciesGraphNode>(Object.values(childrenToLink), depGraph)

        if (pkgSnapshots.includes(undefined as any)) { // eslint-disable-line
          await linkBins(depNode.modules, binPath, { extendNodePath: opts.extendNodePath, warn: opts.warn })
        } else {
          const pkgs = await Promise.all(
            pkgSnapshots
              .filter(({ hasBin }) => hasBin)
              .map(async ({ dir }) => ({
                location: dir,
                manifest: await readPackageFromDir(dir) as DependencyManifest,
              }))
          )

          await linkBinsOfPackages(pkgs, binPath, { extendNodePath: opts.extendNodePath, warn: opts.warn })
        }

        // link also the bundled dependencies` bins
        if (depNode.hasBundledDependencies) {
          const bundledModules = path.join(depNode.dir, 'node_modules')
          await linkBins(bundledModules, binPath, { extendNodePath: opts.extendNodePath, warn: opts.warn })
        }
      }))
  )
}

async function linkAllModules (
  depNodes: DependenciesGraphNode[],
  opts: {
    optional: boolean
    lockfileDir: string
  }
) {
  await Promise.all(
    depNodes
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
              await limitLinking(async () => symlinkDependency(childrenToLink[alias], depNode.modules, alias))
            })
        )
      })
  )
}
