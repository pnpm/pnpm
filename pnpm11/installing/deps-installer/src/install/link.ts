import { stageLogger } from '@pnpm/core-loggers'
import type { DepsStateCache } from '@pnpm/deps.graph-hasher'
import type {
  DependenciesGraph,
  LinkedDependency,
} from '@pnpm/installing.deps-resolver'
import type { InstallationResultStats } from '@pnpm/installing.deps-restorer'
import { linkDirectDeps } from '@pnpm/installing.linking.direct-dep-linker'
import {
  hoist,
  type HoistedWorkspaceProject,
  hoistWorkspacePackages,
  type HoistWorkspacePackagesOpts,
  pruneStaleWorkspaceHoists,
} from '@pnpm/installing.linking.hoist'
import { prune } from '@pnpm/installing.linking.modules-cleaner'
import type { IncludedDependencies } from '@pnpm/installing.modules-yaml'
import {
  filterLockfileByImporters,
} from '@pnpm/lockfile.filtering'
import type { LockfileObject } from '@pnpm/lockfile.fs'
import type { StoreController } from '@pnpm/store.controller-types'
import type {
  AllowBuild,
  DepPath,
  HoistedDependencies,
  ProjectId,
  RegistriesByScope,
  RegistryConfig,
  RemoteSideEffectsCacheSettings,
  SupportedArchitectures,
} from '@pnpm/types'
import { equals, pick } from 'ramda'

import { getProjectsToLink } from './getProjectsToLink.js'
import type { ImporterToUpdate } from './index.js'
import { linkNewPackages, type LinkNewPackagesOptions } from './linkNewPackages.js'

export interface LinkPackagesOptions {
  allowBuild?: AllowBuild
  currentLockfile: LockfileObject
  dedupeDirectDeps: boolean
  dependenciesByProjectId: Record<string, Map<string, DepPath>>
  deferDependencyBuilds: boolean
  disableRelinkLocalDirDeps?: boolean
  force: boolean
  depsStateCache: DepsStateCache
  enableGlobalVirtualStore: boolean
  extraNodePaths: string[]
  hoistedDependencies: HoistedDependencies
  hoistedModulesDir: string
  hoistPattern?: string[]
  ignoreScripts: boolean
  publicHoistPattern?: string[]
  include: IncludedDependencies
  linkedDependenciesByProjectId: Record<string, LinkedDependency[]>
  lockfileDir: string
  makePartialCurrentLockfile: boolean
  outdatedDependencies: Record<string, string>
  pruneStore: boolean
  pruneVirtualStore: boolean
  registriesByScope: RegistriesByScope
  resolvePeersFromWorkspaceRoot?: boolean
  rootModulesDir: string
  sideEffectsCacheRead: boolean
  remoteSideEffectsCache?: RemoteSideEffectsCacheSettings
  pnprServer?: string
  configByUri: Record<string, RegistryConfig>
  symlink: boolean
  skipped: Set<DepPath>
  skipRuntimes?: boolean
  storeController: StoreController
  virtualStoreDir: string
  virtualStoreDirMaxLength: number
  wantedLockfile: LockfileObject
  wantedToBeSkippedPackageIds: Set<string>
  hoistWorkspacePackages?: boolean
  virtualStoreOnly: boolean
  supportedArchitectures?: SupportedArchitectures
}

export interface LinkPackagesResult {
  currentLockfile: LockfileObject
  newDepPaths: DepPath[]
  newHoistedDependencies: HoistedDependencies
  removedDepPaths: Set<string>
  stats: InstallationResultStats
}

