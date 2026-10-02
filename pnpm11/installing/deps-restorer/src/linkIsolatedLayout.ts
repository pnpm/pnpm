import { promises as fs } from 'node:fs'

import { stageLogger } from '@pnpm/core-loggers'
import * as dp from '@pnpm/deps.path'
import { hoist, pruneStaleWorkspaceHoists } from '@pnpm/installing.linking.hoist'
import { logger } from '@pnpm/logger'
import type { DepPath, HoistedDependencies, ProjectId } from '@pnpm/types'
import { equals } from 'ramda'

import type { HeadlessContext, HeadlessDepGraph, LinkedDependencies } from './context.js'
import { linkAllBins } from './linkAllBins.js'
import { linkAllModules } from './linkAllModules.js'
import { linkAllPkgs } from './linkAllPkgs.js'
import { symlinkDirectDependencies } from './symlinkDirectDependencies.js'
import { getHoistedWorkspacePackages, workspaceHoistPointsToProject } from './workspaceHoists.js'

export async function linkIsolatedLayout (ctx: HeadlessContext, depGraph: HeadlessDepGraph): Promise<LinkedDependencies> {
  const { opts } = ctx
  const skipGvsInternalLinking = opts.enableGlobalVirtualStore === true && depGraph.added === 0
  if (!skipGvsInternalLinking) {
    await importAndLinkPackages(ctx, depGraph)
  }
  stageLogger.debug({
    prefix: ctx.lockfileDir,
    stage: 'importing_done',
  })
  const newHoistedDependencies = shouldHoistDependencies(ctx) ? await hoistDependencies(ctx, depGraph) : {}
  if (!ctx.skipPostImportLinking && !skipGvsInternalLinking) {
    await linkAllBins(depGraph.graph, {
      extraNodePaths: opts.extraNodePaths,
      optional: opts.include.optionalDependencies,
      preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
      warn: (message: string) => {
        logger.info({
          message,
          prefix: ctx.lockfileDir,
        })
      },
    })
  }
  if ((ctx.currentLockfile != null) && !equals(depGraph.importerIds.sort(), Object.keys(depGraph.filteredLockfile.importers).sort())) {
    Object.assign(depGraph.filteredLockfile.packages!, ctx.currentLockfile.packages)
  }
  return {
    heldBackBinsDirs: [],
    linkedToRoot: await symlinkDirectDependenciesOfProjects(ctx, depGraph),
    newHoistedDependencies,
  }
}

async function importAndLinkPackages (ctx: HeadlessContext, depGraph: HeadlessDepGraph): Promise<void> {
  const { opts } = ctx
  if (opts.enableModulesDir !== false) {
    await Promise.all(depGraph.depNodes.map(async (depNode) => fs.mkdir(depNode.modules, { recursive: true })))
  }
  const [, fetchFailedDirs] = await Promise.all([
    opts.symlink === false || opts.enableModulesDir === false
      ? Promise.resolve()
      : linkAllModules(depGraph.depNodes, {
        currentLockfile: ctx.currentLockfile,
        relinkChangedDependenciesOnly: opts.relinkChangedDependenciesOnly && !opts.force,
        optional: opts.include.optionalDependencies,
        wantedLockfile: depGraph.filteredLockfile,
      }),
    linkAllPkgs(opts.storeController, depGraph.depNodes, {
      allowBuild: depGraph.allowBuild,
      deferDependencyBuilds: opts.deferDependencyBuilds === true,
      force: opts.force,
      disableRelinkLocalDirDeps: opts.disableRelinkLocalDirDeps,
      depGraph: depGraph.graph,
      depsStateCache: ctx.depsStateCache,
      enableGlobalVirtualStore: opts.enableGlobalVirtualStore,
      ignoreScripts: opts.ignoreScripts,
      lockfileDir: opts.lockfileDir,
      nodeVersion: ctx.rootRuntimeNodeVersion,
      sideEffectsCacheRead: opts.sideEffectsCacheRead,
      remoteSideEffectsCache: opts.remoteSideEffectsCache,
      pnprServer: opts.pnprServer,
      configByUri: opts.configByUri,
      storeDir: opts.storeDir,
      supportedArchitectures: opts.supportedArchitectures,
    }),
  ])
  dropFetchFailedPackages(depGraph, fetchFailedDirs)
}

