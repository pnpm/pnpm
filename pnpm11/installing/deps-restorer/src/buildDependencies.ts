import path from 'node:path'

import { buildModules } from '@pnpm/building.during-install'
import { isBuildExplicitlyDisallowed } from '@pnpm/building.policy'
import { installabilityUnderForce } from '@pnpm/config.package-is-installable'
import { makeNodeRequireOption } from '@pnpm/exec.lifecycle'
import { logger } from '@pnpm/logger'
import type { DepPath, HoistedDependencies, IgnoredBuilds } from '@pnpm/types'
import { union } from 'ramda'

import type { HeadlessContext, HeadlessDepGraph } from './context.js'
import { linkAllBins } from './linkAllBins.js'
import { linkRuntimeBinsOfImporters } from './linkBinsOfImporters.js'
import { addPackageMapOption } from './packageMap.js'
import { mergeIntoCurrentLockfile } from './writeModulesDirState.js'

export async function buildDependencies (
  ctx: HeadlessContext,
  depGraph: HeadlessDepGraph,
  {
    hoistedDependencies,
    shouldWritePackageMap,
  }: {
    hoistedDependencies?: HoistedDependencies
    shouldWritePackageMap: boolean
  }
): Promise<{
  builtDepPaths?: Set<DepPath>
  builtPackageDirs?: Set<string>
  ignoredBuilds?: IgnoredBuilds
}> {
  const { opts } = ctx
  const buildsOrPatchesApply = !opts.ignoreScripts || Object.keys(opts.patchedDependencies ?? {}).length > 0
  if (!buildsOrPatchesApply || opts.enableModulesDir === false) return {}
  const directNodes = collectDirectNodes(depGraph)
  const extraBinPaths = [...opts.extraBinPaths ?? []]
  if (opts.hoistPattern != null) {
    extraBinPaths.unshift(path.join(ctx.hoistedModulesDir, '.bin'))
  }
  const extraEnv = createBuildExtraEnv(ctx, shouldWritePackageMap)
  if (!opts.ignoreScripts && !opts.virtualStoreOnly) {
    await linkRuntimeBinsOfImporters({
      directDependenciesByImporterId: depGraph.directDependenciesByImporterId,
      extraNodePaths: opts.extraNodePaths,
      graph: depGraph.graph,
      preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
      projects: ctx.selectedProjects,
    })
  }
  // Dependency lifecycle scripts must not run on an unverified lockfile.
  await opts.verifyLockfile?.()
  const { ignoredBuilds, builtDepPaths, builtPackageDirs } = await runBuildModules(ctx, depGraph, {
    directNodes,
    extraBinPaths,
    extraEnv,
    hoistedDependencies,
  })
  if (builtPackageDirs?.size && opts.nodeLinker !== 'hoisted' && !ctx.skipPostImportLinking) {
    await relinkBuiltVirtualStoreBins(ctx, depGraph, builtPackageDirs)
  }
  return {
    builtDepPaths,
    builtPackageDirs,
    ignoredBuilds: addPreviouslyIgnoredBuilds(ignoredBuilds, ctx, depGraph),
  }
}

async function relinkBuiltVirtualStoreBins (
  ctx: HeadlessContext,
  depGraph: HeadlessDepGraph,
  builtPackageDirs: ReadonlySet<string>
): Promise<void> {
  const { opts } = ctx
  const affectedNodes = depGraph.depNodes.filter((depNode) =>
    builtPackageDirs.has(path.normalize(depNode.dir)) ||
    Object.values(depNode.children).some((childDir) => builtPackageDirs.has(path.normalize(childDir)))
  )
  if (affectedNodes.length === 0) return
  await linkAllBins(depGraph.graph, {
    depNodes: affectedNodes,
    extraNodePaths: opts.extraNodePaths,
    forceForPackages: builtPackageDirs,
    optional: opts.include.optionalDependencies,
    preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
    warn: (message: string) => {
      logger.info({ message, prefix: ctx.lockfileDir })
    },
  })
}

