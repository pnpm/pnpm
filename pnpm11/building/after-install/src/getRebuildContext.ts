import path from 'node:path'

import { isPackagePlaced } from '@pnpm/building.during-install'
import { createAllowBuildFunction } from '@pnpm/building.policy'
import { projectLinksMatchPackageDirs } from '@pnpm/deps.graph-builder'
import { buildsDependenciesDuringInstall, iterateHashedGraphNodes, iteratePkgMeta, lockfileToDepGraph } from '@pnpm/deps.graph-hasher'
import { PnpmError } from '@pnpm/error'
import { safeJoinModulesDir } from '@pnpm/fs.symlink-dependency'
import { getContext, type PnpmContext, type ProjectOptions } from '@pnpm/installing.context'
import { headlessInstall } from '@pnpm/installing.deps-restorer'
import { calcPatchHashes, resolvePatchedDependencies } from '@pnpm/lockfile.settings-checker'
import { findLockedRootNodeRuntime } from '@pnpm/lockfile.utils'
import { groupPatchedDependenciesWithPaths } from '@pnpm/patching.config'
import type { DepPath } from '@pnpm/types'

import type { StrictBuildOptions } from './extendBuildOptions.js'
import { getSelectionBuildPolicy } from './selectionBuildPolicy.js'

/**
 * The build options of the install that places the installed graph in the
 * slots of the current build policy. Packages placed in slots keyed as built
 * are built by it. The slot paths the rebuild computes use the same options.
 */
const SLOT_PLACEMENT_BUILD_OPTIONS = { deferDependencyBuilds: true, enableGlobalVirtualStore: true, ignoreScripts: true } as const

/** Whether the packages in the slots a rebuild locates hash as built. */
export const REBUILD_BUILDS_RUN = buildsDependenciesDuringInstall(SLOT_PLACEMENT_BUILD_OPTIONS)

export type RebuildContext = PnpmContext & {
  /**
   * The global virtual store packages that the rebuild placed in the slots of
   * the current build policy, built. The rebuild does not build them again.
   */
  placedPackageDirs?: ReadonlySet<string>
}

/**
 * `selectDepPaths` picks the packages the rebuild builds from the installed
 * lockfile. When it is passed, placing the graph builds only those packages
 * and what they depend on. See `getSelectionBuildPolicy`.
 */
export async function getRebuildContext (
  projects: ProjectOptions[],
  opts: StrictBuildOptions,
  selectDepPaths?: (ctx: PnpmContext) => DepPath[]
): Promise<RebuildContext> {
  const ctx = await getContext({ ...opts, allProjects: projects })
  if (!opts.enableGlobalVirtualStore || opts.nodeLinker !== 'isolated' || !ctx.currentLockfile.packages) return ctx
  const slotPackageDirs = getBuildSlotPackageDirs(ctx, opts)
  if (!buildPolicyChanged(ctx, opts) && await projectsUseCurrentBuildSlots(ctx, slotPackageDirs)) return ctx

  const resolvedPatches = resolvePatchedDependencies(opts.patchedDependencies, opts.lockfileDir)
  await ensurePatchesMatchInstallation(ctx, resolvedPatches)

  // A policy change also changes the slots of packages depending on the build.
  // Materialize the installed graph before running only the selected scripts.
  const unplacedPackageDirs = await filterUnplaced(slotPackageDirs.values())
  const buildPolicy = await getPlacementBuildPolicy(ctx, opts, { selectDepPaths, slotPackageDirs })
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
    ...SLOT_PLACEMENT_BUILD_OPTIONS,
    ...buildPolicy,
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
  return { ...rebuiltContext, placedPackageDirs: await filterPlaced(unplacedPackageDirs) }
}

async function filterPlaced (packageDirs: string[]): Promise<Set<string>> {
  const stillUnplaced = new Set(await filterUnplaced(packageDirs))
  return new Set(packageDirs.filter((dir) => !stillUnplaced.has(dir)))
}

async function getPlacementBuildPolicy (
  ctx: PnpmContext,
  opts: StrictBuildOptions,
  { selectDepPaths, slotPackageDirs }: { selectDepPaths?: (ctx: PnpmContext) => DepPath[], slotPackageDirs: Map<DepPath, string> }
): Promise<{ allowBuilds?: Record<string, boolean>, dangerouslyAllowAllBuilds?: boolean }> {
  const selected = selectDepPaths?.(ctx)
  if (selected == null) return {}
  return {
    allowBuilds: await getSelectionBuildPolicy(ctx.currentLockfile, opts, { selected, builtSlotPackageDirs: slotPackageDirs }),
    dangerouslyAllowAllBuilds: false,
  }
}

async function filterUnplaced (packageDirs: Iterable<string>): Promise<string[]> {
  const dirs = Array.from(packageDirs)
  const placed = await Promise.all(dirs.map(isPackagePlaced))
  return dirs.filter((_, index) => !placed[index])
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

async function projectsUseCurrentBuildSlots (ctx: PnpmContext, packageDirs: Map<DepPath, string>): Promise<boolean> {
  return projectLinksMatchPackageDirs(Object.values(ctx.projects), { lockfile: ctx.currentLockfile, packageDirs })
}

/** The directory each package has in the global virtual store under the current build policy. */
function getBuildSlotPackageDirs (ctx: PnpmContext, opts: StrictBuildOptions): Map<DepPath, string> {
  const graph = lockfileToDepGraph(ctx.currentLockfile, opts.supportedArchitectures)
  const globalVirtualStoreDir = getGlobalVirtualStoreDir(opts)
  const packageDirs = new Map<DepPath, string>()
  for (const { hash, pkgMeta } of iterateHashedGraphNodes(graph, iteratePkgMeta(ctx.currentLockfile, graph), {
    allowBuild: createAllowBuildFunction(opts) ?? (() => undefined),
    buildsRun: REBUILD_BUILDS_RUN,
    supportedArchitectures: opts.supportedArchitectures,
    nodeVersion: findLockedRootNodeRuntime(ctx.currentLockfile)?.version,
    lockfileDir: opts.lockfileDir,
  })) {
    packageDirs.set(pkgMeta.depPath, safeJoinModulesDir(path.join(globalVirtualStoreDir, hash, 'node_modules'), pkgMeta.name))
  }
  return packageDirs
}