/**
 * Removes the optional packages that could not be fetched from the graph, so
 * they are not hoisted, built, or linked into the projects that depend on them.
 */
function dropFetchFailedPackages (depGraph: HeadlessDepGraph, fetchFailedDirs: Set<string>): void {
  if (fetchFailedDirs.size === 0) return
  for (const dir of fetchFailedDirs) {
    delete depGraph.graph[dir]
  }
  for (const directDependencies of Object.values(depGraph.directDependenciesByImporterId)) {
    for (const [alias, dir] of Object.entries(directDependencies)) {
      if (fetchFailedDirs.has(dir)) {
        delete directDependencies[alias]
      }
    }
  }
}

function shouldHoistDependencies ({ opts, skipPostImportLinking }: HeadlessContext): boolean {
  return opts.ignorePackageManifest !== true && !skipPostImportLinking && (opts.hoistPattern != null || opts.publicHoistPattern != null)
}

async function hoistDependencies (ctx: HeadlessContext, depGraph: HeadlessDepGraph): Promise<HoistedDependencies> {
  const { opts } = ctx
  const allImportersIncluded = equals([...depGraph.importerIds].sort(), Object.keys(ctx.wantedLockfile.importers).sort())
  const priorWorkspaceProjectIds = await findPriorWorkspaceProjectIds(ctx, { allImportersIncluded, importerIds: depGraph.importerIds })
  // With the full graph the recomputed hoist map is complete, so it
  // replaces the recorded one and drops the entries this install made
  // ineligible. The incremental graph only knows the packages it
  // imported, so there the recorded map fills in the rest.
  const hoisted = await hoistIntoModulesDirs(ctx, depGraph, priorWorkspaceProjectIds)
  // The recomputed map only replaces the recorded one when the graph is
  // the whole workspace: a filtered install hoists from the filtered
  // lockfile, so replacing there would forget the unselected importers'
  // entries. Everywhere else the recorded map fills in what the graph
  // does not know.
  const hoistMapIsComplete = depGraph.includeUnchangedDeps && allImportersIncluded
  if (hoistMapIsComplete) return hoisted
  const retainedHoistedDependencies = Object.fromEntries(
    Object.entries(opts.hoistedDependencies)
      .filter(([key]) => !priorWorkspaceProjectIds.has(key as ProjectId))
  ) as HoistedDependencies
  return { ...retainedHoistedDependencies, ...hoisted }
}

async function hoistIntoModulesDirs (
  ctx: HeadlessContext,
  depGraph: HeadlessDepGraph,
  priorWorkspaceProjectIds: Set<ProjectId>
): Promise<HoistedDependencies> {
  const { opts } = ctx
  return await hoist({
    extraNodePath: opts.extraNodePaths,
    graph: depGraph.graph,
    directDependencyAliases: opts.hoistWorkspacePackages ? getDirectDependencyAliases(ctx) : undefined,
    directDepsByImporterId: Object.fromEntries(Object.entries(depGraph.directDependenciesByImporterId).map(([projectId, deps]) => [
      projectId,
      new Map(Object.entries(deps)),
    ])),
    importerIds: depGraph.importerIds,
    preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
    privateHoistedModulesDir: ctx.hoistedModulesDir,
    privateHoistPattern: opts.hoistPattern ?? [],
    publicHoistedModulesDir: ctx.publicHoistedModulesDir,
    publicHoistPattern: opts.publicHoistPattern ?? [],
    virtualStoreDir: ctx.virtualStoreDir,
    virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
    hoistedWorkspacePackages: opts.hoistWorkspacePackages ? getHoistedWorkspacePackages(opts.allProjects) : undefined,
    beforeWorkspaceLinks: async (nextWorkspaceHoists: HoistedDependencies) => pruneStaleWorkspaceHoists(
      opts.hoistedDependencies,
      nextWorkspaceHoists,
      priorWorkspaceProjectIds,
      ctx.hoistedModulesDir,
      ctx.publicHoistedModulesDir
    ),
    skipped: opts.skipped,
  }) ?? {}
}

