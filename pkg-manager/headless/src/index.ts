import { promises as fs } from 'fs'
import path from 'path'
import { buildModules } from '@pnpm/build-modules'
import { createAllowBuildFunction } from '@pnpm/builder.policy'
import { calcDepState, type DepsStateCache } from '@pnpm/calc-dep-state'
import {
  LAYOUT_VERSION,
  WANTED_LOCKFILE,
} from '@pnpm/constants'
import {
  packageManifestLogger,
  progressLogger,
  stageLogger,
  statsLogger,
  summaryLogger,
} from '@pnpm/core-loggers'
import {
  filterLockfileByEngine,
  filterLockfileByImportersAndEngine,
} from '@pnpm/filter-lockfile'
import { hoist, type HoistedWorkspaceProject } from '@pnpm/hoist'
import {
  runLifecycleHooksConcurrently,
  makeNodeRequireOption,
} from '@pnpm/lifecycle'
import { linkBins, linkBinsOfPackages } from '@pnpm/link-bins'
import {
  getLockfileImporterId,
  type Lockfile,
  readCurrentLockfile,
  readWantedLockfile,
  writeLockfiles,
  writeCurrentLockfile,
  type PatchFile,
} from '@pnpm/lockfile-file'
import { writePnpFile } from '@pnpm/lockfile-to-pnp'
import {
  extendProjectsWithTargetDirs,
  nameVerFromPkgSnapshot,
} from '@pnpm/lockfile-utils'
import {
  type LogBase,
  logger,
  streamParser,
} from '@pnpm/logger'
import { prune } from '@pnpm/modules-cleaner'
import {
  type IncludedDependencies,
  writeModulesManifest,
} from '@pnpm/modules-yaml'
import { type HoistingLimits } from '@pnpm/real-hoist'
import { readPackageJsonFromDir } from '@pnpm/read-package-json'
import { readProjectManifestOnly, safeReadProjectManifestOnly } from '@pnpm/read-project-manifest'
import {
  type PackageFilesResponse,
  type StoreController,
} from '@pnpm/store-controller-types'
import { symlinkDependency } from '@pnpm/symlink-dependency'
import {
  type DepPath,
  type DependencyManifest,
  type HoistedDependencies,
  type ProjectId,
  type ProjectManifest,
  type Registries,
  DEPENDENCIES_FIELDS,
  type SupportedArchitectures,
  type ProjectRootDir,
} from '@pnpm/types'
import * as dp from '@pnpm/dependency-path'
import { symlinkAllModules } from '@pnpm/worker'
import pLimit from 'p-limit'
import pathAbsolute from 'path-absolute'
import equals from 'ramda/src/equals'
import isEmpty from 'ramda/src/isEmpty'
import omit from 'ramda/src/omit'
import pick from 'ramda/src/pick'
import pickBy from 'ramda/src/pickBy'
import props from 'ramda/src/props'
import union from 'ramda/src/union'
import realpathMissing from 'realpath-missing'
import { linkHoistedModules } from './linkHoistedModules'
import {
  type DirectDependenciesByImporterId,
  type DependenciesGraph,
  type DependenciesGraphNode,
  type LockfileToDepGraphOptions,
  lockfileToDepGraph,
} from '@pnpm/deps.graph-builder'
import { lockfileToHoistedDepGraph } from './lockfileToHoistedDepGraph'
import { linkDirectDeps, type LinkedDirectDep } from '@pnpm/pkg-manager.direct-dep-linker'

export type { HoistingLimits }

export type ReporterFunction = (logObj: LogBase) => void

export interface Project {
  binsDir: string
  buildIndex: number
  manifest: ProjectManifest
  modulesDir: string
  id: ProjectId
  pruneDirectDependencies?: boolean
  rootDir: ProjectRootDir
}

