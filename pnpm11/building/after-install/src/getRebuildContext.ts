import { promises as fs } from 'node:fs'
import path from 'node:path'
import util from 'node:util'

import { createAllowBuildFunction } from '@pnpm/building.policy'
import { iterateHashedGraphNodes, iteratePkgMeta, lockfileToDepGraph } from '@pnpm/deps.graph-hasher'
import { refToRelative } from '@pnpm/deps.path'
import { PnpmError } from '@pnpm/error'
import { safeJoinModulesDir } from '@pnpm/fs.symlink-dependency'
import { getContext, type PnpmContext, type ProjectOptions } from '@pnpm/installing.context'
import { headlessInstall } from '@pnpm/installing.deps-restorer'
import { calcPatchHashes, resolvePatchedDependencies } from '@pnpm/lockfile.settings-checker'
import { findLockedRootNodeRuntime } from '@pnpm/lockfile.utils'
import { groupPatchedDependenciesWithPaths } from '@pnpm/patching.config'
import type { DepPath } from '@pnpm/types'

import type { StrictBuildOptions } from './extendBuildOptions.js'

export async function getRebuildContext (projects: ProjectOptions[], opts: StrictBuildOptions): Promise<PnpmContext> {
  const ctx = await getContext({ ...opts, allProjects: projects })
  if (!opts.enableGlobalVirtualStore || opts.nodeLinker !== 'isolated' || !ctx.currentLockfile.packages) return ctx
  const allowBuild = createAllowBuildFunction(opts)
  const previousAllowBuild = createAllowBuildFunction({ allowBuilds: ctx.modulesFile?.allowBuilds })
  const policyChanged = Object.keys(ctx.currentLockfile.packages).some((depPath) =>
    (allowBuild?.(depPath as DepPath) === true) !== (previousAllowBuild?.(depPath as DepPath) === true)
  )
  if (!policyChanged && await projectsUseCurrentBuildSlots(ctx, opts)) return ctx

  const resolvedPatches = resolvePatchedDependencies(opts.patchedDependencies, opts.lockfileDir)
  const patchHashes = resolvedPatches ? await calcPatchHashes(resolvedPatches) : {}
  const installedPatchHashes = ctx.currentLockfile.patchedDependencies ?? {}
  const patchesMatchInstallation = Object.keys(patchHashes).length === Object.keys(installedPatchHashes).length &&
    Object.entries(installedPatchHashes).every(([key, hash]) => patchHashes[key] === hash)
  if (!patchesMatchInstallation) {
    throw new PnpmError('LOCKFILE_CONFIG_MISMATCH', 'Cannot rebuild because patchedDependencies differ from the installed lockfile. Run "pnpm install" first.')
  }

  // A policy change also changes the slots of packages depending on the build.
  // Materialize the installed graph before running only the selected scripts.
  await headlessInstall({
    ...opts,
    ...ctx,
    allProjects: ctx.projects,
    selectedProjectDirs: projects.map(({ rootDir }) => rootDir),
    currentEngine: { nodeVersion: process.version, pnpmVersion: opts.packageManager.version },
    nodeVersionFromEnginesRuntime: true,
    engineStrict: false,
    force: false,
    globalVirtualStoreDir: opts.globalVirtualStoreDir ?? path.join(opts.storeDir, 'links'),
    ignoreScripts: true,
    wantedLockfile: ctx.currentLockfile,
    useLockfile: false,
    pruneStore: false,
    pruneVirtualStore: false,
    sideEffectsCacheRead: false,
    omitSummaryLog: true,
    reporter: undefined,
    patchedDependencies: groupPatchedDependenciesWithPaths(
      ctx.currentLockfile.patchedDependencies,
      resolvedPatches
    ),
  })
  const rebuiltContext = await getContext({ ...opts, allProjects: projects })
  if (rebuiltContext.modulesFile) {
    rebuiltContext.modulesFile.ignoredBuilds = ctx.modulesFile?.ignoredBuilds
  }
  return rebuiltContext
}

async function projectsUseCurrentBuildSlots (ctx: PnpmContext, opts: StrictBuildOptions): Promise<boolean> {
  const graph = lockfileToDepGraph(ctx.currentLockfile, opts.supportedArchitectures)
  const globalVirtualStoreDir = opts.globalVirtualStoreDir ?? path.join(opts.storeDir, 'links')
  const packageDirs = new Map<DepPath, string>()
  for (const { hash, pkgMeta } of iterateHashedGraphNodes(graph, iteratePkgMeta(ctx.currentLockfile, graph), {
    allowBuild: createAllowBuildFunction(opts) ?? (() => undefined),
    supportedArchitectures: opts.supportedArchitectures,
    nodeVersion: findLockedRootNodeRuntime(ctx.currentLockfile)?.version,
    lockfileDir: opts.lockfileDir,
  })) {
    packageDirs.set(pkgMeta.depPath, safeJoinModulesDir(path.join(globalVirtualStoreDir, hash, 'node_modules'), pkgMeta.name))
  }
  const checks = Object.values(ctx.projects).flatMap(({ id, modulesDir }) => {
    const importer = ctx.currentLockfile.importers[id]
    if (!importer) return []
    return Object.entries({ ...importer.dependencies, ...importer.devDependencies, ...importer.optionalDependencies })
      .map(async ([alias, ref]) => {
        const depPath = refToRelative(ref, alias)
        const expected = depPath && packageDirs.get(depPath)
        if (!expected) return true
        try {
          const [actualDir, expectedDir] = await Promise.all([
            fs.realpath(safeJoinModulesDir(modulesDir, alias)),
            fs.realpath(expected),
          ])
          return actualDir === expectedDir
        } catch (error: unknown) {
          if (util.types.isNativeError(error) && 'code' in error && error.code === 'ENOENT') return false
          throw error
        }
      })
  })
  return (await Promise.all(checks)).every(Boolean)
}
