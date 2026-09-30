import path from 'node:path'

import { buildModules } from '@pnpm/building.during-install'
import { isBuildExplicitlyDisallowed } from '@pnpm/building.policy'
import { installabilityUnderForce } from '@pnpm/config.package-is-installable'
import { makeNodeRequireOption } from '@pnpm/exec.lifecycle'
import type { IgnoredBuilds } from '@pnpm/types'
import { union } from 'ramda'

import type { HeadlessContext, HeadlessDepGraph } from './context.js'
import { linkRuntimeBinsOfImporters } from './linkBinsOfImporters.js'
import { addPackageMapOption } from './packageMap.js'

export async function buildDependencies (
  ctx: HeadlessContext,
  depGraph: HeadlessDepGraph,
  shouldWritePackageMap: boolean
): Promise<IgnoredBuilds | undefined> {
  const { opts } = ctx
  const buildsOrPatchesApply = !opts.ignoreScripts || Object.keys(opts.patchedDependencies ?? {}).length > 0
  if (!buildsOrPatchesApply || opts.enableModulesDir === false) return undefined
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
  const { ignoredBuilds } = await runBuildModules(ctx, depGraph, { directNodes, extraBinPaths, extraEnv })
  return addPreviouslyIgnoredBuilds(ignoredBuilds, ctx, depGraph)
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
  { directNodes, extraBinPaths, extraEnv }: { directNodes: string[], extraBinPaths: string[], extraEnv: Record<string, string> | undefined }
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
    extraEnv,
    depsStateCache: ctx.depsStateCache,
    ignoreScripts: opts.ignoreScripts,
    hoistedLocations: depGraph.hoistedLocations,
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
  { opts }: HeadlessContext,
  { allowBuild, filteredLockfile }: HeadlessDepGraph
): IgnoredBuilds | undefined {
  if (!opts.modulesFile?.ignoredBuilds?.size) return ignoredBuilds
  const allIgnoredBuilds = ignoredBuilds ?? new Set()
  for (const ignoredBuild of opts.modulesFile.ignoredBuilds.values()) {
    if (filteredLockfile.packages?.[ignoredBuild] && !isBuildExplicitlyDisallowed(ignoredBuild, allowBuild)) {
      allIgnoredBuilds.add(ignoredBuild)
    }
  }
  return allIgnoredBuilds
}