export interface HeadlessOptions {
  neverBuiltDependencies?: string[]
  onlyBuiltDependencies?: string[]
  onlyBuiltDependenciesFile?: string
  autoInstallPeers?: boolean
  childConcurrency?: number
  currentLockfile?: Lockfile
  currentEngine: {
    nodeVersion?: string
    pnpmVersion: string
  }
  dedupeDirectDeps?: boolean
  enablePnp?: boolean
  engineStrict: boolean
  excludeLinksFromLockfile?: boolean
  extraBinPaths?: string[]
  extraEnv?: Record<string, string>
  extraNodePaths?: string[]
  preferSymlinkedExecutables?: boolean
  hoistingLimits?: HoistingLimits
  externalDependencies?: Set<string>
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
  currentHoistedLocations?: Record<string, string[]>
  lockfileDir: string
  modulesDir?: string
  virtualStoreDir?: string
  virtualStoreDirMaxLength: number
  patchedDependencies?: Record<string, PatchFile>
  scriptsPrependNodePath?: boolean | 'warn-only'
  scriptShell?: string
  shellEmulator?: boolean
  storeController: StoreController
  sideEffectsCacheRead: boolean
  sideEffectsCacheWrite: boolean
  symlink?: boolean
  disableRelinkLocalDirDeps?: boolean
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
  skipped: Set<DepPath>
  enableModulesDir?: boolean
  nodeLinker?: 'isolated' | 'hoisted' | 'pnp'
  useGitBranchLockfile?: boolean
  useLockfile?: boolean
  supportedArchitectures?: SupportedArchitectures
  hoistWorkspacePackages?: boolean
}

export interface InstallationResultStats {
  added: number
  removed: number
  linkedToRoot: number
}

export interface InstallationResult {
  stats: InstallationResultStats
}

