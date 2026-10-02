import path from 'node:path'

import { createAllowBuildFunction } from '@pnpm/building.policy'
import { type DepsGraph, iterateHashedGraphNodes, iteratePkgMeta, lockfileToDepGraph } from '@pnpm/deps.graph-hasher'
import {
  findLockedRootNodeRuntime,
} from '@pnpm/lockfile.utils'
import { logger } from '@pnpm/logger'
import { ImmutableStoreIndex, StoreIndex } from '@pnpm/store.index'
import type {
  DepPath,
} from '@pnpm/types'
import { scheduleGraph, type TaskCompletion } from '@pnpm/workspace.task-scheduler'

import { getPkgModulesDir, runBuild } from './buildSinglePackage.js'
import type { StrictBuildOptions } from './extendBuildOptions.js'
import { getGraphToBuild, relinkBins } from './rebuildGraph.js'
import type {
  RebuildPackagesContext,
  RebuildPackagesResult,
  RebuildState,
} from './rebuildTypes.js'

export async function rebuildPackages (
  ctx: RebuildPackagesContext,
  opts: StrictBuildOptions
): Promise<RebuildPackagesResult> {
  const graph = getGraphToBuild(ctx, opts)
  const state = createRebuildState(ctx, opts)
  let firstError: unknown
  await scheduleGraph(graph, {
    bail: false,
    concurrency: opts.childConcurrency || 5,
    runNode: async (depPath): Promise<TaskCompletion> => {
      if (!ctx.pkgsToRebuild.has(depPath) || ctx.skipped.has(depPath)) return 'passed'
      try {
        await runBuild(depPath, state)
        return 'passed'
      } catch (error: unknown) {
        firstError ??= error
        return 'failed'
      }
    },
    onNodeSkipped: () => {},
  })
  state.storeIndex?.close()
  if (firstError != null) throw firstError

  if (state.builtDepPaths.size > 0) {
    await relinkBins(state, getPkgModulesDir)
  }

  return { pkgsThatWereRebuilt: state.pkgsThatWereRebuilt, ignoredPkgs: state.ignoredPkgs }
}

function createRebuildState (ctx: RebuildPackagesContext, opts: StrictBuildOptions): RebuildState {
  const depGraph = lockfileToDepGraph(ctx.currentLockfile, opts.supportedArchitectures)
  const nodeVersion = findLockedRootNodeRuntime(ctx.currentLockfile)?.version
  const ignoredPkgs = new Set<DepPath>()
  const allowBuildByPolicy = createAllowBuildFunction(opts) ?? (() => undefined)
  return {
    ctx,
    opts,
    depGraph,
    depsStateCache: {},
    nodeVersion,
    pkgSnapshots: ctx.currentLockfile.packages ?? {},
    allowBuild: (depPath: DepPath) => {
      switch (allowBuildByPolicy(depPath)) {
        case true: return true
        case undefined: {
          ignoredPkgs.add(depPath)
          break
        }
        case false: break
      }
      return false
    },
    ignoredPkgs,
    pkgsThatWereRebuilt: new Set<string>(),
    builtDepPaths: new Set<string>(),
    storeIndex: openStoreIndex(opts),
    gvsDirByDepPath: getGvsDirByDepPath({ ctx, opts, depGraph, nodeVersion, allowBuild: allowBuildByPolicy }),
    warn: (message: string) => {
      logger.info({ message, prefix: opts.dir })
    },
  }
}

function openStoreIndex (opts: StrictBuildOptions): ImmutableStoreIndex | StoreIndex | undefined {
  return opts.skipIfHasSideEffectsCache
    ? (opts.frozenStore ? new ImmutableStoreIndex(opts.storeDir) : new StoreIndex(opts.storeDir))
    : undefined
}

function getGvsDirByDepPath (
  { ctx, opts, depGraph, nodeVersion, allowBuild }: {
    ctx: RebuildPackagesContext
    opts: StrictBuildOptions
    depGraph: DepsGraph<DepPath>
    nodeVersion: string | undefined
    allowBuild: (depPath: DepPath) => boolean | undefined
  }
): Map<DepPath, string> {
  const gvsDirByDepPath = new Map<DepPath, string>()
  if (!opts.enableGlobalVirtualStore) return gvsDirByDepPath
  const globalVirtualStoreDir = opts.globalVirtualStoreDir ?? path.join(opts.storeDir, 'links')
  for (const { hash, pkgMeta } of iterateHashedGraphNodes(
    depGraph,
    iteratePkgMeta(ctx.currentLockfile, depGraph),
    {
      allowBuild,
      supportedArchitectures: opts.supportedArchitectures,
      nodeVersion,
      lockfileDir: opts.lockfileDir,
    }
  )) {
    gvsDirByDepPath.set(pkgMeta.depPath, path.join(globalVirtualStoreDir, hash))
  }
  return gvsDirByDepPath
}