export async function linkPackages (projects: ImporterToUpdate[], depGraph: DependenciesGraph, opts: LinkPackagesOptions): Promise<LinkPackagesResult> {
  const graph = selectLinkedNodes(depGraph, opts)
  const removedDepPaths = await pruneModules(projects, opts)
  const projectIds = projects.map(({ id }) => id)
  const { newDepPaths, added, newCurrentLockfile } = await importPackages(graph, projectIds, opts)

  const allImportersIncluded = equals(projectIds.sort(), Object.keys(opts.wantedLockfile.importers).sort())
  const context: LinkContext = { allImportersIncluded, graph, newCurrentLockfile, projectIds }
  const currentLockfile = selectCurrentLockfile(context, opts)
  const newHoistedDependencies = await hoistDependencies(projects, {
    ...context,
    graphChanged: newDepPaths.length > 0 || removedDepPaths.size > 0,
  }, opts)

  const linkedToRoot = opts.symlink && !opts.virtualStoreOnly
    ? await linkDirectDeps(getProjectsToLink(projects, context, opts), { dedupe: opts.dedupeDirectDeps })
    : 0

  return {
    currentLockfile,
    newDepPaths,
    newHoistedDependencies,
    removedDepPaths,
    stats: {
      added,
      removed: removedDepPaths.size,
      linkedToRoot,
    },
  }
}

export interface LinkContext {
  allImportersIncluded: boolean
  graph: DependenciesGraph
  newCurrentLockfile: LockfileObject
  projectIds: ProjectId[]
}

/**
 * The nodes this install links: the ones the included dependency types reach,
 * without the ones to be skipped. Records the skipped ones in `opts.skipped`.
 */
function selectLinkedNodes (depGraph: DependenciesGraph, opts: LinkPackagesOptions): DependenciesGraph {
  let depNodes = Object.values(depGraph).filter(({ depPath, id }) => {
    if (((opts.wantedLockfile.packages?.[depPath]) != null) && !opts.wantedLockfile.packages[depPath].optional) {
      opts.skipped.delete(depPath)
      return true
    }
    if (opts.wantedToBeSkippedPackageIds.has(id)) {
      opts.skipped.add(depPath)
      return false
    }
    opts.skipped.delete(depPath)
    return true
  })
  if (!opts.include.dependencies) {
    depNodes = depNodes.filter(({ dev, optional }) => dev || optional)
  }
  if (!opts.include.devDependencies) {
    depNodes = depNodes.filter(({ optional, prod }) => prod || optional)
  }
  if (!opts.include.optionalDependencies) {
    depNodes = depNodes.filter(({ optional }) => !optional)
  }
  return Object.fromEntries(depNodes.map((depNode) => [depNode.depPath, depNode]))
}

async function pruneModules (projects: ImporterToUpdate[], opts: LinkPackagesOptions): Promise<Set<string>> {
  return prune(projects, {
    currentLockfile: opts.currentLockfile,
    dedupeDirectDeps: opts.dedupeDirectDeps,
    hoistedDependencies: opts.hoistedDependencies,
    hoistedModulesDir: (opts.hoistPattern != null) ? opts.hoistedModulesDir : undefined,
    include: opts.include,
    lockfileDir: opts.lockfileDir,
    pruneStore: opts.pruneStore,
    pruneVirtualStore: opts.pruneVirtualStore,
    publicHoistedModulesDir: (opts.publicHoistPattern != null) ? opts.rootModulesDir : undefined,
    resolvePeersFromWorkspaceRoot: opts.resolvePeersFromWorkspaceRoot,
    skipped: opts.skipped,
    skipRuntimes: opts.skipRuntimes,
    storeController: opts.storeController,
    virtualStoreDir: opts.virtualStoreDir,
    virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
    wantedLockfile: opts.wantedLockfile,
  })
}

function getLockfileFilterOptions (opts: LinkPackagesOptions): Pick<LinkPackagesOptions, 'include' | 'registriesByScope' | 'resolvePeersFromWorkspaceRoot' | 'skipped' | 'skipRuntimes'> {
  return {
    include: opts.include,
    registriesByScope: opts.registriesByScope,
    resolvePeersFromWorkspaceRoot: opts.resolvePeersFromWorkspaceRoot,
    skipped: opts.skipped,
    skipRuntimes: opts.skipRuntimes,
  }
}