export async function headlessInstall (opts: HeadlessOptions): Promise<InstallationResult> {
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

  const scriptsOpts = {
    optional: false,
    extraBinPaths: opts.extraBinPaths,
    extraNodePaths: opts.extraNodePaths,
    preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
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

  const skipped = opts.skipped || new Set<DepPath>()
  const filterOpts = {
    include: opts.include,
    registries: opts.registries,
    skipped,
    currentEngine: opts.currentEngine,
    engineStrict: opts.engineStrict,
    failOnMissingDependencies: true,
    includeIncompatiblePackages: opts.force,
    lockfileDir,
    supportedArchitectures: opts.supportedArchitectures,
  }
  let removed = 0
  if (opts.nodeLinker !== 'hoisted') {
    if (currentLockfile != null && !opts.ignorePackageManifest) {
      const removedDepPaths = await prune(
        selectedProjects,
        {
          currentLockfile,
          dedupeDirectDeps: opts.dedupeDirectDeps,
          dryRun: false,
          hoistedDependencies: opts.hoistedDependencies,
          hoistedModulesDir: (opts.hoistPattern == null) ? undefined : hoistedModulesDir,
          include: opts.include,
          lockfileDir,
          pruneStore: opts.pruneStore,
          pruneVirtualStore: opts.pruneVirtualStore,
          publicHoistedModulesDir: (opts.publicHoistPattern == null) ? undefined : publicHoistedModulesDir,
          skipped,
          storeController: opts.storeController,
          virtualStoreDir,
          virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
          wantedLockfile: filterLockfileByEngine(wantedLockfile, filterOpts).lockfile,
        }
      )
      removed = removedDepPaths.size
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

  const initialImporterIds = (opts.ignorePackageManifest === true || opts.nodeLinker === 'hoisted')
    ? Object.keys(wantedLockfile.importers) as ProjectId[]
    : selectedProjects.map(({ id }) => id)
  const { lockfile: filteredLockfile, selectedImporterIds: importerIds } = filterLockfileByImportersAndEngine(wantedLockfile, initialImporterIds, filterOpts)
  if (opts.excludeLinksFromLockfile) {
    for (const { id, manifest, rootDir } of selectedProjects) {
      if (filteredLockfile.importers[id]) {
        for (const depType of DEPENDENCIES_FIELDS) {
          filteredLockfile.importers[id][depType] = {
            ...filteredLockfile.importers[id][depType],
            ...Object.entries(manifest[depType] ?? {})
              .filter(([_, spec]) => spec.startsWith('link:'))
              .reduce((acc, [depName, spec]) => {
                const linkPath = spec.substring(5)
                acc[depName] = path.isAbsolute(linkPath) ? `link:${path.relative(rootDir, spec.substring(5))}` : spec
                return acc
              }, {} as Record<string, string>),
          }
        }
      }
    }
  }

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
    supportedArchitectures: opts.supportedArchitectures,
  } as LockfileToDepGraphOptions
  const {
    directDependenciesByImporterId,
    graph,
    hierarchy,
    hoistedLocations,
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
    const importerNames = Object.fromEntries(
      selectedProjects.map(({ manifest, id }) => [id, manifest.name ?? id])
    )
    await writePnpFile(filteredLockfile, {
      importerNames,
      lockfileDir,
      virtualStoreDir,
      virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
      registries: opts.registries,
    })
  }
  const depNodes = Object.values(graph)

  const added = depNodes.filter(({ fetching }) => fetching).length
  statsLogger.debug({
    added,
    prefix: lockfileDir,
  })

  function warn (message: string) {
    logger.info({
      message,
      prefix: lockfileDir,
    })
  }

  let newHoistedDependencies!: HoistedDependencies
  let linkedToRoot = 0
  if (opts.nodeLinker === 'hoisted' && hierarchy && prevGraph) {
    await linkHoistedModules(opts.storeController, graph, prevGraph, hierarchy, {
      depsStateCache,
      disableRelinkLocalDirDeps: opts.disableRelinkLocalDirDeps,
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

    linkedToRoot = await symlinkDirectDependencies({
      directDependenciesByImporterId: symlinkedDirectDependenciesByImporterId!,
      dedupe: Boolean(opts.dedupeDirectDeps),
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
          optional: opts.include.optionalDependencies,
        }),
      linkAllPkgs(opts.storeController, depNodes, {
        force: opts.force,
        disableRelinkLocalDirDeps: opts.disableRelinkLocalDirDeps,
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
        packages: filteredLockfile.packages != null ? omit(Array.from(skipped), filteredLockfile.packages) : {},
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
        virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
        hoistedWorkspacePackages: opts.hoistWorkspacePackages
          ? Object.values(opts.allProjects).reduce((hoistedWorkspacePackages, project) => {
            if (project.manifest.name && project.id !== '.') {
              hoistedWorkspacePackages[project.id] = {
                dir: project.rootDir,
                name: project.manifest.name,
              }
            }
            return hoistedWorkspacePackages
          }, {} as Record<string, HoistedWorkspaceProject>)
          : undefined,
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
      linkedToRoot = await symlinkDirectDependencies({
        dedupe: Boolean(opts.dedupeDirectDeps),
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
      allowBuild: createAllowBuildFunction(opts),
      childConcurrency: opts.childConcurrency,
      extraBinPaths,
      extraEnv,
      depsStateCache,
      ignoreScripts: opts.ignoreScripts || opts.ignoreDepScripts,
      hoistedLocations,
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
    virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
  })

  if (opts.enableModulesDir !== false) {
    const rootProjectDeps = !opts.dedupeDirectDeps ? {} : (directDependenciesByImporterId['.'] ?? {})
    /** Skip linking and due to no project manifest */
    if (!opts.ignorePackageManifest) {
      await Promise.all(selectedProjects.map(async (project) => {
        if (opts.nodeLinker === 'hoisted' || opts.publicHoistPattern?.length && path.relative(opts.lockfileDir, project.rootDir) === '') {
          await linkBinsOfImporter(project, {
            extraNodePaths: opts.extraNodePaths,
            preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
          })
        } else {
          let directPkgDirs: string[]
          if (project.id === '.') {
            directPkgDirs = Object.values(directDependenciesByImporterId[project.id])
          } else {
            directPkgDirs = []
            for (const [alias, dir] of Object.entries(directDependenciesByImporterId[project.id])) {
              if (rootProjectDeps[alias] !== dir) {
                directPkgDirs.push(dir)
              }
            }
          }
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
    const injectedDeps: Record<string, string[]> = {}
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
      hoistedLocations,
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
      virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
    }, {
      makeModulesDir: Object.keys(filteredLockfile.packages ?? {}).length > 0,
    })
    if (opts.useLockfile) {
      // We need to write the wanted lockfile as well.
      // Even though it will only be changed if the workspace will have new projects with no dependencies.
      await writeLockfiles({
        wantedLockfileDir: opts.lockfileDir,
        currentLockfileDir: virtualStoreDir,
        wantedLockfile,
        currentLockfile: filteredLockfile,
      })
    } else {
      await writeCurrentLockfile(virtualStoreDir, filteredLockfile)
    }
  }

  // waiting till package requests are finished
  await Promise.all(depNodes.map(async ({ fetching }) => {
    try {
      await fetching?.()
    } catch {}
  }))

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
  return {
    stats: {
      added,
      removed,
      linkedToRoot,
    },
  }
}

type SymlinkDirectDependenciesOpts = Pick<HeadlessOptions, 'registries' | 'symlink' | 'lockfileDir'> & {
  filteredLockfile: Lockfile
  dedupe: boolean
  directDependenciesByImporterId: DirectDependenciesByImporterId
  projects: Project[]
}

async function symlinkDirectDependencies (
  {
    filteredLockfile,
    dedupe,
    directDependenciesByImporterId,
    lockfileDir,
    projects,
    registries,
    symlink,
  }: SymlinkDirectDependenciesOpts
): Promise<number> {
  projects.forEach(({ rootDir, manifest }) => {
    // Even though headless installation will never update the package.json
    // this needs to be logged because otherwise install summary won't be printed
    packageManifestLogger.debug({
      prefix: rootDir,
      updated: manifest,
    })
  })
  if (symlink === false) return 0
  const importerManifestsByImporterId = {} as { [id: string]: ProjectManifest }
  for (const { id, manifest } of projects) {
    importerManifestsByImporterId[id] = manifest
  }
  const projectsToLink = Object.fromEntries(await Promise.all(
    projects.map(async ({ rootDir, id, modulesDir }) => ([id, {
      dir: rootDir,
      modulesDir,
      dependencies: await getRootPackagesToLink(filteredLockfile, {
        importerId: id,
        importerModulesDir: modulesDir,
        lockfileDir,
        projectDir: rootDir,
        importerManifestsByImporterId,
        registries,
        rootDependencies: directDependenciesByImporterId[id],
      }),
    }]))
  ))
  const rootProject = projectsToLink['.']
  if (rootProject && dedupe) {
    const rootDeps = Object.fromEntries(rootProject.dependencies.map((dep: LinkedDirectDep) => [dep.alias, dep.dir]))
    for (const project of Object.values(omit(['.'], projectsToLink))) {
      project.dependencies = project.dependencies.filter((dep: LinkedDirectDep) => dep.dir !== rootDeps[dep.alias])
    }
  }
  return linkDirectDeps(projectsToLink, { dedupe: Boolean(dedupe) })
}

async function linkBinsOfImporter (
  { manifest, modulesDir, binsDir, rootDir }: {
    binsDir: string
    manifest: ProjectManifest
    modulesDir: string
    rootDir: ProjectRootDir
  },
  { extraNodePaths, preferSymlinkedExecutables }: { extraNodePaths?: string[], preferSymlinkedExecutables?: boolean } = {}
): Promise<string[]> {
  const warn = (message: string) => {
    logger.info({ message, prefix: rootDir })
  }
  return linkBins(modulesDir, binsDir, {
    extraNodePaths,
    allowExoticManifests: true,
    preferSymlinkedExecutables,
    projectManifest: manifest,
    warn,
  })
}

async function getRootPackagesToLink (
  lockfile: Lockfile,
  opts: {
    registries: Registries
    projectDir: string
    importerId: ProjectId
    importerModulesDir: string
    importerManifestsByImporterId: { [id: string]: ProjectManifest }
    lockfileDir: string
    rootDependencies: { [alias: string]: string }
  }
): Promise<LinkedDirectDep[]> {
  const projectSnapshot = lockfile.importers[opts.importerId]
  const allDeps = {
    ...projectSnapshot.devDependencies,
    ...projectSnapshot.dependencies,
    ...projectSnapshot.optionalDependencies,
  }
  return (await Promise.all(
    Object.entries(allDeps)
      .map(async ([alias, ref]) => {
        if (ref.startsWith('link:')) {
          const isDev = Boolean(projectSnapshot.devDependencies?.[alias])
          const isOptional = Boolean(projectSnapshot.optionalDependencies?.[alias])
          const packageDir = path.join(opts.projectDir, ref.slice(5))
          const linkedPackage = await (async () => {
            const importerId = getLockfileImporterId(opts.lockfileDir, packageDir)
            if (opts.importerManifestsByImporterId[importerId]) {
              return opts.importerManifestsByImporterId[importerId]
            }
            try {
              // TODO: cover this case with a test
              return await readProjectManifestOnly(packageDir) as DependencyManifest
            } catch (err: any) { // eslint-disable-line
              if (err['code'] !== 'ERR_PNPM_NO_IMPORTER_MANIFEST_FOUND') throw err
              return { name: alias, version: '0.0.0' }
            }
          })() as DependencyManifest
          return {
            alias,
            name: linkedPackage.name,
            version: linkedPackage.version,
            dir: packageDir,
            id: ref,
            isExternalLink: true,
            dependencyType: isDev && 'dev' ||
              isOptional && 'optional' ||
              'prod',
          }
        }
        const dir = opts.rootDependencies[alias]
        // Skipping linked packages
        if (!dir) {
          return
        }
        const isDev = Boolean(projectSnapshot.devDependencies?.[alias])
        const isOptional = Boolean(projectSnapshot.optionalDependencies?.[alias])

        const depPath = dp.refToRelative(ref, alias)
        if (depPath === null) return
        const pkgSnapshot = lockfile.packages?.[depPath]
        if (pkgSnapshot == null) return // this won't ever happen. Just making typescript happy
        const pkgId = pkgSnapshot.id ?? dp.refToRelative(ref, alias) ?? undefined
        const pkgInfo = nameVerFromPkgSnapshot(depPath, pkgSnapshot)
        return {
          alias,
          isExternalLink: false,
          name: pkgInfo.name,
          version: pkgInfo.version,
          dependencyType: isDev && 'dev' || isOptional && 'optional' || 'prod',
          dir,
          id: pkgId,
        }
      })
  ))
    .filter(Boolean) as LinkedDirectDep[]
}

const limitLinking = pLimit(16)

async function linkAllPkgs (
  storeController: StoreController,
  depNodes: DependenciesGraphNode[],
  opts: {
    depGraph: DependenciesGraph
    depsStateCache: DepsStateCache
    disableRelinkLocalDirDeps?: boolean
    force: boolean
    ignoreScripts: boolean
    lockfileDir: string
    sideEffectsCacheRead: boolean
  }
): Promise<void> {
  await Promise.all(
    depNodes.map(async (depNode) => {
      if (!depNode.fetching) return
      let filesResponse!: PackageFilesResponse
      try {
        filesResponse = (await depNode.fetching()).files
      } catch (err: any) { // eslint-disable-line
        if (depNode.optional) return
        throw err
      }

      depNode.requiresBuild = filesResponse.requiresBuild
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
        disableRelinkLocalDirDeps: opts.disableRelinkLocalDirDeps,
        requiresBuild: depNode.patchFile != null || depNode.requiresBuild,
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
): Promise<void> {
  await Promise.all(
    Object.values(depGraph)
      .map(async (depNode) => limitLinking(async () => {
        const childrenToLink: Record<string, string> = opts.optional
          ? depNode.children
          : pickBy((_, childAlias) => !depNode.optionalDependencies.has(childAlias), depNode.children)

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
  depNodes: Array<Pick<DependenciesGraphNode, 'children' | 'optionalDependencies' | 'modules' | 'name'>>,
  opts: {
    optional: boolean
  }
): Promise<void> {
  await symlinkAllModules({
    deps: depNodes.map((depNode) => ({
      children: opts.optional
        ? depNode.children
        : pickBy((_, childAlias) => !depNode.optionalDependencies.has(childAlias), depNode.children),
      modules: depNode.modules,
      name: depNode.name,
    })),
  })
}
