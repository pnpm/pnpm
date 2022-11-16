import { promises as fs } from 'fs'
import path from 'path'
import { buildModules } from '@pnpm/build-modules'
import { calcDepState, DepsStateCache } from '@pnpm/calc-dep-state'
import {
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
import { PnpmError } from '@pnpm/error'
import {
  filterLockfileByImportersAndEngine,
} from '@pnpm/filter-lockfile'
import { hoist } from '@pnpm/hoist'
import {
  runLifecycleHooksConcurrently,
  makeNodeRequireOption,
} from '@pnpm/lifecycle'
import { linkBins, linkBinsOfPackages } from '@pnpm/link-bins'
import {
  getLockfileImporterId,
  Lockfile,
  readCurrentLockfile,
  readWantedLockfile,
  writeCurrentLockfile,
  PatchFile,
} from '@pnpm/lockfile-file'
import { writePnpFile } from '@pnpm/lockfile-to-pnp'
import {
  extendProjectsWithTargetDirs,
  nameVerFromPkgSnapshot,
  satisfiesPackageManifest,
} from '@pnpm/lockfile-utils'
import {
  LogBase,
  logger,
  streamParser,
} from '@pnpm/logger'
import { prune } from '@pnpm/modules-cleaner'
import {
  IncludedDependencies,
  writeModulesManifest,
} from '@pnpm/modules-yaml'
import { HoistingLimits } from '@pnpm/real-hoist'
import { readPackageJsonFromDir } from '@pnpm/read-package-json'
import { readProjectManifestOnly, safeReadProjectManifestOnly } from '@pnpm/read-project-manifest'
import {
  PackageFilesResponse,
  StoreController,
} from '@pnpm/store-controller-types'
import { symlinkDependency, symlinkDirectRootDependency } from '@pnpm/symlink-dependency'
import { DependencyManifest, HoistedDependencies, ProjectManifest, Registries } from '@pnpm/types'
import * as dp from 'dependency-path'
import pLimit from 'p-limit'
import pathAbsolute from 'path-absolute'
import equals from 'ramda/src/equals'
import fromPairs from 'ramda/src/fromPairs'
import isEmpty from 'ramda/src/isEmpty'
import omit from 'ramda/src/omit'
import pick from 'ramda/src/pick'
import props from 'ramda/src/props'
import union from 'ramda/src/union'
import realpathMissing from 'realpath-missing'
import { linkHoistedModules } from './linkHoistedModules'
import {
  DirectDependenciesByImporterId,
  DependenciesGraph,
  DependenciesGraphNode,
  LockfileToDepGraphOptions,
  lockfileToDepGraph,
} from './lockfileToDepGraph'
import { lockfileToHoistedDepGraph } from './lockfileToHoistedDepGraph'

export { HoistingLimits }

export type ReporterFunction = (logObj: LogBase) => void

export interface Project {
  binsDir: string
  buildIndex: number
  manifest: ProjectManifest
  modulesDir: string
  id: string
  pruneDirectDependencies?: boolean
  rootDir: string
}

export interface HeadlessOptions {
  autoInstallPeers?: boolean
  childConcurrency?: number
  currentLockfile?: Lockfile
  currentEngine: {
    nodeVersion: string
    pnpmVersion: string
  }
  enablePnp?: boolean
  engineStrict: boolean
  extraBinPaths?: string[]
  extraEnv?: Record<string, string>
  extraNodePaths?: string[]
  preferSymlinkedExecutables?: boolean
  hoistingLimits?: HoistingLimits
  ignoreDepScripts: boolean
  ignoreScripts: boolean
  ignorePackageManifest?: boolean
  include: IncludedDependencies
  selectedProjectDirs: string[]
  allProjects: Record<string, Project>
  prunedAt?: string
  hoistedDependencies: HoistedDependencies
  hoistPattern?: string[]
  publicHoistPattern?: string[]
  lockfileDir: string
  modulesDir?: string
  virtualStoreDir?: string
  patchedDependencies?: Record<string, PatchFile>
  scriptsPrependNodePath?: boolean | 'warn-only'
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
  resolveSymlinksInInjectedDirs?: boolean
  skipped: Set<string>
  enableModulesDir?: boolean
  nodeLinker?: 'isolated' | 'hoisted' | 'pnp'
  useGitBranchLockfile?: boolean
}

export async function headlessInstall (opts: HeadlessOptions) {
  const reporter = opts.reporter
  if ((reporter != null) && typeof reporter === 'function') {
    streamParser.on('data', reporter)
  }

  const lockfileDir = opts.lockfileDir
  const wantedLockfile = opts.wantedLockfile ?? await readWantedLockfile(lockfileDir, {
    ignoreIncompatible: false,
    useGitBranchLockfile: opts.useGitBranchLockfile,
    // mergeGitBranchLockfiles is intentionally not supported in headless
    mergeGitBranchLockfiles: false,
  })

  if (wantedLockfile == null) {
    throw new Error(`Headless installation requires a ${WANTED_LOCKFILE} file`)
  }

  const depsStateCache: DepsStateCache = {}
  const relativeModulesDir = opts.modulesDir ?? 'node_modules'
  const rootModulesDir = await realpathMissing(path.join(lockfileDir, relativeModulesDir))
  const virtualStoreDir = pathAbsolute(opts.virtualStoreDir ?? path.join(relativeModulesDir, '.pnpm'), lockfileDir)
  const currentLockfile = opts.currentLockfile ?? await readCurrentLockfile(virtualStoreDir, { ignoreIncompatible: false })
  const hoistedModulesDir = path.join(virtualStoreDir, 'node_modules')
  const publicHoistedModulesDir = rootModulesDir
  const selectedProjects = Object.values(pick(opts.selectedProjectDirs, opts.allProjects))

  if (!opts.ignorePackageManifest) {
    for (const { id, manifest, rootDir } of selectedProjects) {
      if (!satisfiesPackageManifest(wantedLockfile, manifest, id, { autoInstallPeers: opts.autoInstallPeers })) {
        throw new PnpmError('OUTDATED_LOCKFILE',
          `Cannot install with "frozen-lockfile" because ${WANTED_LOCKFILE} is not up to date with ` +
          path.relative(lockfileDir, path.join(rootDir, 'package.json')), {
            hint: 'Note that in CI environments this setting is true by default. If you still need to run install in such cases, use "pnpm install --no-frozen-lockfile"',
          })
      }
    }
  }

  const scriptsOpts = {
    optional: false,
    extraBinPaths: opts.extraBinPaths,
    extraEnv: opts.extraEnv,
    rawConfig: opts.rawConfig,
    resolveSymlinksInInjectedDirs: opts.resolveSymlinksInInjectedDirs,
    scriptsPrependNodePath: opts.scriptsPrependNodePath,
    scriptShell: opts.scriptShell,
    shellEmulator: opts.shellEmulator,
    stdio: opts.ownLifecycleHooksStdio ?? 'inherit',
    storeController: opts.storeController,
    unsafePerm: opts.unsafePerm || false,
  }

  const skipped = opts.skipped || new Set<string>()
  if (opts.nodeLinker !== 'hoisted') {
    if (currentLockfile != null && !opts.ignorePackageManifest) {
      await prune(
        selectedProjects,
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
  const initialImporterIds = (opts.ignorePackageManifest === true || opts.nodeLinker === 'hoisted')
    ? Object.keys(wantedLockfile.importers)
    : selectedProjects.map(({ id }) => id)
  const { lockfile: filteredLockfile, selectedImporterIds: importerIds } = filterLockfileByImportersAndEngine(wantedLockfile, initialImporterIds, {
    ...filterOpts,
    currentEngine: opts.currentEngine,
    engineStrict: opts.engineStrict,
    failOnMissingDependencies: true,
    includeIncompatiblePackages: opts.force,
    lockfileDir,
  })

  // Update selectedProjects to add missing projects. importerIds will have the updated ids, found from deeply linked workspace projects
  const initialImporterIdSet = new Set(initialImporterIds)
  const missingIds = importerIds.filter((importerId) => !initialImporterIdSet.has(importerId))
  if (missingIds.length > 0) {
    for (const project of Object.values(opts.allProjects)) {
      if (missingIds.includes(project.id)) {
        selectedProjects.push(project)
      }
    }
  }

  const lockfileToDepGraphOpts = {
    ...opts,
    importerIds,
    lockfileDir,
    skipped,
    virtualStoreDir,
    nodeVersion: opts.currentEngine.nodeVersion,
    pnpmVersion: opts.currentEngine.pnpmVersion,
  } as LockfileToDepGraphOptions
  const {
    directDependenciesByImporterId,
    graph,
    hierarchy,
    pkgLocationsByDepPath,
    prevGraph,
    symlinkedDirectDependenciesByImporterId,
  } = await (
    opts.nodeLinker === 'hoisted'
      ? lockfileToHoistedDepGraph(
        filteredLockfile,
        currentLockfile,
        lockfileToDepGraphOpts
      )
      : lockfileToDepGraph(
        filteredLockfile,
        opts.force ? null : currentLockfile,
        lockfileToDepGraphOpts
      )
  )
  if (opts.enablePnp) {
    const importerNames = fromPairs(
      selectedProjects.map(({ manifest, id }) => [id, manifest.name ?? id])
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

  let newHoistedDependencies!: HoistedDependencies
  if (opts.nodeLinker === 'hoisted' && hierarchy && prevGraph) {
    await linkHoistedModules(opts.storeController, graph, prevGraph, hierarchy, {
      depsStateCache,
      force: opts.force,
      ignoreScripts: opts.ignoreScripts,
      lockfileDir: opts.lockfileDir,
      preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
      sideEffectsCacheRead: opts.sideEffectsCacheRead,
    })
    stageLogger.debug({
      prefix: lockfileDir,
      stage: 'importing_done',
    })

    await symlinkDirectDependencies({
      directDependenciesByImporterId: symlinkedDirectDependenciesByImporterId!,
      filteredLockfile,
      lockfileDir,
      projects: selectedProjects,
      registries: opts.registries,
      symlink: opts.symlink,
    })
  } else if (opts.enableModulesDir !== false) {
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
        depGraph: graph,
        depsStateCache,
        ignoreScripts: opts.ignoreScripts,
        lockfileDir: opts.lockfileDir,
        sideEffectsCacheRead: opts.sideEffectsCacheRead,
      }),
    ])

    stageLogger.debug({
      prefix: lockfileDir,
      stage: 'importing_done',
    })

    if (opts.ignorePackageManifest !== true && (opts.hoistPattern != null || opts.publicHoistPattern != null)) {
      // It is important to keep the skipped packages in the lockfile which will be saved as the "current lockfile".
      // pnpm is comparing the current lockfile to the wanted one and they should match.
      // But for hoisting, we need a version of the lockfile w/o the skipped packages, so we're making a copy.
      const hoistLockfile = {
        ...filteredLockfile,
        packages: omit(Array.from(skipped), filteredLockfile.packages),
      }
      newHoistedDependencies = await hoist({
        extraNodePath: opts.extraNodePaths,
        lockfile: hoistLockfile,
        importerIds,
        preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
        privateHoistedModulesDir: hoistedModulesDir,
        privateHoistPattern: opts.hoistPattern ?? [],
        publicHoistedModulesDir,
        publicHoistPattern: opts.publicHoistPattern ?? [],
        virtualStoreDir,
      })
    } else {
      newHoistedDependencies = {}
    }

    await linkAllBins(graph, {
      extraNodePaths: opts.extraNodePaths,
      optional: opts.include.optionalDependencies,
      preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
      warn,
    })

    if ((currentLockfile != null) && !equals(importerIds.sort(), Object.keys(filteredLockfile.importers).sort())) {
      Object.assign(filteredLockfile.packages!, currentLockfile.packages)
    }

    /** Skip linking and due to no project manifest */
    if (!opts.ignorePackageManifest) {
      await symlinkDirectDependencies({
        directDependenciesByImporterId,
        filteredLockfile,
        lockfileDir,
        projects: selectedProjects,
        registries: opts.registries,
        symlink: opts.symlink,
      })
    }
  }

  if (opts.ignoreScripts) {
    for (const { id, manifest } of selectedProjects) {
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
  }
  if (!opts.ignoreScripts || Object.keys(opts.patchedDependencies ?? {}).length > 0) {
    const directNodes = new Set<string>()
    for (const id of union(importerIds, ['.'])) {
      Object
        .values(directDependenciesByImporterId[id] ?? {})
        .filter((loc) => graph[loc])
        .forEach((loc) => {
          directNodes.add(loc)
        })
    }
    const extraBinPaths = [...opts.extraBinPaths ?? []]
    if (opts.hoistPattern != null) {
      extraBinPaths.unshift(path.join(virtualStoreDir, 'node_modules/.bin'))
    }
    let extraEnv: Record<string, string> | undefined = opts.extraEnv
    if (opts.enablePnp) {
      extraEnv = {
        ...extraEnv,
        ...makeNodeRequireOption(path.join(opts.lockfileDir, '.pnp.cjs')),
      }
    }
    await buildModules(graph, Array.from(directNodes), {
      childConcurrency: opts.childConcurrency,
      extraBinPaths,
      extraEnv,
      depsStateCache,
      ignoreScripts: opts.ignoreScripts || opts.ignoreDepScripts,
      lockfileDir,
      optional: opts.include.optionalDependencies,
      preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
      rawConfig: opts.rawConfig,
      rootModulesDir: virtualStoreDir,
      scriptsPrependNodePath: opts.scriptsPrependNodePath,
      scriptShell: opts.scriptShell,
      shellEmulator: opts.shellEmulator,
      sideEffectsCacheWrite: opts.sideEffectsCacheWrite,
      storeController: opts.storeController,
      unsafePerm: opts.unsafePerm,
      userAgent: opts.userAgent,
    })
  }

  const projectsToBeBuilt = extendProjectsWithTargetDirs(selectedProjects, wantedLockfile, {
    pkgLocationsByDepPath,
    virtualStoreDir,
  })

  if (opts.enableModulesDir !== false) {
    /** Skip linking and due to no project manifest */
    if (!opts.ignorePackageManifest) {
      await Promise.all(selectedProjects.map(async (project) => {
        if (opts.publicHoistPattern?.length && path.relative(opts.lockfileDir, project.rootDir) === '') {
          await linkBinsOfImporter(project, {
            extraNodePaths: opts.extraNodePaths,
            preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
          })
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
            {
              extraNodePaths: opts.extraNodePaths,
              preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
            }
          )
        }
      }))
    }
    const injectedDeps = {}
    for (const project of projectsToBeBuilt) {
      if (project.targetDirs.length > 0) {
        injectedDeps[project.id] = project.targetDirs.map((targetDir) => path.relative(opts.lockfileDir, targetDir))
      }
    }
    await writeModulesManifest(rootModulesDir, {
      hoistedDependencies: newHoistedDependencies,
      hoistPattern: opts.hoistPattern,
      included: opts.include,
      injectedDeps,
      layoutVersion: LAYOUT_VERSION,
      nodeLinker: opts.nodeLinker,
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
    await writeCurrentLockfile(virtualStoreDir, filteredLockfile)
  }

  // waiting till package requests are finished
  await Promise.all(depNodes.map(({ finishing }) => finishing))

  summaryLogger.debug({ prefix: lockfileDir })

  await opts.storeController.close()

  if (!opts.ignoreScripts && !opts.ignorePackageManifest) {
    await runLifecycleHooksConcurrently(
      ['preinstall', 'install', 'postinstall', 'prepare'],
      projectsToBeBuilt,
      opts.childConcurrency ?? 5,
      scriptsOpts
    )
  }

  if ((reporter != null) && typeof reporter === 'function') {
    streamParser.removeListener('data', reporter)
  }
}

type SymlinkDirectDependenciesOpts = Pick<HeadlessOptions, 'registries' | 'symlink' | 'lockfileDir'> & {
  filteredLockfile: Lockfile
  directDependenciesByImporterId: DirectDependenciesByImporterId
  projects: Project[]
}

async function symlinkDirectDependencies (
  {
    filteredLockfile,
    directDependenciesByImporterId,
    lockfileDir,
    projects,
    registries,
    symlink,
  }: SymlinkDirectDependenciesOpts
) {
  await Promise.all(projects.map(async ({ rootDir, id, manifest, modulesDir }) => {
    if (symlink !== false) {
      await linkRootPackages(filteredLockfile, {
        importerId: id,
        importerModulesDir: modulesDir,
        lockfileDir,
        projectDir: rootDir,
        projects,
        registries,
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
}

async function linkBinsOfImporter (
  { manifest, modulesDir, binsDir, rootDir }: {
    binsDir: string
    manifest: ProjectManifest
    modulesDir: string
    rootDir: string
  },
  { extraNodePaths, preferSymlinkedExecutables }: { extraNodePaths?: string[], preferSymlinkedExecutables?: boolean } = {}
) {
  const warn = (message: string) => logger.info({ message, prefix: rootDir })
  return linkBins(modulesDir, binsDir, {
    extraNodePaths,
    allowExoticManifests: true,
    preferSymlinkedExecutables,
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
    rootDependencies: { [alias: string]: string }
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
          const packageDir = path.join(opts.projectDir, allDeps[alias].slice(5))
          const linkedPackage = await (async () => {
            const importerId = getLockfileImporterId(opts.lockfileDir, packageDir)
            if (importerManifestsByImporterId[importerId]) {
              return importerManifestsByImporterId[importerId]
            }
            try {
              // TODO: cover this case with a test
              return await readProjectManifestOnly(packageDir) as DependencyManifest
            } catch (err: any) { // eslint-disable-line
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

const limitLinking = pLimit(16)

async function linkAllPkgs (
  storeController: StoreController,
  depNodes: DependenciesGraphNode[],
  opts: {
    depGraph: DependenciesGraph
    depsStateCache: DepsStateCache
    force: boolean
    ignoreScripts: boolean
    lockfileDir: string
    sideEffectsCacheRead: boolean
  }
) {
  return Promise.all(
    depNodes.map(async (depNode) => {
      let filesResponse!: PackageFilesResponse
      try {
        filesResponse = await depNode.fetchingFiles()
      } catch (err: any) { // eslint-disable-line
        if (depNode.optional) return
        throw err
      }

      let sideEffectsCacheKey: string | undefined
      if (opts.sideEffectsCacheRead && filesResponse.sideEffects && !isEmpty(filesResponse.sideEffects)) {
        sideEffectsCacheKey = calcDepState(opts.depGraph, opts.depsStateCache, depNode.dir, {
          isBuilt: !opts.ignoreScripts && depNode.requiresBuild,
          patchFileHash: depNode.patchFile?.hash,
        })
      }
      const { importMethod, isBuilt } = await storeController.importPackage(depNode.dir, {
        filesResponse,
        force: opts.force,
        requiresBuild: depNode.requiresBuild || depNode.patchFile != null,
        sideEffectsCacheKey,
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

      const selfDep = depNode.children[depNode.name]
      if (selfDep) {
        const pkg = opts.depGraph[selfDep]
        if (!pkg) return
        const targetModulesDir = path.join(depNode.modules, depNode.name, 'node_modules')
        await limitLinking(async () => symlinkDependency(pkg.dir, targetModulesDir, depNode.name))
      }
    })
  )
}

async function linkAllBins (
  depGraph: DependenciesGraph,
  opts: {
    extraNodePaths?: string[]
    optional: boolean
    preferSymlinkedExecutables?: boolean
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
          await linkBins(depNode.modules, binPath, {
            extraNodePaths: opts.extraNodePaths,
            preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
            warn: opts.warn,
          })
        } else {
          const pkgs = await Promise.all(
            pkgSnapshots
              .filter(({ hasBin }) => hasBin)
              .map(async ({ dir }) => ({
                location: dir,
                manifest: await readPackageJsonFromDir(dir) as DependencyManifest,
              }))
          )

          await linkBinsOfPackages(pkgs, binPath, {
            extraNodePaths: opts.extraNodePaths,
            preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
          })
        }

        // link also the bundled dependencies` bins
        if (depNode.hasBundledDependencies) {
          const bundledModules = path.join(depNode.dir, 'node_modules')
          await linkBins(bundledModules, binPath, {
            extraNodePaths: opts.extraNodePaths,
            preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
            warn: opts.warn,
          })
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
          Object.entries(childrenToLink)
            .map(async ([alias, pkgDir]) => {
              // if (!pkg.installable && pkg.optional) return
              if (alias === depNode.name) {
                return
              }
              await limitLinking(async () => symlinkDependency(pkgDir, depNode.modules, alias))
            })
        )
      })
  )
}
