import { promises as fs } from 'node:fs'
import path from 'node:path'

import { createAllowBuildFunction } from '@pnpm/building.policy'
import { iterateHashedGraphNodes, iteratePkgMeta, lockfileToDepGraph } from '@pnpm/deps.graph-hasher'
import { refToRelative } from '@pnpm/deps.path'
import { isError, PnpmError } from '@pnpm/error'
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
  if (!buildPolicyChanged(ctx, opts) && await projectsUseCurrentBuildSlots(ctx, opts)) return ctx

  const resolvedPatches = resolvePatchedDependencies(opts.patchedDependencies, opts.lockfileDir)
  await ensurePatchesMatchInstallation(ctx, resolvedPatches)

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
    globalVirtualStoreDir: getGlobalVirtualStoreDir(opts),
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

function getGlobalVirtualStoreDir (opts: StrictBuildOptions): string {
  return opts.globalVirtualStoreDir ?? path.join(opts.storeDir, 'links')
}

/** Whether the build policy allows a different set of installed packages to build than at install time. */
function buildPolicyChanged (ctx: PnpmContext, opts: StrictBuildOptions): boolean {
  const allowBuild = createAllowBuildFunction(opts)
  const previousAllowBuild = createAllowBuildFunction({ allowBuilds: ctx.modulesFile?.allowBuilds })
  return Object.keys(ctx.currentLockfile.packages ?? {}).some((depPath) =>
    (allowBuild?.(depPath as DepPath) === true) !== (previousAllowBuild?.(depPath as DepPath) === true)
  )
}

async function ensurePatchesMatchInstallation (
  ctx: PnpmContext,
  resolvedPatches: ReturnType<typeof resolvePatchedDependencies>
): Promise<void> {
  const patchHashes = resolvedPatches ? await calcPatchHashes(resolvedPatches) : {}
  const installedPatchHashes = ctx.currentLockfile.patchedDependencies ?? {}
  const patchesMatchInstallation = Object.keys(patchHashes).length === Object.keys(installedPatchHashes).length &&
    Object.entries(installedPatchHashes).every(([key, hash]) => patchHashes[key] === hash)
  if (!patchesMatchInstallation) {
    throw new PnpmError('LOCKFILE_CONFIG_MISMATCH', 'Cannot rebuild because patchedDependencies differ from the installed lockfile. Run "pnpm install" first.')
  }
}

async function projectsUseCurrentBuildSlots (ctx: PnpmContext, opts: StrictBuildOptions): Promise<boolean> {
  const packageDirs = getBuildSlotPackageDirs(ctx, opts)
  const checks = Object.values(ctx.projects).flatMap(({ id, modulesDir }) => {
    const importer = ctx.currentLockfile.importers[id]
    if (!importer) return []
    return Object.entries({ ...importer.dependencies, ...importer.devDependencies, ...importer.optionalDependencies })
      .map(async ([alias, ref]) => {
        const depPath = refToRelative(ref, alias)
        const expected = depPath && packageDirs.get(depPath)
        if (!expected) return true
        return isSameRealPath(safeJoinModulesDir(modulesDir, alias), expected)
      })
  })
  return (await Promise.all(checks)).every(Boolean)
}

/** The directory each package has in the global virtual store under the current build policy. */
function getBuildSlotPackageDirs (ctx: PnpmContext, opts: StrictBuildOptions): Map<DepPath, string> {
  const graph = lockfileToDepGraph(ctx.currentLockfile, opts.supportedArchitectures)
  const globalVirtualStoreDir = getGlobalVirtualStoreDir(opts)
  const packageDirs = new Map<DepPath, string>()
  for (const { hash, pkgMeta } of iterateHashedGraphNodes(graph, iteratePkgMeta(ctx.currentLockfile, graph), {
    allowBuild: createAllowBuildFunction(opts) ?? (() => undefined),
    supportedArchitectures: opts.supportedArchitectures,
    nodeVersion: findLockedRootNodeRuntime(ctx.currentLockfile)?.version,
    lockfileDir: opts.lockfileDir,
  })) {
    packageDirs.set(pkgMeta.depPath, safeJoinModulesDir(path.join(globalVirtualStoreDir, hash, 'node_modules'), pkgMeta.name))
  }
  return packageDirs
}

/** Whether both paths resolve to the same directory. `false` when either is missing. */
async function isSameRealPath (actualPath: string, expectedPath: string): Promise<boolean> {
  try {
    const [actualDir, expectedDir] = await Promise.all([
      fs.realpath(actualPath),
      fs.realpath(expectedPath),
    ])
    return actualDir === expectedDir
  } catch (error: unknown) {
    if (isError(error) && 'code' in error && error.code === 'ENOENT') return false
    throw error
  }
}