/** Import the packages new to the projects' node_modules into the virtual store. */
async function importPackages (
  graph: DependenciesGraph,
  projectIds: ProjectId[],
  opts: LinkPackagesOptions
): Promise<{ newDepPaths: DepPath[], added: number, newCurrentLockfile: LockfileObject }> {
  stageLogger.debug({
    prefix: opts.lockfileDir,
    stage: 'importing_started',
  })

  const filterOpts = getLockfileFilterOptions(opts)
  const newCurrentLockfile = filterLockfileByImporters(opts.wantedLockfile, projectIds, {
    ...filterOpts,
    failOnMissingDependencies: true,
    skipped: new Set(),
  })
  const { newDepPaths, added } = await linkNewPackages(
    filterLockfileByImporters(opts.currentLockfile, projectIds, {
      ...filterOpts,
      failOnMissingDependencies: false,
    }),
    newCurrentLockfile,
    graph,
    getLinkNewPackagesOptions(opts)
  )

  stageLogger.debug({
    prefix: opts.lockfileDir,
    stage: 'importing_done',
  })
  return { newDepPaths, added, newCurrentLockfile }
}

function getLinkNewPackagesOptions (opts: LinkPackagesOptions): LinkNewPackagesOptions {
  return {
    allowBuild: opts.allowBuild,
    deferDependencyBuilds: opts.deferDependencyBuilds,
    disableRelinkLocalDirDeps: opts.disableRelinkLocalDirDeps,
    enableGlobalVirtualStore: opts.enableGlobalVirtualStore,
    force: opts.force,
    depsStateCache: opts.depsStateCache,
    ignoreScripts: opts.ignoreScripts,
    lockfileDir: opts.lockfileDir,
    optional: opts.include.optionalDependencies,
    sideEffectsCacheRead: opts.sideEffectsCacheRead,
    remoteSideEffectsCache: opts.remoteSideEffectsCache,
    pnprServer: opts.pnprServer,
    configByUri: opts.configByUri,
    symlink: opts.symlink,
    skipped: opts.skipped,
    storeController: opts.storeController,
    supportedArchitectures: opts.supportedArchitectures,
    virtualStoreDir: opts.virtualStoreDir,
  }
}

/** The lockfile that describes node_modules once this install is done. */
function selectCurrentLockfile (context: LinkContext, opts: LinkPackagesOptions): LockfileObject {
  if (
    opts.makePartialCurrentLockfile ||
    !context.allImportersIncluded
  ) {
    return mergeIntoCurrentLockfile(context, opts)
  }
  if (
    opts.include.dependencies &&
    opts.include.devDependencies &&
    opts.include.optionalDependencies &&
    opts.skipped.size === 0
  ) {
    return opts.wantedLockfile
  }
  return context.newCurrentLockfile
}

/** The current lockfile with the installed projects and packages taken from the wanted one. */
function mergeIntoCurrentLockfile (
  { graph, projectIds }: LinkContext,
  opts: LinkPackagesOptions
): LockfileObject {
  const packages = opts.currentLockfile.packages ?? {}
  if (opts.wantedLockfile.packages != null) {
    for (const depPath of Object.keys(opts.wantedLockfile.packages)) {
      if (graph[depPath as DepPath]) {
        packages[depPath as DepPath] = opts.wantedLockfile.packages[depPath as DepPath]
      }
    }
  }
  const projects = {
    ...opts.currentLockfile.importers,
    ...pick(projectIds, opts.wantedLockfile.importers),
  }
  return filterLockfileByImporters(
    {
      ...opts.wantedLockfile,
      importers: projects,
      packages,
    },
    Object.keys(projects) as ProjectId[], {
      ...getLockfileFilterOptions(opts),
      failOnMissingDependencies: false,
      skipped: new Set(),
    }
  )
}