/**
 * The aliases of every importer in the wanted lockfile, not only the selected
 * ones, so that a filtered install hoists the same packages as a full one.
 */
function getDirectDependencyAliases ({ opts, skipped, wantedLockfile }: HeadlessContext): string[] {
  return Object.values(wantedLockfile.importers).flatMap((importer) => {
    const refs = {
      ...(opts.include.devDependencies ? importer.devDependencies : {}),
      ...(opts.include.dependencies ? importer.dependencies : {}),
      ...(opts.include.dependencies && opts.include.optionalDependencies ? importer.optionalDependencies : {}),
    }
    return Object.entries(refs)
      .filter(entry => isHoistableDirectDependencyAlias(entry, opts, skipped))
      .map(([alias]) => alias)
  })
}

function isHoistableDirectDependencyAlias (
  [alias, ref]: [string, string],
  opts: HeadlessContext['opts'],
  skipped: Set<DepPath>
): boolean {
  if (opts.skipRuntimes && ref.startsWith('runtime:')) return false
  const depPath = dp.refToRelative(ref, alias)
  return depPath != null && !skipped.has(depPath)
}

async function findPriorWorkspaceProjectIds (
  ctx: HeadlessContext,
  { allImportersIncluded, importerIds }: { allImportersIncluded: boolean, importerIds: ProjectId[] }
): Promise<Set<ProjectId>> {
  const candidateKeys = Object.keys(ctx.opts.hoistedDependencies)
    .filter(key => allImportersIncluded || importerIds.includes(key as ProjectId))
  const projectIds = await Promise.all(candidateKeys.map(async (key) => getPriorWorkspaceProjectId(ctx, key)))
  return new Set(projectIds.filter(projectId => projectId != null))
}

async function getPriorWorkspaceProjectId (ctx: HeadlessContext, key: string): Promise<ProjectId | undefined> {
  const { currentLockfile, wantedLockfile } = ctx
  if (currentLockfile?.packages?.[key as DepPath] != null || wantedLockfile.packages?.[key as DepPath] != null) return undefined
  const projectId = key as ProjectId
  if (currentLockfile?.importers[projectId] != null || wantedLockfile.importers[projectId] != null) return projectId
  return await workspaceHoistPointsToProject(projectId, ctx.opts.hoistedDependencies[projectId], {
    lockfileDir: ctx.lockfileDir,
    privateHoistedModulesDir: ctx.hoistedModulesDir,
    publicHoistedModulesDir: ctx.publicHoistedModulesDir,
  }) ? projectId : undefined
}

async function symlinkDirectDependenciesOfProjects (ctx: HeadlessContext, depGraph: HeadlessDepGraph): Promise<number> {
  const { opts } = ctx
  /** Skip linking and due to no project manifest */
  if (opts.ignorePackageManifest || ctx.skipPostImportLinking) return 0
  return symlinkDirectDependencies({
    dedupe: Boolean(opts.dedupeDirectDeps),
    directDependenciesByImporterId: depGraph.directDependenciesByImporterId,
    filteredLockfile: depGraph.filteredLockfile,
    lockfileDir: ctx.lockfileDir,
    projects: ctx.selectedProjects,
    registriesByScope: opts.registriesByScope,
    symlink: opts.symlink,
  })
}