function collectDirectNodes ({ directDependenciesByImporterId, graph, importerIds }: HeadlessDepGraph): string[] {
  const directNodes = new Set<string>()
  for (const id of union(importerIds, ['.'])) {
    const directDependencies = directDependenciesByImporterId[id]
    for (const alias in directDependencies) {
      const loc = directDependencies[alias]
      if (!graph[loc]) continue
      directNodes.add(loc)
    }
  }
  return Array.from(directNodes)
}

function createBuildExtraEnv (ctx: HeadlessContext, shouldWritePackageMap: boolean): Record<string, string> | undefined {
  const { opts } = ctx
  let extraEnv: Record<string, string> | undefined = opts.extraEnv
  // Only point Node at the loader when it was actually written —
  // `--require` on a missing file fails the script before it runs.
  if (opts.enablePnp && !ctx.skipPostImportLinking) {
    extraEnv = {
      ...extraEnv,
      ...makeNodeRequireOption(path.join(opts.lockfileDir, '.pnp.cjs'), extraEnv),
    }
  }
  if (opts.nodeExperimentalPackageMap && shouldWritePackageMap) {
    extraEnv = addPackageMapOption(extraEnv, ctx.rootModulesDir)
  }
  return extraEnv
}

async function runBuildModules (
  ctx: HeadlessContext,
  depGraph: HeadlessDepGraph,
  {
    directNodes,
    extraBinPaths,
    extraEnv,
    hoistedDependencies,
  }: {
    directNodes: string[]
    extraBinPaths: string[]
    extraEnv: Record<string, string> | undefined
    hoistedDependencies?: HoistedDependencies
  }
): ReturnType<typeof buildModules> {
  const { opts } = ctx
  return buildModules(depGraph.graph, directNodes, {
    engineStrict: installabilityUnderForce(opts).engineStrict,
    engineNodeVersion: ctx.currentEngine.nodeVersion,
    linkedModulesDirs: [...Object.values(opts.allProjects).map(({ modulesDir }) => modulesDir), ctx.hoistedModulesDir],
    skipped: ctx.skipped,
    allowBuild: depGraph.allowBuild,
    childConcurrency: opts.childConcurrency,
    extraBinPaths,
    extraNodePaths: opts.extraNodePaths,
    extraEnv,
    depsStateCache: ctx.depsStateCache,
    ignoreScripts: opts.ignoreScripts,
    hoistedDependencies: hoistedDependencies ?? opts.hoistedDependencies,
    hoistedLocations: depGraph.hoistedLocations,
    hoistedModulesDir: ctx.hoistedModulesDir,
    lockfileDir: ctx.lockfileDir,
    nodeVersion: ctx.rootRuntimeNodeVersion,
    optional: opts.include.optionalDependencies,
    preferSymlinkedExecutables: opts.preferSymlinkedExecutables,
    rootModulesDir: ctx.virtualStoreDir,
    scriptsPrependNodePath: opts.scriptsPrependNodePath,
    scriptShell: opts.scriptShell,
    shellEmulator: opts.shellEmulator,
    sideEffectsCacheWrite: opts.sideEffectsCacheWrite,
    remoteSideEffectsCache: opts.remoteSideEffectsCache,
    storeController: opts.storeController,
    supportedArchitectures: opts.supportedArchitectures,
    unsafePerm: opts.unsafePerm,
    userAgent: opts.userAgent,
    enableGlobalVirtualStore: opts.enableGlobalVirtualStore,
    configByUri: opts.configByUri,
    pnprServer: opts.pnprServer,
  })
}

function addPreviouslyIgnoredBuilds (
  ignoredBuilds: IgnoredBuilds | undefined,
  ctx: HeadlessContext,
  depGraph: HeadlessDepGraph
): IgnoredBuilds | undefined {
  const { opts } = ctx
  if (!opts.modulesFile?.ignoredBuilds?.size) return ignoredBuilds
  const currentPackages = mergeIntoCurrentLockfile(ctx, depGraph).packages
  const allIgnoredBuilds = ignoredBuilds ?? new Set()
  for (const ignoredBuild of opts.modulesFile.ignoredBuilds.values()) {
    if (currentPackages?.[ignoredBuild] && !isBuildExplicitlyDisallowed(ignoredBuild, depGraph.allowBuild)) {
      allIgnoredBuilds.add(ignoredBuild)
    }
  }
  return allIgnoredBuilds
}