async function hoistDependencies (
  projects: ImporterToUpdate[],
  context: LinkContext & { graphChanged: boolean },
  opts: LinkPackagesOptions
): Promise<HoistedDependencies> {
  if (opts.virtualStoreOnly || (opts.hoistPattern == null && opts.publicHoistPattern == null)) {
    return {}
  }
  const priorWorkspaceProjectIds = findPriorWorkspaceProjectIds(context, opts)
  const hoistOpts = getHoistOptions(projects, { context, priorWorkspaceProjectIds }, opts)
  const retainedHoistedDependencies = Object.fromEntries(
    Object.entries(opts.hoistedDependencies)
      .filter(([key]) => !priorWorkspaceProjectIds.has(key as ProjectId))
  ) as HoistedDependencies
  let nextHoistedDependencies: HoistedDependencies
  if (context.graphChanged) {
    nextHoistedDependencies = await hoist({
      ...hoistOpts,
      extraNodePath: opts.extraNodePaths,
      importerIds: context.projectIds,
      virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
      skipped: opts.skipped,
    }) ?? {}
  } else {
    // No dependency was added or removed, so the hoisted graph cannot have
    // changed. The set of workspace projects still can: this install may have
    // added one, and a workspace that depends on nothing external has no graph
    // to hoist from in the first place.
    nextHoistedDependencies = await hoistWorkspacePackages(hoistOpts)
  }
  return {
    ...retainedHoistedDependencies,
    ...nextHoistedDependencies,
  }
}

/** The workspace projects hoisted by the previous install that this one hoists anew. */
function findPriorWorkspaceProjectIds (
  { allImportersIncluded, projectIds }: LinkContext,
  opts: LinkPackagesOptions
): Set<ProjectId> {
  return new Set(
    Object.keys(opts.hoistedDependencies)
      .filter((key) => (
        opts.currentLockfile.packages?.[key as DepPath] == null &&
        (allImportersIncluded || projectIds.includes(key as ProjectId))
      )) as ProjectId[]
  )
}

function getHoistOptions (
  projects: ImporterToUpdate[],
  { context, priorWorkspaceProjectIds }: { context: LinkContext, priorWorkspaceProjectIds: Set<ProjectId> },
  opts: LinkPackagesOptions
): HoistWorkspacePackagesOpts<DepPath> {
  return {
    graph: context.graph,
    directDepsByImporterId: {
      ...opts.dependenciesByProjectId,
      '.': new Map(Array.from(opts.dependenciesByProjectId['.']?.entries() ?? []).filter(([alias]) => {
        return context.newCurrentLockfile.importers['.' as ProjectId].specifiers[alias]
      })),
    },
    privateHoistedModulesDir: opts.hoistedModulesDir,
    privateHoistPattern: opts.hoistPattern ?? [],
    publicHoistedModulesDir: opts.rootModulesDir,
    publicHoistPattern: opts.publicHoistPattern ?? [],
    virtualStoreDir: opts.virtualStoreDir,
    hoistedWorkspacePackages: opts.hoistWorkspacePackages
      ? collectHoistedWorkspacePackages(projects)
      : undefined,
    beforeWorkspaceLinks: async (nextWorkspaceHoists: HoistedDependencies) => pruneStaleWorkspaceHoists(
      opts.hoistedDependencies,
      nextWorkspaceHoists,
      priorWorkspaceProjectIds,
      opts.hoistedModulesDir,
      opts.rootModulesDir
    ),
  }
}

function collectHoistedWorkspacePackages (projects: ImporterToUpdate[]): Record<string, HoistedWorkspaceProject> {
  return projects.reduce((hoistedWorkspacePackages, project) => {
    if (project.manifest.name && project.id !== '.') {
      hoistedWorkspacePackages[project.id] = {
        dir: project.rootDir,
        name: project.manifest.name,
      }
    }
    return hoistedWorkspacePackages
  }, {} as Record<string, HoistedWorkspaceProject>)
}
