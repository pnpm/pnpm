import path from 'node:path'

import { linkBins } from '@pnpm/bins.linker'
import {
  stageLogger,
  statsLogger,
  summaryLogger,
} from '@pnpm/core-loggers'
import type { DependenciesGraphNode } from '@pnpm/deps.graph-builder'
import { prune } from '@pnpm/installing.linking.modules-cleaner'
import { filterLockfileByEngine } from '@pnpm/lockfile.filtering'
import { logger, streamParser } from '@pnpm/logger'

import { buildDependencies } from './buildDependencies.js'
import { createHeadlessContext, type HeadlessContext, type HeadlessDepGraph, type LinkedDependencies } from './context.js'
import { createHeadlessDepGraph } from './createHeadlessDepGraph.js'
import { extendProjectsWithTargetDirs } from './extendProjectsWithTargetDirs.js'
import { linkHoistedLayout } from './linkHoistedLayout.js'
import { linkIsolatedLayout } from './linkIsolatedLayout.js'
import { updatePackageMap } from './packageMap.js'
import { reconcilePendingBuilds, runProjectLifecycleScripts } from './projectLifecycle.js'
import type { HeadlessOptions, InstallationResult } from './types.js'
import { writeModulesDirState } from './writeModulesDirState.js'
export { extendProjectsWithTargetDirs, getInjectedDeps } from './extendProjectsWithTargetDirs.js'
export type {
  HeadlessOptions,
  InstallationResult,
  InstallationResultStats,
  Project,
  ReporterFunction,
} from './types.js'
export type { HoistingLimits } from '@pnpm/installing.linking.real-hoist'

export async function headlessInstall (opts: HeadlessOptions): Promise<InstallationResult> {
  const reporter = opts.reporter
  const hasReporter = (reporter != null) && typeof reporter === 'function'
  if (hasReporter) {
    streamParser.on('data', reporter)
  }
  try {
    return await installFromLockfile(opts)
  } finally {
    if (hasReporter) {
      streamParser.removeListener('data', reporter)
    }
  }
}

async function installFromLockfile (opts: HeadlessOptions): Promise<InstallationResult> {
  const ctx = await createHeadlessContext(opts)
  const removed = await pruneRemovedDependencies(ctx)
  stageLogger.debug({
    prefix: ctx.lockfileDir,
    stage: 'importing_started',
  })
  const depGraph = await createHeadlessDepGraph(ctx)
  const { heldBackBinsDirs, linkedToRoot, newHoistedDependencies } = await linkDependencies(ctx, depGraph)
  const shouldWritePackageMap = await updatePackageMap(ctx, depGraph)
  reconcilePendingBuilds(ctx, depGraph.depNodes)
  const ignoredBuilds = await buildDependencies(ctx, depGraph, shouldWritePackageMap)
  await linkHeldBackBins(heldBackBinsDirs, opts.preferSymlinkedExecutables)
  const projectsToBeBuilt = extendProjectsWithTargetDirs(ctx.selectedProjects, depGraph.injectionTargetsByDepPath, opts.lockfileDir)
  await writeModulesDirState(ctx, depGraph, { ignoredBuilds, newHoistedDependencies })
  await waitForPackageRequests(depGraph.depNodes)
  if (!opts.omitSummaryLog) {
    summaryLogger.debug({ prefix: ctx.lockfileDir })
  }
  await runProjectLifecycleScripts(ctx, projectsToBeBuilt, shouldWritePackageMap)
  return {
    stats: {
      added: depGraph.added,
      removed,
      linkedToRoot,
    },
    ignoredBuilds,
  }
}

async function pruneRemovedDependencies (ctx: HeadlessContext): Promise<number> {
  const { opts, currentLockfile, lockfileDir } = ctx
  if (opts.nodeLinker === 'hoisted') return 0
  if (currentLockfile == null || opts.ignorePackageManifest) {
    statsLogger.debug({
      prefix: lockfileDir,
      removed: 0,
    })
    return 0
  }
  const removedDepPaths = await prune(
    ctx.selectedProjects,
    {
      currentLockfile,
      dedupeDirectDeps: opts.dedupeDirectDeps,
      dryRun: false,
      hoistedDependencies: opts.hoistedDependencies,
      hoistedModulesDir: (opts.hoistPattern == null) ? undefined : ctx.hoistedModulesDir,
      include: opts.include,
      lockfileDir,
      pruneStore: opts.pruneStore,
      pruneVirtualStore: opts.pruneVirtualStore,
      publicHoistedModulesDir: (opts.publicHoistPattern == null) ? undefined : ctx.publicHoistedModulesDir,
      resolvePeersFromWorkspaceRoot: opts.resolvePeersFromWorkspaceRoot,
      skipped: ctx.skipped,
      storeController: opts.storeController,
      virtualStoreDir: ctx.virtualStoreDir,
      virtualStoreDirMaxLength: opts.virtualStoreDirMaxLength,
      wantedLockfile: filterLockfileByEngine(ctx.wantedLockfile, ctx.filterOpts).lockfile,
    }
  )
  return removedDepPaths.size
}

async function linkDependencies (ctx: HeadlessContext, depGraph: HeadlessDepGraph): Promise<LinkedDependencies> {
  const { opts } = ctx
  const { hierarchy, prevGraph } = depGraph
  if (opts.nodeLinker === 'hoisted' && hierarchy && prevGraph) {
    return linkHoistedLayout(ctx, depGraph, { hierarchy, prevGraph })
  }
  if (opts.enableModulesDir !== false || opts.enableGlobalVirtualStore) {
    return linkIsolatedLayout(ctx, depGraph)
  }
  return { heldBackBinsDirs: [], linkedToRoot: 0 }
}

/**
 * Links the bins the hoisted linker held back in nested `.bin` directories
 * while the builds that may create their targets were pending.
 */
async function linkHeldBackBins (heldBackBinsDirs: string[], preferSymlinkedExecutables: boolean | undefined): Promise<void> {
  await Promise.all(heldBackBinsDirs.map(async (binsDir) => linkBins(path.dirname(binsDir), binsDir, {
    allowExoticManifests: true,
    preferSymlinkedExecutables,
    warn: (message) => logger.info({ message, prefix: path.dirname(path.dirname(binsDir)) }),
  })))
}

async function waitForPackageRequests (depNodes: DependenciesGraphNode[]): Promise<void> {
  await Promise.all(depNodes.map(async ({ fetching }) => {
    try {
      await fetching?.()
    } catch {}
  }))
}
